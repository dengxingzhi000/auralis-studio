//! PCM 数据模型。
//!
//! `docs/TAD.md` §5 与 `docs/DSP_SPEC.md` §2 的落地：
//!
//! - 解码后统一为**交错 `f32`**，满刻度 ±1.0（`i16` 的 ±32768 映射到 ±1.0）。
//! - `first_frame` 以**帧**为单位，不是字节、不是样本。
//! - 样本数必须能被声道数整除；不整除的尾部不完整帧由构造函数拒绝。
//! - 不因为绘图方便而重采样，保留源采样率。

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// 声道布局。首版只建模「按交错顺序排列」，不预设 L/R 语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelLayout {
    /// 未知布局，解码器未能提供语义。
    Unknown,
    /// 单声道。
    Mono,
    /// 双声道，交错顺序由容器决定。
    Stereo,
    /// 多于两声道，按容器顺序。
    Multichannel,
}

impl ChannelLayout {
    /// 由声道数推断布局。不做「2 声道一定叫立体声」的强断言——
    /// 双单声道文件也是 2 声道，只是布局不同；这里只做数量级映射，
    /// 语义交给解码器在元数据里覆盖。
    #[must_use]
    pub const fn from_channel_count(channels: u32) -> Self {
        match channels {
            1 => Self::Mono,
            2 => Self::Stereo,
            0 => Self::Unknown,
            _ => Self::Multichannel,
        }
    }
}

/// 音频格式：采样率 + 声道数。
///
/// `sample_rate` 与 `channels` 均为 [`NonZeroU32`]，从类型上排除「采样率为 0」
/// 「声道数为 0」这类会让 DSP 静默产出垃圾数据的输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    /// 采样率，Hz。
    pub sample_rate: NonZeroU32,
    /// 声道数。
    pub channels: NonZeroU32,
}

impl AudioFormat {
    /// 构造格式；`0` 会被拒绝。
    pub fn new(sample_rate: u32, channels: u32) -> Result<Self, PcmFormatError> {
        let sample_rate = NonZeroU32::new(sample_rate).ok_or(PcmFormatError::ZeroSampleRate)?;
        let channels = NonZeroU32::new(channels).ok_or(PcmFormatError::ZeroChannels)?;
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    /// 单声道构造快捷方式。
    pub fn mono(sample_rate: u32) -> Result<Self, PcmFormatError> {
        Self::new(sample_rate, 1)
    }

    /// 每秒帧数，等于采样率。
    #[must_use]
    pub const fn frames_per_second(&self) -> u32 {
        self.sample_rate.get()
    }

    /// 奈奎斯特频率，Hz。
    #[must_use]
    pub const fn nyquist_hz(&self) -> f64 {
        self.sample_rate.get() as f64 / 2.0
    }
}

/// PCM 格式错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PcmFormatError {
    /// 采样率为 0。
    #[error("sample_rate must be greater than zero")]
    ZeroSampleRate,
    /// 声道数为 0。
    #[error("channel count must be greater than zero")]
    ZeroChannels,
    /// 样本数不能被声道数整除，说明存在不完整帧。
    #[error("sample count {samples} is not a multiple of channel count {channels}")]
    IncompleteFrame {
        /// 实际样本数。
        samples: usize,
        /// 声道数。
        channels: u32,
    },
}

/// 一块 PCM 数据。
///
/// 交错布局：`samples[frame * channels + channel]`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcmBlock {
    /// 该块的格式。
    pub format: AudioFormat,
    /// 该块**第一帧**在整条音频流中的帧偏移。
    pub first_frame: u64,
    /// 交错样本，线性幅度，满刻度 ±1.0。
    pub samples: Vec<f32>,
}

impl PcmBlock {
    /// 构造 PCM 块，拒绝样本数与声道数不匹配的输入。
    ///
    /// `DSP_SPEC.md` §2 明确要求「校验帧边界与通道数」「对空输入、部分帧和长度不足
    /// 返回明确结果/错误」，因此这里是错误而不是静默截断。
    pub fn new(
        format: AudioFormat,
        first_frame: u64,
        samples: Vec<f32>,
    ) -> Result<Self, PcmFormatError> {
        let channels = format.channels.get() as usize;
        if !samples.len().is_multiple_of(channels) {
            return Err(PcmFormatError::IncompleteFrame {
                samples: samples.len(),
                channels: format.channels.get(),
            });
        }
        Ok(Self {
            format,
            first_frame,
            samples,
        })
    }

    /// 空块。
    #[must_use]
    pub fn empty(format: AudioFormat, first_frame: u64) -> Self {
        Self {
            format,
            first_frame,
            samples: Vec::new(),
        }
    }

    /// 块内帧数。
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.format.channels.get() as usize
    }

    /// 块结束后的帧偏移（不含）。
    #[must_use]
    pub fn end_frame(&self) -> u64 {
        self.first_frame + self.frame_count() as u64
    }

    /// 取出某一通道的样本为独立 `Vec`。
    ///
    /// 仅在算法确实需要逐通道处理时调用；否则用 [`PcmBlock::channel_view`]
    /// 避免复制。
    #[must_use]
    pub fn to_channel(&self, channel: u32) -> Vec<f32> {
        let channels = self.format.channels.get();
        if channel >= channels {
            return Vec::new();
        }
        let ch = channel as usize;
        self.samples
            .iter()
            .skip(ch)
            .step_by(channels as usize)
            .copied()
            .collect()
    }

    /// 借用某一通道视图，不复制。
    ///
    /// # Panics
    /// `channel >= 声道数` 时 panic。这是编程错误而非运行时输入错误，
    /// 应当在边界处先做校验。
    pub fn channel_view(&self, channel: u32) -> impl Iterator<Item = f32> + '_ {
        let channels = self.format.channels.get() as usize;
        let ch = channel as usize;
        assert!(
            ch < channels,
            "channel {channel} out of range for {channels} channel(s)"
        );
        self.samples.iter().skip(ch).step_by(channels).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_zero_sample_rate() {
        assert_eq!(AudioFormat::new(0, 2), Err(PcmFormatError::ZeroSampleRate));
    }

    #[test]
    fn new_rejects_zero_channels() {
        assert_eq!(
            AudioFormat::new(48_000, 0),
            Err(PcmFormatError::ZeroChannels)
        );
    }

    #[test]
    fn frame_count_and_end_frame() {
        let format = AudioFormat::new(48_000, 2).unwrap();
        // 3 帧立体声 = 6 个样本
        let block = PcmBlock::new(format, 1_000, vec![0.0; 6]).unwrap();
        assert_eq!(block.frame_count(), 3);
        assert_eq!(block.end_frame(), 1_003);
    }

    #[test]
    fn new_rejects_incomplete_frame() {
        let format = AudioFormat::new(48_000, 2).unwrap();
        // 5 个样本 / 2 声道 = 2.5 帧
        assert_eq!(
            PcmBlock::new(format, 0, vec![0.0; 5]),
            Err(PcmFormatError::IncompleteFrame {
                samples: 5,
                channels: 2
            })
        );
    }

    #[test]
    fn interleaved_channel_view_is_correct() {
        let format = AudioFormat::new(48_000, 2).unwrap();
        // L0 R0 L1 R1 L2 R2
        let block = PcmBlock::new(format, 0, vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0]).unwrap();

        assert_eq!(
            block.channel_view(0).collect::<Vec<_>>(),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(
            block.channel_view(1).collect::<Vec<_>>(),
            vec![-1.0, -2.0, -3.0]
        );
        assert_eq!(block.to_channel(1), vec![-1.0, -2.0, -3.0]);
    }

    #[test]
    fn channel_view_panics_on_out_of_range() {
        let format = AudioFormat::new(48_000, 2).unwrap();
        let block = PcmBlock::new(format, 0, vec![0.0; 4]).unwrap();
        // 单声道样本第 2 声道越界，属于编程错误
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = block.channel_view(2).count();
            }))
            .is_err()
        );
    }

    #[test]
    fn nyquist_is_half_sample_rate() {
        let format = AudioFormat::new(48_000, 2).unwrap();
        assert!((format.nyquist_hz() - 24_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_block_has_zero_frames() {
        let format = AudioFormat::new(44_100, 1).unwrap();
        let block = PcmBlock::empty(format, 42);
        assert_eq!(block.frame_count(), 0);
        assert_eq!(block.end_frame(), 42);
    }
}
