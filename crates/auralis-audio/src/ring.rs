//! 有界无锁 SPSC 环形缓冲。
//!
//! # 实时安全性论证
//!
//! | 操作 | 复杂度 | 是否可能阻塞 | 是否分配 |
//! |---|---|---|---|
//! | [`SampleRing::push`] | O(1) | 否（无锁 CAS） | **否**（预分配 `Vec`） |
//! | [`SampleRing::pop_chunk`] | O(1) | 否 | 否（复用调用方缓冲） |
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
///
/// # 计数器语义
///
/// | 计数器 | 含义 |
/// |---|---|
/// | [`Self::pushed_frames`] | **进过**环形缓冲的帧数 |
/// | [`Self::dropped_frames`] | 写入时被拒绝、从未进过环的帧数 |
/// | [`Self::popped_frames`] | **被消费方取走**的帧数 |
/// | [`Self::evicted_frames`] | 进过环、但被 `DropOldest` 策略挤掉、没被消费方看到的帧数 |
///
/// 不变式：`pushed == popped + evicted + 环内残留`。
/// 四者单位都是**帧**（不是项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RingStats {
    /// 成功写入的帧数。
    pub pushed_frames: u64,
    /// 因队列满而丢弃的帧数。**UI 必须显示这个数**。
    pub dropped_frames: u64,
    /// 被消费方取走的帧数。
    ///
    /// 只表示「消费方实际取走了多少」。被策略挤掉的数据算在
    /// [`Self::evicted_frames`]，**不**在这里做减法——早先的实现在挤掉
    /// 最旧项时对这里减一，而那一项从没被加过，原子减法会回绕成
    /// `u64::MAX`，整个计数器从此失真。
    pub popped_frames: u64,
    /// 被 `DropOldest` 策略挤出的帧数。
    ///
    /// 这部分数据**进过**环形缓冲却没被消费方看到，属于丢失，必须单独
    /// 计数，否则「溢出必然可见」这条就不成立。
    pub evicted_frames: u64,
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

    /// 当前环内**残留**的帧数。
    ///
    /// `pushed_frames` 记的是进过环的帧，`popped_frames` / `evicted_frames`
    /// 记的是离开环的帧，三者相减才是当前占用。
    #[must_use]
    pub fn occupancy_frames(&self) -> u64 {
        self.pushed_frames
            .saturating_sub(self.popped_frames)
            .saturating_sub(self.evicted_frames)
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
    capacity_items: usize,
    frames_per_item: usize,
    policy: DropPolicy,
    pushed: AtomicU64,
    dropped: AtomicU64,
    popped: AtomicU64,
    evicted: AtomicU64,
    closed: AtomicBool,
}

impl<T> SampleRing<T> {
    /// 创建环形缓冲。
    ///
    /// `capacity_items` 是队列能容纳的**项**数，每项 `frames_per_item` 帧。
    /// 实际能缓存的帧数是两者之积 —— 见 [`Self::capacity_frames`]。
    ///
    /// # Panics
    /// `capacity_items == 0` 或 `frames_per_item == 0`。
    ///
    /// 这两个值在运行期由配置决定，属编程错误而非输入错误。
    pub fn new(capacity_items: usize, frames_per_item: usize, policy: DropPolicy) -> Self {
        assert!(capacity_items > 0, "capacity must be positive");
        assert!(frames_per_item > 0, "frames_per_item must be positive");
        Self {
            queue: Arc::new(ArrayQueue::new(capacity_items)),
            capacity_items,
            frames_per_item,
            policy,
            pushed: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            popped: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// 队列能容纳的**项**数。
    ///
    /// 早先这个参数叫 `capacity_frames`，实际却是项数 —— 一个 4096「帧」的
    /// 环在 256 帧/项时其实能装 1 048 576 帧。名字说帧、实为项，读代码的人
    /// 会把缓冲深度算错一个数量级。
    #[must_use]
    pub const fn capacity_items(&self) -> usize {
        self.capacity_items
    }

    /// 实际能缓存的**帧**数（`项数 × 每项帧数`）。
    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_items * self.frames_per_item
    }

    /// 每项含多少帧。
    #[must_use]
    pub const fn frames_per_item(&self) -> usize {
        self.frames_per_item
    }

    /// 当前环内**残留**的帧数。
    ///
    /// `pushed` 记的是进过环的帧，`popped` / `evicted` 记的是离开环的帧，
    /// 三者相减才是当前占用。
    #[must_use]
    pub fn occupancy_frames(&self) -> u64 {
        self.stats().occupancy_frames()
    }

    /// 缓冲占用比例（0.0–1.0）。
    ///
    /// 早先这个指标挂在 [`RingStats`] 上、要调用方自己传容量，于是两处都
    /// 错了：分子用的是**累计已消费**（消费得越多读数越满），分母传的又
    /// 是**项数**而不是帧数。现在容量由环自己知道，公式也只有一处。
    #[must_use]
    pub fn fill_ratio(&self) -> f64 {
        let capacity = self.capacity_frames();
        if capacity == 0 {
            0.0
        } else {
            (self.occupancy_frames() as f64 / capacity as f64).min(1.0)
        }
    }

    /// 当前统计。
    #[must_use]
    pub fn stats(&self) -> RingStats {
        RingStats {
            pushed_frames: self.pushed.load(Ordering::Relaxed),
            dropped_frames: self.dropped.load(Ordering::Relaxed),
            popped_frames: self.popped.load(Ordering::Relaxed),
            evicted_frames: self.evicted.load(Ordering::Relaxed),
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
    /// 返回 `Err(item)` 表示因队列满（或已关闭）而**未写入**，此时 item 被
    /// 原样退回，调用方可继续复用它 —— 这正是实时路径上避免分配的关键
    /// （见 [`crate::capture`] 的缓冲区回收）。
    ///
    /// 永不阻塞，成功路径永不分配。
    pub fn push(&self, item: T) -> Result<(), T> {
        if self.is_closed() {
            self.dropped
                .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
            return Err(item);
        }
        // ArrayQueue::push 失败时把 item 原样还回来，必须接住，
        // 否则 item 在这里就被移动走了。
        let item = match self.queue.push(item) {
            Ok(()) => {
                self.pushed
                    .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                return Ok(());
            }
            Err(item) => item,
        };

        match self.policy {
            DropPolicy::DropNewest => {
                self.dropped
                    .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                Err(item)
            }
            DropPolicy::DropOldest => {
                if self.queue.pop().is_some() {
                    // 被挤出的那一项：**进过环、但消费方没看到**，属于丢失。
                    // 记在 `evicted` 上，**不动 `popped`**——
                    // `popped` 只表示「消费方实际取走了多少」，
                    // 那一项从没被加过，在这里减一会让原子计数回绕成 u64::MAX。
                    self.evicted
                        .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                    // 腾出一个位置后写入新项；真失败也必须把 item 还回去。
                    match self.queue.push(item) {
                        Ok(()) => {
                            self.pushed
                                .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                            Ok(())
                        }
                        Err(item) => {
                            self.dropped
                                .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                            Err(item)
                        }
                    }
                } else {
                    self.dropped
                        .fetch_add(self.frames_per_item as u64, Ordering::Relaxed);
                    Err(item)
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
            assert!(r.push(i).is_ok());
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
        assert_eq!(r.stats().dropped_frames, 0, "挤掉旧数据不算写入拒绝");
    }

    #[test]
    fn drop_oldest_does_not_corrupt_the_popped_counter() {
        // 回归测试：早先的实现挤掉最旧项时对 `popped` 做减一，
        // 而那一项从没被加过 → 原子减法回绕成 u64::MAX，计数器从此失真。
        let r: SampleRing<u32> = SampleRing::new(2, 1, DropPolicy::DropOldest);
        for i in 0..10 {
            let _ = r.push(i);
        }
        let s = r.stats();
        assert_eq!(s.evicted_frames, 8, "被挤掉的 8 项必须单独计数");
        assert_eq!(s.popped_frames, 0, "消费方还没取过东西，计数必须是 0");
        assert_eq!(s.pushed_frames, 10, "10 次写入都进过环");
        // 不变式：pushed == popped + evicted + 环内残留（此时残留 2）
        assert_eq!(s.pushed_frames, s.popped_frames + s.evicted_frames + 2);
    }

    #[test]
    fn drop_oldest_evicted_frames_stay_counted_after_consumption() {
        let r: SampleRing<u32> = SampleRing::new(2, 1, DropPolicy::DropOldest);
        for i in 0..5 {
            let _ = r.push(i);
        }
        let mut out = Vec::new();
        let _ = r.pop_chunk(&mut out);
        let s = r.stats();
        assert_eq!(s.popped_frames, 2, "消费方取走了 2 项");
        assert_eq!(s.evicted_frames, 3, "3 项被策略挤掉");
        assert!(s.popped_frames < u64::MAX / 2, "计数器不得回绕");
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
        assert_eq!(r.capacity_items(), 8, "容量是项数");
        assert_eq!(r.capacity_frames(), 8 * 64, "帧容量 = 项数 × 每项帧数");
        assert_eq!(r.frames_per_item(), 64);
    }

    /// 回归测试：容量参数的名字说「帧」，实际是「项」。
    ///
    /// 早先 `capacity_frames` 传的是项数，256 帧/项时能装 1 048 576 帧，
    /// 而读代码的人会当成 4096 帧 —— 缓冲深度算错一个数量级。
    #[test]
    fn capacity_units_are_explicit() {
        let r: SampleRing<u32> = SampleRing::new(4096, 256, DropPolicy::DropNewest);
        assert_eq!(r.capacity_items(), 4096);
        assert_eq!(r.capacity_frames(), 4096 * 256);
        assert_eq!(r.frames_per_item(), 256);
    }

    #[test]
    fn closed_ring_rejects_writes() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        r.close();
        assert!(r.is_closed());
        assert!(r.push(2).is_err(), "关闭后不应再接受写入");
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
        // 第一阶段在**没有消费者**时灌满并溢出环形缓冲：丢弃是确定性的，
        // 不依赖调度器谁先跑。第二轮再并发生产/消费，验证不阻塞且计数一致。
        //
        // 早期版本只靠「生产者应该比消费者快」来断言 dropped > 0，
        // 在快的 CI runner（ubuntu-latest）上消费者可能完全跟上，
        // 断言偶发失败（run 39752201678）。测试不能依赖线程调度的假设。
        const N: u32 = 20_000;
        let r: Arc<SampleRing<u32>> = Arc::new(SampleRing::new(256, 1, DropPolicy::DropNewest));

        // 第一阶段：无消费者，容量 256，必然丢弃 N - 256 个。
        for i in 0..N {
            let _ = r.push(i);
        }
        assert!(
            r.stats().dropped_frames > 0,
            "无消费者时灌入超过容量的样本，必然发生丢弃"
        );

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
            u64::from(N) * 2,
            "每个样本要么成功要么被计数，不得凭空消失"
        );
        assert_eq!(
            s.popped_frames,
            consumed.load(Ordering::Relaxed) as u64,
            "消费计数应与实际取出一致"
        );
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

    /// 回归测试：占用比例必须反映**当前占用**，不是累计已消费。
    ///
    /// 早先的公式是 `popped / capacity` —— 消费得越多读数越满，方向是反的，
    /// 而且几乎恒为 1.0。消费掉之后环是空的，占用必须回到 0。
    #[test]
    fn fill_ratio_reflects_current_occupancy() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        assert_eq!(r.fill_ratio(), 0.0, "空环占用为 0");

        let _ = r.push(1);
        let _ = r.push(2);
        assert!(
            (r.fill_ratio() - 0.5).abs() < 1e-9,
            "2/4 项 → 0.5，实得 {}",
            r.fill_ratio()
        );
        assert_eq!(r.occupancy_frames(), 2);

        // 消费掉一半：占用降到 0.25，而不是因为「消费过」而保持满。
        let mut out = Vec::new();
        let _ = r.pop_chunk(&mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(r.occupancy_frames(), 0, "取走后环内不应有残留");
        assert_eq!(r.fill_ratio(), 0.0, "消费后占用必须回落");
    }

    /// 占用比例永远在 0..1 之间。
    #[test]
    fn fill_ratio_is_bounded() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        for i in 0..100 {
            let _ = r.push(i);
        }
        let f = r.fill_ratio();
        assert!((0.0..=1.0).contains(&f), "占用比例必须在 0..1，实得 {f}");
    }

    /// 帧容量下界：项数 × 每项帧数，分子分母单位一致。
    #[test]
    fn fill_ratio_uses_frames_on_both_sides() {
        // 4 项 × 64 帧 = 256 帧容量。写满 2 项 = 128 帧 → 0.5。
        let r: SampleRing<u32> = SampleRing::new(4, 64, DropPolicy::DropNewest);
        let _ = r.push(0);
        let _ = r.push(1);
        assert_eq!(r.occupancy_frames(), 128);
        assert!(
            (r.fill_ratio() - 0.5).abs() < 1e-9,
            "实得 {}",
            r.fill_ratio()
        );
    }

    #[test]
    fn drop_rate_is_zero_when_nothing_dropped() {
        let r: SampleRing<u32> = SampleRing::new(4, 1, DropPolicy::DropNewest);
        let _ = r.push(1);
        let _ = r.push(2);
        assert_eq!(r.stats().drop_rate(), 0.0);
    }
}
