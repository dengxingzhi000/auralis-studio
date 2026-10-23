//! 任务引擎。
//!
//! M1 只做**离线分析**的单线程串行队列。理由：离线分析之间没有实时性要求，
//! 串行能避免多个大文件同时占用内存；M4 的实时采集走完全独立的路径
//! （ADR-004 的有界缓冲），不共用这套调度。
//!
//! 事件通道是**有界**的（默认 256），且进度事件用 `try_send`：
//! UI 没跟上时丢弃旧进度，**绝不阻塞分析线程**。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::cancel::CancelToken;
use crate::event::{TaskEvent, TaskId, TaskOutcome, TaskProgress, TaskStatus};

/// 任务完成后的返回值。
pub type TaskResult = Result<Arc<TaskOutput>, String>;

/// 任务产出的结果。
///
/// M1 只需要让 UI 拿到「文件元数据 + 指标」。
/// 波形包络与频谱在 M2/M3 接入，此处保持结构不变以便扩展。
#[derive(Debug, Clone)]
pub struct TaskOutput {
    /// 文件路径。
    pub path: String,
    /// 采样率。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u32,
    /// 处理的总帧数。
    pub frames_processed: u64,
    /// 峰值 dBFS；`None` 表示静音（真 `-inf`）。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS；`None` 表示静音。
    pub rms_dbfs: Option<f32>,
    /// 直流偏移，线性。
    pub dc: f32,
    /// 波峰因数 dB。
    pub crest_db: Option<f32>,
    /// 超量程样本数。
    pub over_full_scale_samples: usize,
    /// 削波片段数。
    pub clipping_regions: u32,
    /// 处理耗时，毫秒。
    pub elapsed_ms: u64,
}

/// 任务体签名：`(取消令牌, 进度回调) -> 结果`。
///
/// 单独起别名，让 `TaskRequest` 的字段类型可读。
pub type TaskBody = dyn Fn(&CancelToken, &dyn Fn(TaskProgress)) -> TaskResult + Send + Sync;

/// 一个待执行的任务。
///
/// 刻意**不**派生 `Debug`：它持有一个 trait object 闭包，无法 Debug。
pub struct TaskRequest {
    /// 文件路径。
    pub path: String,
    /// 样本处理回调：由调用方注入（解码 + 分析）。
    ///
    /// 引擎**不**关心具体做什么，只负责调度、进度与取消。
    pub run: Arc<TaskBody>,
}

impl TaskRequest {
    /// 构造任务请求。
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        run: impl Fn(&CancelToken, &dyn Fn(TaskProgress)) -> TaskResult + Send + Sync + 'static,
    ) -> Self {
        Self {
            path: path.into(),
            run: Arc::new(run),
        }
    }
}

/// 进度事件的最短间隔，毫秒。
///
/// 见 [`TaskEngine::emit_progress`]：节流之后，通道里装的最多是
/// 「每 100 ms 一条」，终态事件几乎总有位置。
const PROGRESS_THROTTLE_MS: u64 = 100;

/// 毫秒时间戳。节流只需要相对间隔，墙钟够用。
fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 任务引擎。
pub struct TaskEngine {
    next_id: std::sync::atomic::AtomicU64,
    cancel: std::sync::Mutex<std::collections::HashMap<TaskId, CancelToken>>,
    sender: Sender<TaskEvent>,
    receiver: Receiver<TaskEvent>,
    finished: AtomicBool,
    /// 上一次发出进度事件的时刻（毫秒时间戳）。节流用。
    last_progress: std::sync::atomic::AtomicU64,
}

impl TaskEngine {
    /// 创建引擎。`channel_capacity` 为 0 时用默认 256。
    #[must_use]
    pub fn new(channel_capacity: usize) -> Self {
        let cap = if channel_capacity == 0 {
            256
        } else {
            channel_capacity
        };
        let (sender, receiver) = bounded(cap);
        Self {
            next_id: std::sync::atomic::AtomicU64::new(1),
            cancel: std::sync::Mutex::new(std::collections::HashMap::new()),
            sender,
            receiver,
            finished: AtomicBool::new(false),
            last_progress: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 事件接收端。UI 侧用它拉取状态更新。
    #[must_use]
    pub const fn events(&self) -> &Receiver<TaskEvent> {
        &self.receiver
    }

    /// 提交一个任务，**同步执行**（M1 不做后台并发）。
    ///
    /// 返回任务的终态。事件同时被推送到通道。
    ///
    /// # Panics
    /// 若引擎已被 `shutdown`。
    pub fn submit(&self, request: TaskRequest) -> TaskOutcome {
        assert!(
            !self.finished.load(Ordering::Acquire),
            "engine is shut down"
        );

        let id = TaskId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let token = CancelToken::new();
        self.register(id, token.clone());

        self.emit(TaskEvent::Queued {
            id,
            path: request.path.clone(),
        });

        self.execute(id, &request, &token)
    }

    /// 取消指定任务。
    ///
    /// 若任务已完成或 id 不存在，返回 `false`。
    pub fn cancel(&self, id: TaskId) -> bool {
        if let Ok(map) = self.cancel.lock()
            && let Some(t) = map.get(&id)
        {
            t.cancel();
            return true;
        }
        false
    }

    /// 取消全部未完成任务。
    pub fn cancel_all(&self) {
        if let Ok(map) = self.cancel.lock() {
            for t in map.values() {
                t.cancel();
            }
        }
    }

    /// 关闭引擎。之后不能再提交任务。
    pub fn shutdown(&self) {
        self.cancel_all();
        self.finished.store(true, Ordering::Release);
    }

    /// 执行单个任务，负责 panic 隔离与事件发射。
    fn execute(&self, id: TaskId, request: &TaskRequest, token: &CancelToken) -> TaskOutcome {
        let started = std::time::Instant::now();

        // catch_unwind 是 ADR-009 要求的：单个坏文件不能带崩整个应用。
        // 注意 run() 捕获的是闭包，所以只能捕获它自己的 panic；
        // 闭包内部应当把可预期的错误转成 Err，不要依赖这里兜底。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (request.run)(token, &|progress| self.emit_progress(id, progress))
        }));

        let (outcome, message) = match result {
            Ok(Ok(_)) => (TaskOutcome::Succeeded, None),
            // 取消检查必须排在通用 Err 之前，否则被它遮蔽，
            // 取消的任务会被误报为 Failed（UI 就会显示「重试」按钮）。
            Ok(Err(msg)) if token.is_cancelled() => (TaskOutcome::Cancelled, Some(msg)),
            Ok(Err(msg)) => (TaskOutcome::Failed, Some(msg)),
            Err(_) => (
                TaskOutcome::Panicked,
                Some("worker panicked; see logs".to_string()),
            ),
        };

        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(
            task = %id,
            path = %request.path,
            ?outcome,
            elapsed_ms,
            "task finished"
        );

        self.emit_terminal(TaskEvent::Finished {
            id,
            outcome,
            message,
        });
        self.unregister(id);
        let _ = elapsed_ms;
        outcome
    }

    /// 推送一个事件。
    ///
    /// 用 `try_send`：通道满时**丢弃**而不是阻塞。
    /// 丢的只会是进度提示 —— 终态事件走 [`Self::emit_terminal`]。
    fn emit(&self, event: TaskEvent) {
        if self.sender.try_send(event).is_err() {
            tracing::debug!("event channel full; dropping update");
        }
    }

    /// 推送一个**终态**事件（任务的结束状态）。
    ///
    /// 终态必须送达：它是「这次任务结束了」的唯一信号，丢了就再也补不回来
    /// —— 进度事件可以丢（UI 只是少一帧进度），终态不行。
    ///
    /// 这里最多等 100 ms：通道满时消费者很快就会腾出位置，而进度事件已经
    /// 节流，通道本来也不会满到哪儿去。等的是一个有界的时刻，不是无限期。
    fn emit_terminal(&self, event: TaskEvent) {
        if self
            .sender
            .send_timeout(event, std::time::Duration::from_millis(100))
            .is_err()
        {
            tracing::warn!("terminal task event could not be delivered");
        }
    }

    /// 进度事件节流：两次之间至少隔 [`PROGRESS_THROTTLE_MS`]。
    ///
    /// 分析是按解码块推进的，一个 3 小时的音频能产生上万次进度通知。
    /// 不节流的话，256 容量的通道会被灌满 —— UI 拿到的全是过期进度，
    /// 别的事件（尤其是终态）反而挤不进去。节流的是**频率**，不是
    /// 「丢不丢」。
    fn emit_progress(&self, id: TaskId, progress: TaskProgress) {
        let now = unix_millis();
        let last = self.last_progress.load(Ordering::Relaxed);
        // 用 wrapping_sub：墙钟被 NTP 回拨时差值会变成一个巨大的正数，
        // 于是「时间过去了」成立，不会把进度节流到时钟追上来为止。
        if now.wrapping_sub(last) < PROGRESS_THROTTLE_MS {
            return;
        }
        // 用 CAS 抢「本轮」：抢不到说明同一时刻有别的线程正在发。
        if self
            .last_progress
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        self.emit(TaskEvent::Progress { id, progress });
    }

    fn register(&self, id: TaskId, token: CancelToken) {
        if let Ok(mut map) = self.cancel.lock() {
            map.insert(id, token);
        }
    }

    fn unregister(&self, id: TaskId) {
        if let Ok(mut map) = self.cancel.lock() {
            map.remove(&id);
        }
    }
}

impl std::fmt::Debug for TaskEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskEngine")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field("shutdown", &self.finished.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

/// 当前任务状态快照（供 UI 查询）。
pub fn status_of(events: &Receiver<TaskEvent>) -> Vec<(TaskId, TaskStatus)> {
    events.try_iter().fold(Vec::new(), |mut acc, e| {
        let id = e.id();
        let status = match e {
            TaskEvent::Queued { .. } => TaskStatus::Queued,
            TaskEvent::Started { .. } | TaskEvent::Progress { .. } => TaskStatus::Running,
            TaskEvent::Finished { outcome, .. } => TaskStatus::Finished(outcome),
        };
        match acc.iter_mut().find(|(tid, _)| *tid == id) {
            Some(slot) => slot.1 = status,
            None => acc.push((id, status)),
        }
        acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn engine() -> TaskEngine {
        TaskEngine::new(64)
    }

    fn ok_task(path: &str) -> TaskRequest {
        let p = path.to_string();
        TaskRequest::new(path, move |_token, _progress| {
            Ok(Arc::new(TaskOutput {
                path: p.clone(),
                sample_rate: 48_000,
                channels: 2,
                frames_processed: 1000,
                peak_dbfs: Some(-6.0),
                rms_dbfs: Some(-9.0),
                dc: 0.0,
                crest_db: Some(3.0),
                over_full_scale_samples: 0,
                clipping_regions: 0,
                elapsed_ms: 1,
            }))
        })
    }

    #[test]
    fn successful_task_reports_succeeded() {
        let e = engine();
        let outcome = e.submit(ok_task("a.wav"));
        assert_eq!(outcome, TaskOutcome::Succeeded);
    }

    #[test]
    fn failed_task_reports_failed_with_message() {
        let e = engine();
        let r = TaskRequest::new("bad.wav", |_t, _p| Err("codec exploded".to_string()));
        assert_eq!(e.submit(r), TaskOutcome::Failed);
    }

    #[test]
    fn pre_cancelled_token_yields_cancelled_not_failed() {
        // 这是 M1 的关键语义：取消 != 失败
        let e = engine();
        let r = TaskRequest::new("x.wav", |token, _p| {
            // 模拟「刚开始就被取消」
            token.cancel();
            Err("stopped".to_string())
        });
        let outcome = e.submit(r);
        assert_eq!(
            outcome,
            TaskOutcome::Cancelled,
            "已取消的任务必须报告 Cancelled 而非 Failed"
        );
    }

    #[test]
    fn panicking_task_does_not_propagate() {
        // ADR-009：单个任务 panic 不能终止应用
        let e = engine();
        let r = TaskRequest::new("boom.wav", |_t, _p| panic!("deliberate panic in worker"));
        // set a panic hook so the test output is not polluted
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = e.submit(r);
        std::panic::set_hook(prev);
        assert_eq!(outcome, TaskOutcome::Panicked);
    }

    #[test]
    fn engine_survives_a_panic_and_accepts_next_task() {
        let e = engine();
        let bad = TaskRequest::new("boom.wav", |_t, _p| panic!("boom"));
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = e.submit(bad);
        std::panic::set_hook(prev);

        // 引擎必须还能继续工作
        assert_eq!(e.submit(ok_task("good.wav")), TaskOutcome::Succeeded);
    }

    #[test]
    fn cancellation_stops_work_early() {
        let e = engine();
        let ran = Arc::new(AtomicUsize::new(0));

        let r2 = ran.clone();
        let req = TaskRequest::new("long.wav", move |token, progress| {
            for i in 0..1000u64 {
                // 每块检查一次取消
                if token.bail_if_cancelled().is_err() {
                    return Err("cancelled".to_string());
                }
                r2.fetch_add(1, Ordering::SeqCst);
                progress(TaskProgress {
                    frames_done: i,
                    frames_total: Some(1000),
                    bytes_done: i * 100,
                });
                if i == 5 {
                    token.cancel();
                }
            }
            Ok(Arc::new(TaskOutput {
                path: "long.wav".into(),
                sample_rate: 48_000,
                channels: 1,
                frames_processed: 1000,
                peak_dbfs: None,
                rms_dbfs: None,
                dc: 0.0,
                crest_db: None,
                over_full_scale_samples: 0,
                clipping_regions: 0,
                elapsed_ms: 1,
            }))
        });

        assert_eq!(e.submit(req), TaskOutcome::Cancelled);
        assert!(
            ran.load(Ordering::SeqCst) < 1000,
            "取消后应停止处理，实得 {} 块",
            ran.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn events_are_emitted_in_order() {
        let e = engine();
        e.submit(ok_task("a.wav"));

        let events: Vec<TaskEvent> = e.events().try_iter().collect();
        assert!(!events.is_empty());
        assert!(matches!(events.first(), Some(TaskEvent::Queued { .. })));
        assert!(events.last().expect("has events").is_terminal());
    }

    #[test]
    fn progress_events_do_not_block_when_channel_full() {
        // 事件通道满时必须丢弃而不是阻塞分析线程
        let e = TaskEngine::new(1); // 极小容量
        let req = TaskRequest::new("chatty.wav", |_t, progress| {
            for i in 0..1000u64 {
                progress(TaskProgress {
                    frames_done: i,
                    frames_total: Some(1000),
                    bytes_done: i,
                });
            }
            Ok(Arc::new(TaskOutput {
                path: "chatty.wav".into(),
                sample_rate: 48_000,
                channels: 1,
                frames_processed: 1000,
                peak_dbfs: None,
                rms_dbfs: None,
                dc: 0.0,
                crest_db: None,
                over_full_scale_samples: 0,
                clipping_regions: 0,
                elapsed_ms: 1,
            }))
        });
        // 若 try_send 会阻塞，这里会挂死
        let outcome = e.submit(req);
        assert_eq!(outcome, TaskOutcome::Succeeded);
    }

    #[test]
    fn cancel_unknown_task_returns_false() {
        let e = engine();
        assert!(!e.cancel(TaskId(9999)));
    }

    /// 回归测试：进度事件必须节流。
    ///
    /// 早先的注释写着「进度事件节流」，但代码里**根本没有节流** ——
    /// 分析按解码块推进，一个长文件能产生上万次进度通知，把 256 容量的
    /// 通道灌满，别的事件（尤其是终态）反而挤不进去。
    #[test]
    fn progress_events_are_throttled() {
        let e = engine();
        let id = TaskId(1);
        for i in 0..1000u64 {
            e.emit_progress(
                id,
                TaskProgress {
                    frames_done: i,
                    frames_total: Some(1000),
                    bytes_done: i,
                },
            );
        }
        let events: Vec<TaskEvent> = e.events().try_iter().collect();
        let progress = events
            .iter()
            .filter(|ev| matches!(ev, TaskEvent::Progress { .. }))
            .count();
        assert!(
            progress <= 2,
            "1000 次进度通知应被节流到个位数，实得 {progress}"
        );
    }

    /// 回归测试：终态事件必须送达。
    ///
    /// 进度丢了无所谓（UI 少一帧进度），终态丢了就再也补不回来 ——
    /// 那是「这次分析结束了」的唯一信号。
    #[test]
    fn the_terminal_event_is_delivered() {
        let e = engine();
        let r = TaskRequest::new("x.wav", |_t, _p| {
            Ok(Arc::new(TaskOutput {
                path: "x.wav".into(),
                sample_rate: 48_000,
                channels: 2,
                frames_processed: 1000,
                peak_dbfs: Some(-6.0),
                rms_dbfs: Some(-9.0),
                dc: 0.0,
                crest_db: Some(3.0),
                over_full_scale_samples: 0,
                clipping_regions: 0,
                elapsed_ms: 1,
            }))
        });
        e.submit(r);
        let events: Vec<TaskEvent> = e.events().try_iter().collect();
        assert!(
            events.iter().any(|ev| matches!(
                ev,
                TaskEvent::Finished {
                    outcome: TaskOutcome::Succeeded,
                    ..
                }
            )),
            "终态事件必须送达，实得 {events:?}"
        );
    }

    #[test]
    fn status_tracks_lifecycle() {
        let e = engine();
        e.submit(ok_task("a.wav"));
        let statuses = status_of(e.events());
        let (_, st) = statuses.first().expect("one task");
        assert!(matches!(st, TaskStatus::Finished(TaskOutcome::Succeeded)));
    }
}
