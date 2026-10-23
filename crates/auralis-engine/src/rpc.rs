//! stdio JSON-Lines RPC 端点。
//!
//! ## 协议
//!
//! 一行一条 JSON 消息，请求与响应靠 `id` 配对：
//!
//! ```text
//! → {"id":1,"cmd":"app_info","args":{}}
//! ← {"id":1,"ok":true,"result":{"version":"0.1.0",...}}
//! ← {"id":1,"ok":false,"error":"path must not be empty"}
//! ```
//!
//! ## 为什么是 stdio + JSON
//!
//! - **没有端口**：不占 TCP 端口、不触发防火墙、不需要鉴权，也不会和用户
//!   机器上别的进程抢端口。
//! - **生命周期天然对齐**：stdin 关闭 = 宿主退出，引擎跟着退出，不会留孤儿进程。
//! - **可调试**：管道里是纯文本，出问题时可以直接看到是哪一条命令坏了。
//!
//! 二进制字段（频谱 / 包络 / 声谱图）在 JSON 里是 base64 字符串（`src/b64.rs`），
//! 体积只增加 33%，前端一次解码就还原成 `Uint8Array`。**不**用 JSON 数字
//! 数组（ADR-018 禁止），也暂时**不**上 MessagePack：以当前的快照尺寸
//! （按画布宽度降采样后约 1k bin）base64 的开销可以忽略。若将来声谱图
//! 传输成为瓶颈，替换点只在这个文件的编解码边界上。
//!
//! ## 并发模型
//!
//! 三条泳道：
//!
//! - **快命令**（`app_info`、`cancel_task`、`capture_status_cmd`、
//!   `realtime_snapshot`、`task_queue`）在读取线程里原地执行。它们只做加锁取值，是微秒级；
//!   而且 `realtime_snapshot` 每 20–30 Hz 被轮询一次，**绝不能**排在某次
//!   耗时数秒的文件分析后面。
//! - **控制命令**（`input_devices`、`start_realtime`、`stop_realtime`）走一条
//!   独立的单 worker 泳道。它们与快命令一样必须是「立刻响应」的：
//!   设备枚举和采集开关如果排在文件分析后面，用户点「停止采集」要等
//!   一次几分钟的分析结束才生效，而期间麦克风一直在采集、缓冲一直在丢帧。
//! - **慢命令**（分析、导出、探测）交给固定大小的线程池，队列有界。
//!   队列满时立刻回 `engine busy`，而不是无限堆积或阻塞读取线程。
//!
//! 这样实时视图的刷新频率、采集开关的响应速度和后台分析的耗时彼此独立。

use std::io::{self, BufRead, BufWriter, Stdout, Write};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::Engine;
use crate::{commands, file_commands, realtime_commands};

/// 慢命令的并发上限。
///
/// 2 个就够：桌面上同时最多一个文件分析 + 一个导出。
const SLOW_WORKERS: usize = 2;

/// 慢命令的排队上限。超过即拒绝，避免内存无界增长。
const SLOW_QUEUE: usize = 8;

/// 控制命令的排队上限。
///
/// 采集开关是用户随时可能点的按钮，排队深度大于 1 没有意义：
/// 连按两次「开始/停止」时，第二次要么排队要么被拒，两种都比
/// 「立刻回一个 busy」更容易让人困惑。
const CONTROL_QUEUE: usize = 2;

/// 一条请求。
#[derive(Debug, Deserialize)]
struct Request {
    /// 调用方分配的自增编号；响应原样带回。
    id: u64,
    /// 命令名，与迁移前的 Tauri 命令名一致。
    cmd: String,
    /// 参数对象；无参数命令可省略。
    #[serde(default)]
    args: Value,
}

/// 启动 RPC 循环，直到 stdin 关闭。
///
/// 本函数不返回：stdin EOF 表示宿主进程已退出。
pub fn serve(engine: Engine) -> ! {
    let engine = Arc::new(engine);
    let stdout: Arc<Mutex<BufWriter<Stdout>>> = Arc::new(Mutex::new(BufWriter::new(io::stdout())));

    // ---- 慢命令泳道 ----
    let (tx, rx) = mpsc::sync_channel::<(u64, String, Value)>(SLOW_QUEUE);
    let rx = Arc::new(Mutex::new(rx));
    let mut workers = Vec::with_capacity(SLOW_WORKERS);

    for index in 0..SLOW_WORKERS {
        let engine = Arc::clone(&engine);
        let stdout = Arc::clone(&stdout);
        let rx = Arc::clone(&rx);

        let handle = thread::Builder::new()
            .name(format!("rpc-slow-{index}"))
            .spawn(move || {
                loop {
                    // 只在取任务时持锁；执行期间不持锁，两个 worker 才能真正并行。
                    let job = {
                        let guard = match rx.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.recv()
                    };

                    let Ok((id, cmd, args)) = job else {
                        return; // 通道关闭
                    };

                    let body = respond(&engine, id, &cmd, &args);
                    write_line(&stdout, &body);
                }
            })
            .expect("failed to spawn rpc worker");
        workers.push(handle);
    }

    // ---- 控制泳道（单 worker）：设备枚举与采集开关 ----
    //
    // 独立于慢命令泳道，采集开关就不会被一次几分钟的文件分析堵在后面。
    // 单 worker 让 start/stop 天然串行，不会出现「刚启动就被停掉」的交错。
    let (control_tx, control_rx) = mpsc::sync_channel::<(u64, String, Value)>(CONTROL_QUEUE);
    let control_worker = {
        let engine = Arc::clone(&engine);
        let stdout = Arc::clone(&stdout);
        thread::Builder::new()
            .name("rpc-control".to_string())
            .spawn(move || {
                while let Ok((id, cmd, args)) = control_rx.recv() {
                    let body = respond(&engine, id, &cmd, &args);
                    write_line(&stdout, &body);
                }
            })
            .expect("failed to spawn rpc control worker")
    };

    // ---- 读取循环（快命令泳道） ----
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(error) => {
                write_line(
                    &stdout,
                    &json!({ "id": 0, "ok": false, "error": format!("bad request: {error}") }),
                );
                continue;
            }
        };

        if is_fast(&request.cmd) {
            let body = respond(&engine, request.id, &request.cmd, &request.args);
            write_line(&stdout, &body);
            continue;
        }

        if is_control(&request.cmd) {
            match control_tx.try_send((request.id, request.cmd, request.args)) {
                Ok(()) => {}
                Err(TrySendError::Full((id, ..))) => write_line(
                    &stdout,
                    &json!({ "id": id, "ok": false, "error": "engine busy: control queue is full" }),
                ),
                Err(TrySendError::Disconnected(_)) => break,
            }
            continue;
        }

        match tx.try_send((request.id, request.cmd, request.args)) {
            Ok(()) => {}
            Err(TrySendError::Full((id, ..))) => write_line(
                &stdout,
                &json!({ "id": id, "ok": false, "error": "engine busy: command queue is full" }),
            ),
            Err(TrySendError::Disconnected(_)) => break,
        }
    }

    // stdin 关闭 = 宿主要求退出。先断开队列，让 worker 把**已接受**的任务
    // 执行完并写回响应，再退出；否则最后一条 analyze_file 的响应会被丢掉。
    drop(tx);
    drop(control_tx);
    for worker in workers {
        let _ = worker.join();
    }
    let _ = control_worker.join();

    tracing::info!("stdin closed, engine exiting");
    std::process::exit(0);
}

/// 是否需要留在读取线程里立即执行。
///
/// 判据：被前端高频轮询，或必须在别的命令运行期间仍然生效。
fn is_fast(cmd: &str) -> bool {
    matches!(
        cmd,
        "app_info" | "cancel_task" | "capture_status_cmd" | "realtime_snapshot" | "task_queue"
    )
}

/// 是否属于「控制」命令。
///
/// 这些命令与用户的手指直接相关：设备列表要能立刻刷出来，
/// 「停止采集」不能等一次文件分析跑完。
fn is_control(cmd: &str) -> bool {
    matches!(cmd, "input_devices" | "start_realtime" | "stop_realtime")
}

/// 执行一条命令并生成响应体。
fn respond(engine: &Engine, id: u64, cmd: &str, args: &Value) -> Value {
    match guard(cmd, || dispatch(engine, cmd, args)) {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    }
}

/// 把一次可能 panic 的调用变成一条错误。
///
/// **panic 必须变成 `Err`，而不是让工作线程死掉**：线程一旦因 panic 退出，
/// 那条请求既不 resolve 也不 reject，前端会一直转圈，而线程池还少了一个
/// worker（少掉的过程没有任何日志）。引擎侧的 `isolate_task` 只覆盖了
/// 文件分析相关命令，设备枚举与采集开关走的是另一条路径，所以这一层
/// 必须有兜底。
fn guard<T>(cmd: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let detail = panic_detail(&payload);
            tracing::error!(cmd = %cmd, panic = %detail, "command panicked");
            Err(format!("command `{cmd}` panicked: {detail}"))
        }
    }
}

/// 从 panic payload 里取出可读文本。
fn panic_detail(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// 写一行响应并立即刷新。
///
/// 多个工作线程共享同一个 stdout，所以持锁写入，避免两行 JSON 交错。
fn write_line(stdout: &Arc<Mutex<BufWriter<Stdout>>>, body: &Value) {
    let Ok(text) = serde_json::to_string(body) else {
        tracing::error!("failed to serialize response");
        return;
    };

    let mut guard = match stdout.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    if guard.write_all(text.as_bytes()).is_ok() && guard.write_all(b"\n").is_ok() {
        let _ = guard.flush();
    }
}

/// 执行一条命令。
fn dispatch(engine: &Engine, cmd: &str, args: &Value) -> Result<Value, String> {
    match cmd {
        "app_info" => encode(commands::app_info()),

        "task_queue" => encode(engine.state.tasks()),

        "analyze_synthetic_signal" => encode(commands::analyze_synthetic_signal(
            arg(args, "frequencyHz")?,
            arg(args, "amplitude")?,
            arg(args, "sampleRate")?,
            arg(args, "fftSize")?,
            arg(args, "targetBins")?,
            arg(args, "envelopePoints")?,
        )?),

        "probe_file" => encode(file_commands::probe_file(arg(args, "path")?)?),

        "analyze_file" => encode(file_commands::analyze_file(
            &engine.state,
            arg(args, "path")?,
            opt_arg(args, "envelopePoints")?,
            opt_arg(args, "taskId")?,
        )?),

        "cancel_task" => encode(file_commands::cancel_task(
            &engine.state,
            arg(args, "taskId")?,
        )?),

        "spectrogram_view" => encode(file_commands::spectrogram_view(
            &engine.state,
            arg(args, "path")?,
            arg(args, "startSeconds")?,
            opt_arg(args, "windowSeconds")?,
            opt_arg(args, "taskId")?,
        )?),

        "export_analysis" => encode(file_commands::export_analysis(
            arg(args, "source")?,
            arg(args, "destination")?,
            arg(args, "format")?,
            opt_arg(args, "envelopePoints")?,
        )?),

        "input_devices" => encode(realtime_commands::input_devices()?),

        "start_realtime" => encode(realtime_commands::start_realtime(
            &engine.realtime,
            arg(args, "deviceId")?,
            opt_arg(args, "sampleRate")?,
            opt_arg(args, "channels")?,
        )?),

        "stop_realtime" => encode(realtime_commands::stop_realtime(&engine.realtime)?),

        "capture_status_cmd" => encode(realtime_commands::capture_status_cmd(&engine.realtime)?),

        "realtime_snapshot" => encode(realtime_commands::realtime_snapshot(&engine.realtime)?),

        other => Err(format!("unknown command `{other}`")),
    }
}

/// 序列化一个成功结果。
fn encode<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("serialize failed: {error}"))
}

/// 取必填参数。
fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    let value = args
        .get(key)
        .ok_or_else(|| format!("missing argument `{key}`"))?;
    serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid argument `{key}`: {error}"))
}

/// 取可选参数；缺失与 `null` 都视为 `None`。
fn opt_arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("invalid argument `{key}`: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 采集开关必须在控制泳道，不能落到慢通道。
    ///
    /// 回归测试：早先 `start_realtime` / `stop_realtime` 与文件分析共用
    /// 慢通道，两个分析同时在跑时，用户点「停止采集」要等几分钟才生效，
    /// 期间麦克风一直在采集、环形缓冲一直在丢帧。
    #[test]
    fn capture_switch_is_on_the_control_lane() {
        for cmd in ["start_realtime", "stop_realtime", "input_devices"] {
            assert!(is_control(cmd), "{cmd} 必须在控制泳道");
            assert!(!is_fast(cmd), "{cmd} 不该占用读取线程");
        }
    }

    /// 泳道必须互斥：一个命令只能属于一条泳道，否则优先级是随机的。
    #[test]
    fn lanes_are_disjoint() {
        let commands = [
            "app_info",
            "cancel_task",
            "capture_status_cmd",
            "realtime_snapshot",
            "task_queue",
            "input_devices",
            "start_realtime",
            "stop_realtime",
            "analyze_file",
            "spectrogram_view",
            "export_analysis",
            "probe_file",
        ];
        for cmd in commands {
            let lanes = [is_fast(cmd), is_control(cmd)]
                .iter()
                .filter(|x| **x)
                .count();
            assert!(lanes <= 1, "{cmd} 同时属于多条泳道");
        }
    }

    /// 快照轮询必须留在读取线程：它每 20–30 Hz 被调一次，
    /// 任何排队都会让实时频谱掉帧。
    #[test]
    fn snapshot_polling_stays_inline() {
        assert!(is_fast("realtime_snapshot"));
        assert!(is_fast("capture_status_cmd"));
        assert!(!is_control("realtime_snapshot"));
    }

    /// panic 必须变成一条 `Err`，而不是让工作线程死掉。
    ///
    /// 回归测试：早先 `dispatch` 没有兜底，设备枚举或采集开关 panic 时
    /// worker 线程静默退出，那条请求永远等不到回应（前端永久转圈），
    /// 而且线程池从 2 降到 1，没有任何日志。
    #[test]
    fn a_panicking_command_becomes_an_error_not_a_dead_worker() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = guard("stop_realtime", || -> Result<(), String> {
            panic!("simulated backend panic")
        });
        std::panic::set_hook(prev);

        let error = outcome.expect_err("panic 必须变成 Err");
        assert!(
            error.contains("panicked") && error.contains("simulated backend panic"),
            "错误文本要带上原因：{error}"
        );
    }

    /// 正常错误原样透传，不被 panic 兜底改写。
    #[test]
    fn ordinary_errors_pass_through_unchanged() {
        let outcome: Result<(), String> =
            guard("probe_file", || Err("missing argument `path`".to_string()));
        assert_eq!(outcome.unwrap_err(), "missing argument `path`");
    }
}
