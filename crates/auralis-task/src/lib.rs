//! # auralis-task
//!
//! 离线分析任务的调度、进度与取消。
//!
//! ## 设计要点
//!
//! 1. **取消必须是真的**。`AGENTS.md` 要求「取消任务后停止后续处理并释放资源」。
//!    取消令牌是 `Arc<AtomicBool>`，工作循环每处理一块就检查一次；
//!    取消后**不再提交新块**，并把结果标记为 `Cancelled` 而**不是** `Failed`
//!    ——这两者在 UI 上必须能区分，否则用户无法判断该重试还是该放弃。
//!
//! 2. **进度事件可丢弃**。进度是 UI 提示，不是数据。用
//!    `try_send`，队列满就丢弃旧进度而不是阻塞分析线程。
//!
//! 3. **任务隔离**。ADR-009 要求保留 unwinding（不设 `panic = "abort"`），
//!    因此工作线程用 `catch_unwind` 捕获 panic，
//!    让单个坏文件不会终止整个应用。

#![forbid(unsafe_code)]

pub mod cancel;
pub mod event;
pub mod queue;

pub use cancel::{CancelToken, Cancelled};
pub use event::{TaskEvent, TaskId, TaskOutcome, TaskProgress, TaskStatus};
pub use queue::{TaskEngine, TaskRequest};
