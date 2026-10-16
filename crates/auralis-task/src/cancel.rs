//! 取消令牌。
//!
//! 刻意用 `AtomicBool` 而不是通道或互斥锁：
//! - 工作循环每块检查一次，必须**无锁、无分配**
//! - 实时线程将来也可能需要读它（ADR-004 有界缓冲的关闭流程）

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 取消令牌的共享句柄。
///
/// 克隆成本仅一次原子递增，可自由传递给工作线程。
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// 新建一个未取消的令牌。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消。可重复调用，无副作用。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// 是否已被请求取消。
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// 若已取消则提前返回。
    ///
    /// 用在每块处理之后，让取消在**下一块**生效而不是等整个文件跑完。
    pub fn bail_if_cancelled(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// 「任务已被取消」信号。
///
/// 单独成一个类型（而不是 `()`），是为了让 `?` 能在
/// 同时返回 `Result<T, CodecError>` 的函数里表达取消语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("task cancelled")]
pub struct Cancelled;

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn starts_uncancelled() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        assert!(t.bail_if_cancelled().is_ok());
    }

    #[test]
    fn cancel_is_visible_across_threads() {
        // 用 channel 握手而不是 sleep + yield_now：
        // yield_now 不让出时间片，1000 次循环会在微秒内跑完，
        // 主线程的 sleep 还没结束——那样的测试是必然 flaky 的。
        let t = CancelToken::new();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let t2 = t.clone();
        let worker = thread::spawn(move || {
            started_tx.send(()).expect("signal start");
            let mut checks = 0usize;
            // 上限远大于实际需要：若取消生效，这个上限不应被触及
            const MAX: usize = 1_000_000;
            while checks < MAX {
                if t2.is_cancelled() {
                    break;
                }
                checks += 1;
                // 让出时间片，确保主线程有机会跑到 cancel
                thread::yield_now();
            }
            done_tx.send(checks).expect("report checks");
        });

        // 等工作线程真的开始跑，再取消 —— 无竞态
        started_rx.recv().expect("worker started");
        t.cancel();
        worker.join().expect("worker should not panic");
        let checks = done_rx.recv().expect("worker reported");

        assert!(t.is_cancelled());
        assert!(
            checks < 1_000_000,
            "取消后应提前退出循环（实得 {checks} 次），而不是跑满上限"
        );
    }

    #[test]
    fn cancel_is_idempotent() {
        let t = CancelToken::new();
        t.cancel();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn bail_returns_distinct_error() {
        let t = CancelToken::new();
        t.cancel();
        let e = t.bail_if_cancelled().unwrap_err();
        // 必须是 Cancelled 而不是别的，这样调用方能区分「取消」与「失败」
        assert_eq!(e, Cancelled);
        assert_eq!(e.to_string(), "task cancelled");
    }
}
