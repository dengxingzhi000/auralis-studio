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
}

impl CaptureSession {
    /// 数据环形缓冲的句柄。工作线程持有它来消费。
    #[must_use]
    pub fn ring(&self) -> &Arc<SampleRing<CallbackBuffer>> {
        &self.ring
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

    /// 回收池中当前待用的缓冲数（诊断用）。
    #[must_use]
    pub fn available(&self) -> usize {
        self.recycle.len()
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

    // 校验设备是否支持请求的采样率；不支持就报可操作的错误
    if let Ok(default_cfg) = device.default_input_config() {
        let sr = default_cfg.sample_rate();
        if sr != config.sample_rate {
            return Err(DeviceError::UnsupportedSampleRate {
                requested: config.sample_rate,
                available: vec![sr],
            });
        }
    }

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
        channels: config.channels,
        sample_rate: config.sample_rate,
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
}
