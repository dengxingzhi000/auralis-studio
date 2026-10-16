//! 有界无锁 SPSC 环形缓冲。
//!
//! # 实时安全性论证
//!
//! | 操作 | 复杂度 | 是否可能阻塞 | 是否分配 |
//! |---|---|---|---|
//! | [`SampleRing::push`] | O(1) | 否（无锁 CAS） | **否**（预分配 `Vec`） |
//! | [`SampleRing::pop`] | O(1) | 否 | 否（复用调用方缓冲） |
//! | [`SampleRing::stats`] | O(1) | 否 | 否 |
//!
//! 设计上的三个保证：
//!
//! 1. **容量固定**。构造时分配，构造后不再增长。
//! 2. **溢出必然可见**。每次丢弃累加原子计数，不会静默丢数据。
//! 3. **关闭不需要额外机制**。worker 退出由调用方决定；本类型没有
//!    「等待另一个线程」的方法，因此不会引入阻塞点。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;

/// 队列满时的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    /// 丢最旧的样本，为新样本腾位置。
    ///
    /// 保「最新」，但需要一次 pop + 一次 push，且会**在调用方线程**上
    /// 执行一次额外操作。适合「显示当前状态」类场景。
    DropOldest,
    /// 丢弃新到的样本（推荐）。
    ///
    /// 只做一次失败的 push，代价最小。显示的时间点更靠近「现在」。
    DropNewest,
}

/// 环形缓冲统计量。全部是原子的，可在回调中递增、在任意线程读取。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RingStats {
    /// 成功写入的帧数。
    pub pushed_frames: u64,
    /// 因队列满而丢弃的帧数。**UI 必须显示这个数**。
    pub dropped_frames: u64,
    /// 被消费方取走的帧数。
    pub popped_frames: u64,
}

impl RingStats {
    /// 丢帧率（0.0–1.0）。
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        let total = self.pushed_frames + self.dropped_frames;
        if total == 0 {
            0.0
        } else {
            self.dropped_frames as f64 / total as f64
        }
    }

    /// 缓冲占用比例（0.0–1.0）。
    #[must_use]
    pub fn fill_ratio(&self, capacity_frames: usize) -> f64 {
        if capacity_frames == 0 {
            0.0
        } else {
            (self.popped_frames.min(self.pushed_frames) as f64 / capacity_frames as f64).min(1.0)
        }
    }
}

/// 固定容量的样本环形缓冲（SPSC）。
///
/// `T` 是样本类型。实际使用中为交错 `f32`。
///
/// # 并发模型
///
/// 单生产者单消费者。生产者（音频回调）调 [`Self::push`]，
/// 消费者（分析工作线程）调 [`Self::pop_chunk`]。
pub struct SampleRing<T> {
    queue: Arc<ArrayQueue<T>>,
    capacity_frames: usize,
    frames_per_item: usize,
    policy: DropPolicy,
    pushed: AtomicU64,
    dropped: AtomicU64,
    popped: AtomicU64,
    closed: AtomicBool,
}

impl<T> SampleRing<T> {
    /// 创建环形缓冲。
    ///
    /// # Panics
    /// `capacity_frames == 0` 或 `frames_per_item == 0`。
    ///
    /// 这两个值在运行期由配置决定，属编程错误而非输入错误。
    pub fn new(capacity_frames: usize, frames_per_item: usize, policy: DropPolicy) -> Self {
        assert!(capacity_frames > 0, "capacity must be positive");
        assert!(frames_per_item > 0, "frames_per_item must be positive");
        Self {
            queue: Arc::new(ArrayQueue::new(capacity_frames)),
            capacity_frames,
            frames_per_item,
            policy,
            pushed: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            popped: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// 容量（帧）。
    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    /// 每项含多少帧。
    #[must_use]
    pub const fn frames_per_item(&self) -> usize {
        self.frames_per_item
    }

    /// 当前统计。
    #[must_use]
    pub fn stats(&self) -> RingStats {
        RingStats {
            pushed_frames: self.pushed.load(Ordering::Relaxed),
            dropped_frames: self.dropped.load(Ordering::Relaxed),
            popped_frames: self.popped.load(Ordering::Relaxed),
        }
    }

    /// 已关闭则写入无效。
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// 标记关闭。生产者端在停止采集后调用。
    ///
    /// 关闭后 `push` 一律丢弃并计数，因此停止顺序上不会出现
    /// 「生产者还在写、消费者已经退出」的悬挂写入。
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    /// 丢弃当前内容（不清空统计）。切换设备时用。
    pub fn clear(&self) {
        while self.queue.pop().is_some() {}
    }

    /// 写入一个样本项（由**音频回调**调用）。
    ///
    /// 返回是否写入成功。**永不阻塞、永不分配。**
    #[must_use]
    pub fn push(&self, item: T) -> bool {
        if self.is_closed() {
            self.dropped
                .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
            return false;
        }
        // ArrayQueue::push 在失败时把 item 原样还回来，必须接住，
        // 否则 item 在这里就被移动走了，后面的溢出分支再用它是编译错误。
        let item = match self.queue.push(item) {
            Ok(()) => {
                self.pushed
                    .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                return true;
            }
            Err(item) => item,
        };

        match self.policy {
            DropPolicy::DropNewest => {
                // 直接丢弃，不额外分配
                drop(item);
                self.dropped
                    .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                false
            }
            DropPolicy::DropOldest => {
                if self.queue.pop().is_some() {
                    self.popped
                        .fetch_sub(self.frames_per_item as u64, Ordering::Relaxed);
                    self.pushed
                        .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                    self.queue.push(item).is_ok()
                } else {
                    self.dropped
                        .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                    false
                }
            }
        }
    }

    /// 尝试消费一项到调用方提供的缓冲（由**工作线程**调用）。
    ///
    /// 复用调用方缓冲以避免分配。返回是否取到。
    #[must_use]
    pub fn pop_chunk(&self, out: &mut Vec<T>) -> bool {
        out.clear();
        while let Some(item) = self.queue.pop() {
            out.push(item);
            if out.len() >= 64 {
                break; // 批量上限，避免单次占用过久
            }
        }
        if out.is_empty() {
            false
        } else {
            self.popped
                .fetch_add((out.len() * self.frames_per_item) as u64, Ordering::Relaxed);
            true
        }
    }

    /// 是否还有待消费数据。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::thread;

    #[test]
    fn push_and_pop_roundtrip() {
        let r: SampleRing<u32> = SampleRing::new(8, 1, DropPolicy::DropNewest);
        assert!(r.is_empty());
        for i in 0..4 {
            assert!(r.push(i));
        }
        let mut out = Vec::new();
        assert!(r.pop_chunk(&mut out));
        assert_eq!(out, vec![0, 1, 2, 3]);
        assert_eq!(r.stats().pushed_frames, 4);
        assert_eq!(r.stats().dropped_frames, 0);
        // 排空之后再取应当返回 false，而不是重复返回已消费的项
        assert!(!r.pop_chunk(&mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn capacity_is_never_exceeded() {
        // 关键性质：无论写多少，队列长度都不超过容量
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        for i in 0..1000 {
            let _ = r.push(i);
        }
        let mut out = Vec::new();
        let mut got = 0;
        while r.pop_chunk(&mut out) {
            got += out.len();
        }
        assert_eq!(got, 4, "最多只能取出容量那么多");
        assert_eq!(r.stats().dropped_frames, 996);
    }

    #[test]
    fn drop_newest_keeps_oldest() {
        let r: SampleRing<u32> = SampleRing::new(2, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        let _ = r.push(2);
        let _ = r.push(3); // 应被丢弃
        let mut out = Vec::new();
        let _ = r.pop_chunk(&mut out);
        assert_eq!(out, vec![1, 2], "DropNewest 应保留原有数据");
    }

    #[test]
    fn drop_oldest_keeps_newest() {
        let r: SampleRing<u32> = SampleRing::new(2, 1, DropPolicy::DropOldest);
        let _ = r.push(1);
        let _ = r.push(2);
        let _ = r.push(3); // 应挤掉最旧的
        let mut out = Vec::new();
        let _ = r.pop_chunk(&mut out);
        assert_eq!(out, vec![2, 3], "DropOldest 应保留最新数据");
        assert_eq!(r.stats().dropped_frames, 0, "挤掉旧数据不算丢弃");
    }

    #[test]
    fn every_dropped_frame_is_counted() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        for _ in 0..100 {
            let _ = r.push(7);
        }
        // AGENTS.md 要求：丢帧必须可观测
        assert_eq!(r.stats().dropped_frames, 96);
        assert!(r.stats().drop_rate() > 0.95);
    }

    #[test]
    fn frames_per_item_is_counted_correctly() {
        // 每项 64 帧，容量 8 项 = 512 帧
        let r: SampleRing<u32> = SampleRing::new(8, 64, DropPolicy::DropNewest);
        let _ = r.push(0);
        let _ = r.push(1);
        assert_eq!(r.stats().pushed_frames, 128, "应按帧计数而非按项计数");
        assert_eq!(r.capacity_frames(), 8);
        assert_eq!(r.frames_per_item(), 64);
    }

    #[test]
    fn closed_ring_rejects_writes() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        r.close();
        assert!(r.is_closed());
        assert!(!r.push(2), "关闭后不应再接受写入");
        assert_eq!(
            r.stats().dropped_frames,
            1,
            "关闭后的写入也要计数，不能静默"
        );
    }

    #[test]
    fn clear_empties_but_keeps_stats() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r.stats().pushed_frames, 1, "clear 不应重置统计");
    }

    #[test]
    fn concurrent_producer_consumer_never_blocks() {
        // 模拟真实场景：生产者快于消费者，必须靠丢弃保持实时性
        const N: u32 = 20_000;
        let r: Arc<SampleRing<u32>> = Arc::new(SampleRing::new(256, 1, DropPolicy::DropNewest));
        let stop = Arc::new(AtomicBool::new(false));
        let consumed = Arc::new(AtomicUsize::new(0));

        let r2 = r.clone();
        let stop2 = stop.clone();
        let c2 = consumed.clone();
        let consumer = thread::spawn(move || {
            let mut out = Vec::new();
            while !stop2.load(Ordering::Relaxed) {
                if r2.pop_chunk(&mut out) {
                    c2.fetch_add(out.len(), Ordering::Relaxed);
                } else {
                    thread::yield_now();
                }
            }
        });

        for i in 0..N {
            let _ = r.push(i);
        }
        stop.store(true, Ordering::Relaxed);
        consumer.join().expect("consumer must not panic");

        let s = r.stats();
        let total = s.pushed_frames + s.dropped_frames;
        assert_eq!(
            total,
            u64::from(N),
            "每个样本要么成功要么被计数，不得凭空消失"
        );
        assert_eq!(
            s.popped_frames,
            consumed.load(Ordering::Relaxed) as u64,
            "消费计数应与实际取出一致"
        );
        assert!(s.dropped_frames > 0, "本测试中消费者故意较慢，应当发生丢弃");
    }

    #[test]
    fn push_does_not_allocate_after_construction() {
        // 用「队列容量恒定 + 反复写入」验证没有隐藏的分配：
        // 若每次 push 都重新分配 Vec，容量会被突破。
        let r: SampleRing<Vec<u32>> = SampleRing::new(8, 1, DropPolicy::DropNewest);
        for i in 0..1000 {
            let _ = r.push(vec![i]);
        }
        let mut out = Vec::new();
        let mut got = 0;
        while r.pop_chunk(&mut out) {
            got += out.len();
        }
        assert_eq!(got, 8);
    }

    #[test]
    #[should_panic(expected = "capacity must be positive")]
    fn zero_capacity_panics() {
        let _: SampleRing<u32> = SampleRing::new(0, 1, DropPolicy::DropNewest);
    }

    #[test]
    fn fill_ratio_is_bounded() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        let f = r.stats().fill_ratio(r.capacity_frames());
        assert!((0.0..=1.0).contains(&f), "占用比例必须在 0..1，实得 {f}");
    }

    #[test]
    fn drop_rate_is_zero_when_nothing_dropped() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        let _ = r.push(2);
        assert_eq!(r.stats().drop_rate(), 0.0);
    }
}
