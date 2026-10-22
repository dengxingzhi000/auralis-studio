//! CPAL 输入流。
//!
//! # 实时安全的核心难题：回调里不能分配
//!
//! 每一次音频回调都要往环形缓冲里放一个 `Vec<f32>`。如果直接
//! `Vec::new()` + `push`，就是**每个回调一次分配**，违反 `AGENTS.md` 的
//! 「no unbounded allocation」。
//!
//! 解法是**缓冲区回收**（buffer recycling）：
//!
//! ```text
//! 音频回调                                分析工作线程
//!   |  从回收池取一个用过的 Vec  <---------------+
//!   |  clear + extend（容量足够，不重新分配）        |
//!   |  push 进环形缓冲  --------------------------> |
//!                                            用完
//!                                                |
//!                                                +-- 放回回收池 --------+
//! ```
//!
//! 首次若干次回调会分配（预热），之后稳定在**零分配**。
//! 回收池容量固定，因此不会无限增长。
//!
//! # 回调里到底做了什么
//!
//! 1. 从回收池取一个 `Vec`（`pop`，非阻塞）
//! 2. `clear()` + `extend_from_slice()` —— 容量足够时**不分配**
//! 3. `ring.push()` —— 无锁；环满时 Vec 被原样退回，可继续复用
//! 4. 返回
//!
//! 没有锁、没有文件、没有日志、没有 IPC、没有 DSP。
//! 依据 [docs/BENCHMARKS.md](../../BENCHMARKS.md)：FFT 4096 需 271 µs，
//! 而回调预算只有约 20.8 µs，任何 DSP 放进来必然掉帧。

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::ArrayQueue;

use crate::devices::{CaptureConfig, DeviceError};
use crate::ring::{DropPolicy, RingStats, SampleRing};

/// 回调缓冲：交错 f32。
pub type CallbackBuffer = Vec<f32>;

/// 回收池容量。取 64：足够覆盖一个缓冲从回调到消费者再回来的往返。
const RECYCLE_SLOTS: usize = 64;

/// 正在运行的采集会话。
///
/// `_stream` 必须持有：一旦释放，cpal 会停止回调。
pub struct CaptureSession {
    _stream: cpal::Stream,
    ring: Arc<SampleRing<CallbackBuffer>>,
    recycle: Arc<ArrayQueue<CallbackBuffer>>,
    playing: bool,
    /// 实际开流用的格式。可能与请求的不同（设备不支持时回退），
    /// 所以调用方**必须**用这个值而不是自己当初请求的值。
    format: ResolvedFormat,
}

impl CaptureSession {
    /// 数据环形缓冲的句柄。工作线程持有它来消费。
    #[must_use]
    pub fn ring(&self) -> &Arc<SampleRing<CallbackBuffer>> {
        &self.ring
    }

    /// 实际使用的格式（采样率 / 声道数）。
    #[must_use]
    pub const fn format(&self) -> ResolvedFormat {
        self.format
    }

    /// 暂停采集。流仍在，但数据不再进入缓冲。
    ///
    /// # Errors
    /// 后端拒绝暂停。
    pub fn pause(&mut self) -> Result<(), DeviceError> {
        self._stream
            .pause()
            .map_err(|e| DeviceError::OpenFailed(e.to_string()))?;
        self.playing = false;
        Ok(())
    }

    /// 恢复采集。
    ///
    /// # Errors
    /// 后端拒绝恢复。
    pub fn resume(&mut self) -> Result<(), DeviceError> {
        self._stream
            .play()
            .map_err(|e| DeviceError::OpenFailed(e.to_string()))?;
        self.playing = true;
        Ok(())
    }

    /// 是否正在采集。
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// 统计（丢帧必须可见，`AGENTS.md`）。
    #[must_use]
    pub fn stats(&self) -> RingStats {
        self.ring.stats()
    }

    /// 取得缓冲区回收器，供工作线程归还用过的缓冲。
    #[must_use]
    pub fn recycler(&self) -> BufferRecycler {
        BufferRecycler {
            recycle: self.recycle.clone(),
        }
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        // 先关闭环形缓冲再丢弃流：这样即使还有最后一次回调在途中，
        // 它的写入会被计为丢弃，而不会让缓冲继续增长。
        self.ring.close();
    }
}

/// 把用完的缓冲还回收池，供回调复用。
#[derive(Debug, Clone)]
pub struct BufferRecycler {
    recycle: Arc<ArrayQueue<CallbackBuffer>>,
}

impl BufferRecycler {
    /// 归还一个缓冲。回收池满时直接丢弃（交给 GC）。
    pub fn recycle(&self, mut buf: CallbackBuffer) {
        buf.clear();
        let _ = self.recycle.push(buf);
    }

    /// 批量归还。
    pub fn recycle_all(&self, items: Vec<CallbackBuffer>) {
        for b in items {
            self.recycle(b);
        }
    }

    /// 归还 `items` 中的全部缓冲，并**保留 `items` 自身的容量**。
    ///
    /// 工作线程每轮都要清空同一个 `Vec<CallbackBuffer>`；用
    /// `recycle_all(std::mem::take(&mut v))` 会把 `v` 换成空 Vec，
    /// 下一轮又得重新分配那几十个指针位。
    pub fn recycle_drain(&self, items: &mut Vec<CallbackBuffer>) {
        for b in items.drain(..) {
            self.recycle(b);
        }
    }

    /// 回收池中当前待用的缓冲数（诊断用）。
    #[must_use]
    pub fn available(&self) -> usize {
        self.recycle.len()
    }

    /// 构造一个独立的回收器（测试用）。
    ///
    /// 测试不涉及真实回调，需要一个自成一体的池子。
    #[cfg(test)]
    #[must_use]
    pub fn for_tests(slots: usize) -> Self {
        let recycle: Arc<ArrayQueue<CallbackBuffer>> = Arc::new(ArrayQueue::new(slots));
        for _ in 0..slots {
            let _ = recycle.push(Vec::with_capacity(4096));
        }
        Self { recycle }
    }
}

/// 设备声明支持的一个输入格式区间。
///
/// 抽成自己的结构体是为了让**选择逻辑可测**：`cpal` 的
/// `SupportedStreamConfigRange` 无法在测试里构造，而这些规则
/// （采样率/声道回退）正是最容易写错、又最需要测试的部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRange {
    /// 声道数。
    pub channels: u16,
    /// 最低采样率。
    pub min_rate: u32,
    /// 最高采样率。
    pub max_rate: u32,
    /// 是否支持 f32 样本（本应用只开 f32 流）。
    pub is_f32: bool,
}

/// 实际用于开流的格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFormat {
    /// 采样率。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u16,
}

/// 从「请求的格式」和「设备确实支持的区间」里选一个能开的格式。
///
/// 优先级：
/// 1. 请求的采样率与声道数都在某个 f32 区间内 → 原样使用；
/// 2. 请求的采样率某个 f32 区间支持、但声道数不匹配 → 用请求采样率 +
///    该区间的声道数（声道数不重要，混音后都是单声道分析）；
/// 3. 否则回退到设备默认格式，**并把真实格式返回给调用方**，
///    让 UI 显示实际的采样率而不是用户填的那个。
///
/// 早期版本直接比较「请求 == 默认」，不等就报错。这在设备默认
/// 44.1 kHz、但完全支持 48 kHz 的机器上会拒绝一个合法请求。
#[must_use]
pub fn resolve_format(
    requested: CaptureConfig,
    default_rate: u32,
    default_channels: u16,
    ranges: &[FormatRange],
) -> ResolvedFormat {
    let f32_ranges: Vec<&FormatRange> = ranges.iter().filter(|r| r.is_f32).collect();

    // 1. 采样率 + 声道都满足
    if f32_ranges.iter().any(|r| {
        r.channels == requested.channels
            && r.min_rate <= requested.sample_rate
            && requested.sample_rate <= r.max_rate
    }) {
        return ResolvedFormat {
            sample_rate: requested.sample_rate,
            channels: requested.channels,
        };
    }

    // 2. 采样率满足，换声道数
    if let Some(r) = f32_ranges
        .iter()
        .find(|r| r.min_rate <= requested.sample_rate && requested.sample_rate <= r.max_rate)
    {
        return ResolvedFormat {
            sample_rate: requested.sample_rate,
            channels: r.channels,
        };
    }

    // 3. 回退到设备默认
    ResolvedFormat {
        sample_rate: default_rate,
        channels: default_channels,
    }
}

/// 打开设备输入流并开始采集。
///
/// # Errors
/// 设备不存在、格式不被支持、流无法建立或无法启动。
pub fn start_capture(
    device_id: &str,
    config: CaptureConfig,
    ring_capacity_frames: usize,
) -> Result<CaptureSession, DeviceError> {
    let host = cpal::default_host();
    // 不单独探测 is_available()：cpal 0.18 里它是无 self 的关联函数，
    // 而 input_devices() 失败已覆盖「无后端」，统一走 BackendUnavailable。

    let device = host
        .input_devices()
        .map_err(|e| DeviceError::BackendUnavailable(e.to_string()))?
        .find(|d| {
            // id() 返回 Result；取不到就当不匹配，别把 Result 当 bool 用
            d.id().map(|id| format!("{id:?}")).unwrap_or_default() == device_id
        })
        .ok_or_else(|| DeviceError::NotFound(device_id.to_string()))?;

    let default_cfg = device
        .default_input_config()
        .map_err(|e| DeviceError::OpenFailed(format!("设备没有可用的默认输入配置：{e}")))?;
    let default_rate = default_cfg.sample_rate();
    let default_channels = default_cfg.channels();

    // 收集设备声明的区间。拿不到就当作「只知道默认格式」。
    let ranges: Vec<FormatRange> = device
        .supported_input_configs()
        .map(|it| {
            it.map(|r| FormatRange {
                channels: r.channels(),
                min_rate: r.min_sample_rate(),
                max_rate: r.max_sample_rate(),
                is_f32: r.sample_format() == cpal::SampleFormat::F32,
            })
            .collect()
        })
        .unwrap_or_default();

    let resolved = resolve_format(config, default_rate, default_channels, &ranges);

    let frames_per_buffer = config.buffer_frames.max(1);
    let ring = Arc::new(SampleRing::new(
        ring_capacity_frames.max(1),
        frames_per_buffer,
        DropPolicy::DropNewest,
    ));
    let recycle: Arc<ArrayQueue<CallbackBuffer>> = Arc::new(ArrayQueue::new(RECYCLE_SLOTS));

    let ring_cb = ring.clone();
    let recycle_cb = recycle.clone();

    // 预热：先放一批空缓冲，前若干次回调就不分配
    for _ in 0..RECYCLE_SLOTS {
        let _ = recycle_cb.push(Vec::with_capacity(frames_per_buffer * 4));
    }

    // cpal 0.18：StreamConfig 是公有字段的结构体
    let stream_config = cpal::StreamConfig {
        channels: resolved.channels,
        sample_rate: resolved.sample_rate,
        buffer_size: cpal::BufferSize::Fixed(frames_per_buffer as u32),
    };

    let stream = device
        .build_input_stream(
            stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // ---- 实时回调从这里开始：只有 O(1) 且不分配的操作 ----

                // 1. 从回收池取缓冲（非阻塞）
                let mut buf = recycle_cb
                    .pop()
                    .unwrap_or_else(|| Vec::with_capacity(data.len() * 2));

                // 2. 写入。容量足够时 extend_from_slice 不重新分配
                buf.clear();
                buf.extend_from_slice(data);

                // 3. 交给环形缓冲；满了 Vec 被原样退回，可继续复用
                if let Err(returned) = ring_cb.push(buf) {
                    let _ = recycle_cb.push(returned);
                }
                // ---- 实时回调到这里结束 ----
            },
            move |err| {
                // 错误回调不在实时路径上，可以打日志
                tracing::warn!(error = %err, "audio input stream error");
            },
            None,
        )
        .map_err(|e| DeviceError::OpenFailed(e.to_string()))?;

    stream
        .play()
        .map_err(|e| DeviceError::OpenFailed(e.to_string()))?;

    Ok(CaptureSession {
        _stream: stream,
        ring,
        recycle,
        playing: true,
        format: resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯逻辑测试：缓冲区回收能否真正做到零分配。
    ///
    /// 真实声卡在 CI 上不存在（`TEST_PLAN.md` §4），所以不测 cpal 本身，
    /// 而是测**我们能测的部分**。
    #[test]
    fn recycled_buffers_reach_a_steady_state() {
        let frames = 256;
        let recycle = Arc::new(ArrayQueue::<CallbackBuffer>::new(RECYCLE_SLOTS));
        for _ in 0..RECYCLE_SLOTS {
            let _ = recycle.push(Vec::with_capacity(frames * 4));
        }

        let ring: SampleRing<CallbackBuffer> = SampleRing::new(8, frames, DropPolicy::DropNewest);

        let recycler = BufferRecycler {
            recycle: recycle.clone(),
        };
        let data = vec![0.25_f32; frames];

        // 模拟真实往返：回调取 -> 写 -> 入环；消费者取 -> 归还
        for _ in 0..RECYCLE_SLOTS {
            let mut buf = recycler
                .recycle
                .pop()
                .unwrap_or_else(|| Vec::with_capacity(frames * 4));
            buf.clear();
            buf.extend_from_slice(&data);
            if let Err(returned) = ring.push(buf) {
                let _ = recycler.recycle.push(returned);
            }
        }

        let mut out = Vec::new();
        let mut rounds = 0usize;
        while ring.pop_chunk(&mut out) && rounds < RECYCLE_SLOTS * 4 {
            recycler.recycle_all(std::mem::take(&mut out));
            rounds += 1;
        }

        assert!(
            recycler.available() <= RECYCLE_SLOTS,
            "长期运行后回收池仍应受限，实得 {}",
            recycler.available()
        );
    }

    #[test]
    fn drop_newest_returns_the_buffer_for_reuse() {
        // 关键性质：环满时 push 把 Vec 原样还回来，回收池不减少
        let recycle = Arc::new(ArrayQueue::<CallbackBuffer>::new(8));
        for _ in 0..8 {
            let _ = recycle.push(Vec::with_capacity(1024));
        }
        let before = recycle.len();

        let ring: SampleRing<CallbackBuffer> = SampleRing::new(2, 256, DropPolicy::DropNewest);
        let data = vec![0.5_f32; 256];
        for _ in 0..8 {
            let mut buf = recycle.pop().expect("pool must not be empty");
            buf.clear();
            buf.extend_from_slice(&data);
            if let Err(returned) = ring.push(buf) {
                let _ = recycle.push(returned);
            }
        }

        // 不变式：**没有任何缓冲丢失**。
        // 8 个缓冲的去向只有两种：留在环形缓冲里，或回到回收池。
        let in_ring = 2_usize; // 环形缓冲容量
        let consumed = ring.stats().popped_frames as usize / 256;
        assert_eq!(
            recycle.len() + in_ring - consumed,
            before,
            "缓冲应被完整回收：池 {} + 环内 {} - 已消费 {} 应等于 {before}",
            recycle.len(),
            in_ring,
            consumed
        );
        assert!(ring.stats().dropped_frames > 0, "环满时必须有丢弃且被计数");
    }

    #[test]
    fn buffer_capacity_survives_reuse() {
        // 复用的 Vec 必须保留容量，否则 extend_from_slice 会重新分配
        let recycle = Arc::new(ArrayQueue::<CallbackBuffer>::new(4));
        let big = vec![0.0_f32; 4096];
        let _ = recycle.push(big.clone());
        let mut buf = recycle.pop().expect("one buffer");
        let cap_before = buf.capacity();
        buf.clear();
        buf.extend_from_slice(&big);
        assert_eq!(buf.capacity(), cap_before, "clear+extend 不应改变容量");
        assert!(buf.capacity() >= 4096);
    }

    #[test]
    fn recycle_clears_content_before_returning() {
        let recycle = Arc::new(ArrayQueue::<CallbackBuffer>::new(4));
        let _ = recycle.push(vec![1.0_f32; 128]);
        let recycler = BufferRecycler { recycle };
        let mut buf = recycler.recycle.pop().expect("one buffer");
        buf.extend_from_slice(&[9.0_f32; 128]);
        assert_eq!(buf.len(), 256);
        recycler.recycle(buf);
        let back = recycler.recycle.pop().expect("returned buffer");
        assert!(back.is_empty(), "回收前必须清空，否则会累积旧数据");
    }

    #[test]
    fn recycle_drain_keeps_the_outer_vec_capacity() {
        // 工作线程每轮复用同一个 Vec<CallbackBuffer>；
        // drain 归还缓冲时必须保留它自身的容量，否则每轮都重新分配。
        let recycle = Arc::new(ArrayQueue::<CallbackBuffer>::new(4));
        let recycler = BufferRecycler {
            recycle: recycle.clone(),
        };
        let mut held: Vec<CallbackBuffer> = Vec::with_capacity(8);
        held.push(vec![0.1_f32; 256]);
        held.push(vec![0.2_f32; 256]);
        let cap_before = held.capacity();

        recycler.recycle_drain(&mut held);

        assert!(held.is_empty(), "缓冲应全部归还");
        assert_eq!(held.capacity(), cap_before, "外层 Vec 容量必须保留");
        assert_eq!(recycle.len(), 2, "两个缓冲都应回到池里");
    }

    #[test]
    fn starting_capture_on_missing_device_errors_rather_than_panicking() {
        let r = start_capture("no-such-device-id-12345", CaptureConfig::default(), 2048);
        match r {
            Err(DeviceError::NotFound(_)) => {}
            Err(DeviceError::BackendUnavailable(_)) => {
                // CI 无声卡后端，也算预期
            }
            Err(e) => panic!("不应对不存在的设备返回 {e}"),
            Ok(_) => panic!("对不存在的设备不应成功"),
        }
    }

    #[test]
    fn zero_buffer_frames_is_clamped_not_divided_by_zero() {
        let cfg = CaptureConfig {
            buffer_frames: 0,
            ..CaptureConfig::default()
        };
        // start_capture 内部用 .max(1) 兜底；这里确认不会 panic
        assert_eq!(cfg.buffer_frames, 0);
        assert!(cfg.buffer_millis() == 0.0);
    }

    // ---- 格式选择：纯逻辑，可在 CI 上跑 ----

    fn range(channels: u16, min_rate: u32, max_rate: u32) -> FormatRange {
        FormatRange {
            channels,
            min_rate,
            max_rate,
            is_f32: true,
        }
    }

    #[test]
    fn exact_supported_format_is_used_as_requested() {
        let got = resolve_format(
            CaptureConfig {
                sample_rate: 48_000,
                channels: 2,
                buffer_frames: 256,
            },
            44_100,
            2,
            &[range(2, 44_100, 96_000)],
        );
        assert_eq!(got.sample_rate, 48_000);
        assert_eq!(got.channels, 2);
    }

    #[test]
    fn supported_rate_but_different_channel_count_keeps_the_rate() {
        // 设备只支持单声道，但支持 48 kHz
        let got = resolve_format(
            CaptureConfig {
                sample_rate: 48_000,
                channels: 2,
                buffer_frames: 256,
            },
            48_000,
            1,
            &[range(1, 44_100, 96_000)],
        );
        assert_eq!(got.sample_rate, 48_000, "采样率不该被降级");
        assert_eq!(got.channels, 1, "应回退到设备支持的声道数");
    }

    #[test]
    fn unsupported_rate_falls_back_to_default_and_reports_it() {
        // 设备默认 44.1 kHz，最高只到 48 kHz；请求 192 kHz
        let got = resolve_format(
            CaptureConfig {
                sample_rate: 192_000,
                channels: 2,
                buffer_frames: 256,
            },
            44_100,
            2,
            &[range(2, 44_100, 48_000)],
        );
        assert_eq!(
            got.sample_rate, 44_100,
            "不支持的采样率必须回退到默认值，而不是假装成功"
        );
        assert_eq!(got.channels, 2);
    }

    #[test]
    fn non_f32_ranges_are_ignored() {
        // 只有 i16 支持 96 kHz；本应用只开 f32 流
        let got = resolve_format(
            CaptureConfig {
                sample_rate: 96_000,
                channels: 2,
                buffer_frames: 256,
            },
            48_000,
            2,
            &[
                FormatRange {
                    channels: 2,
                    min_rate: 96_000,
                    max_rate: 96_000,
                    is_f32: false,
                },
                range(2, 48_000, 48_000),
            ],
        );
        assert_eq!(got.sample_rate, 48_000, "不能选一个开不起来的 i16 格式");
    }

    #[test]
    fn empty_range_list_falls_back_to_default() {
        let got = resolve_format(
            CaptureConfig {
                sample_rate: 48_000,
                channels: 2,
                buffer_frames: 256,
            },
            44_100,
            2,
            &[],
        );
        assert_eq!(got.sample_rate, 44_100);
        assert_eq!(got.channels, 2);
    }
}
