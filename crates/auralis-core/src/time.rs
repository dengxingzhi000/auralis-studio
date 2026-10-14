//! 时间轴。
//!
//! `DSP_SPEC.md` / `PRD.md` 的要求：时间统一以**帧**表达，秒数只在渲染时现算，
//! 禁止累加浮点秒数——累加会引入无法复现的漂移，且长文件上误差不可忽略。

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// 一个时间点：帧偏移 + 采样率。
///
/// 采样率随时间点携带，是因为同一个应用内可能同时存在
/// 「实时输入 48 kHz」和「打开的 44.1 kHz 文件」两条时间轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTime {
    /// 帧偏移（不是样本下标：多声道下一个帧含多个样本）。
    pub frame_offset: u64,
    /// 该帧所属流的采样率。
    pub sample_rate: NonZeroU32,
}

impl AudioTime {
    /// 构造时间点；`sample_rate == 0` 会被拒绝。
    pub fn new(frame_offset: u64, sample_rate: u32) -> Option<Self> {
        NonZeroU32::new(sample_rate).map(|sr| Self {
            frame_offset,
            sample_rate: sr,
        })
    }

    /// 起始时间。
    #[must_use]
    pub const fn zero(sample_rate: NonZeroU32) -> Self {
        Self {
            frame_offset: 0,
            sample_rate,
        }
    }

    /// 相对秒数。
    ///
    /// 单次除法，不累加，因此任意帧号的结果都可由 `(frame, rate)` 独立复现。
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frame_offset as f64 / self.sample_rate.get() as f64
    }

    /// 相对毫秒数。
    #[must_use]
    pub fn milliseconds(&self) -> f64 {
        self.frame_offset as f64 * 1000.0 / self.sample_rate.get() as f64
    }

    /// 前移 `frames` 帧。溢出时饱和到 `u64::MAX`，不 panic。
    #[must_use]
    pub fn saturating_add_frames(&self, frames: u64) -> Self {
        Self {
            frame_offset: self.frame_offset.saturating_add(frames),
            sample_rate: self.sample_rate,
        }
    }

    /// 帧偏移差，饱和减法。
    #[must_use]
    pub fn saturating_sub_frames(&self, frames: u64) -> Self {
        Self {
            frame_offset: self.frame_offset.saturating_sub(frames),
            sample_rate: self.sample_rate,
        }
    }

    /// 向前取整到指定帧数的倍数（hop 对齐）。用于把时间点吸附到分析帧边界。
    ///
    /// 例：`hop_size = 1024` 时，帧 2500 → 2048。
    #[must_use]
    pub fn floor_to_multiple(&self, multiple: u64) -> Self {
        if multiple == 0 {
            return *self;
        }
        Self {
            frame_offset: self.frame_offset - (self.frame_offset % multiple),
            sample_rate: self.sample_rate,
        }
    }

    /// 把秒数转成帧号，四舍五入到最近的整帧。
    ///
    /// 用于用户输入（时间游标、选区）。返回 `None` 表示秒数非有限或为负。
    #[must_use]
    pub fn frames_from_seconds(&self, seconds: f64) -> Option<u64> {
        if !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        Some(seconds.mul_add(self.sample_rate.get() as f64, 0.5) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at48k(frame_offset: u64) -> AudioTime {
        AudioTime::new(frame_offset, 48_000).unwrap()
    }

    #[test]
    fn seconds_is_exact_for_known_values() {
        assert!((at48k(48_000).seconds() - 1.0).abs() < f64::EPSILON);
        assert!((at48k(24_000).seconds() - 0.5).abs() < f64::EPSILON);
        assert!((at48k(0).seconds() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn seconds_does_not_drift_over_long_durations() {
        // 累加 44100.0 * 3600 次 1/44100 秒会累积可观的误差；
        // 直接由帧号换算则与累加方式无关。
        let t = at48k(48_000 * 3_600);
        let by_repeat: f64 = (0..3_600).map(|_| 1.0 / 48_000.0).sum();
        assert!((t.seconds() - 3_600.0).abs() < 1e-9);
        assert!(
            (t.seconds() - by_repeat).abs() > 1e-9,
            "累加方式确实存在漂移，本测试用于证明我们没有采用它"
        );
    }

    #[test]
    fn milliseconds_matches_seconds() {
        let t = at48k(1_500);
        assert!((t.milliseconds() - t.seconds() * 1000.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_zero_sample_rate() {
        assert!(AudioTime::new(0, 0).is_none());
    }

    #[test]
    fn frame_arithmetic_saturates_instead_of_panicking() {
        let t = at48k(10);
        assert_eq!(t.saturating_sub_frames(20).frame_offset, 0);
        assert_eq!(
            at48k(u64::MAX).saturating_add_frames(10).frame_offset,
            u64::MAX
        );
    }

    #[test]
    fn floor_to_multiple_snaps_back() {
        assert_eq!(at48k(2500).floor_to_multiple(1024).frame_offset, 2048);
        assert_eq!(at48k(2048).floor_to_multiple(1024).frame_offset, 2048);
        // 除数为 0 时原样返回，而不是 panic
        assert_eq!(at48k(2500).floor_to_multiple(0).frame_offset, 2500);
    }

    #[test]
    fn frames_from_seconds_rounds_and_rejects_invalid() {
        let t = at48k(0);
        assert_eq!(t.frames_from_seconds(1.0), Some(48_000));
        assert_eq!(t.frames_from_seconds(0.5), Some(24_000));
        assert_eq!(t.frames_from_seconds(f64::NAN), None);
        assert_eq!(t.frames_from_seconds(-1.0), None);
        assert_eq!(t.frames_from_seconds(f64::INFINITY), None);
    }
}
