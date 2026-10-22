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
//! 两条泳道：
//!
//! - **快命令**（`app_info`、`cancel_task`、`capture_status_cmd`、
//!   `realtime_snapshot`）在读取线程里原地执行。它们只做加锁取值，是微秒级；
//!   而且 `realtime_snapshot` 每 20–30 Hz 被轮询一次，**绝不能**排在某次
//!   耗时数秒的文件分析后面。
//! - **慢命令**（分析、导出、探测、设备枚举）交给固定大小的线程池，队列有界。
//!   队列满时立刻回 `engine busy`，而不是无限堆积或阻塞读取线程。
//!
//! 这样实时视图的刷新频率和后台分析的耗时彼此独立。

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

                    let body = match dispatch(&engine, &cmd, &args) {
                        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
                        Err(error) => json!({ "id": id, "ok": false, "error": error }),
                    };
                    write_line(&stdout, &body);
                }
            })
            .expect("failed to spawn rpc worker");
        workers.push(handle);
    }

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
            let body = match dispatch(&engine, &request.cmd, &request.args) {
                Ok(result) => json!({ "id": request.id, "ok": true, "result": result }),
                Err(error) => json!({ "id": request.id, "ok": false, "error": error }),
            };
            write_line(&stdout, &body);
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
    for worker in workers {
        let _ = worker.join();
    }

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
