//! 设备枚举与采集配置。
//!
//! **测试不依赖真实声卡**：本模块的测试只验证纯函数（ID 解析、格式选择、
//! 参数推导），设备枚举本身用「有设备就查、没有就跳过」的方式，
//! 这样 CI 的 headless runner 上也能通过
//! （`docs/TEST_PLAN.md` §4：普通 CI 不应假设一定有音频设备）。

use std::num::NonZeroU32;

use thiserror::Error;

/// 设备相关错误。
#[derive(Debug, Error)]
pub enum DeviceError {
    /// 找不到指定设备。
    #[error("audio device not found: {0}")]
    NotFound(String),

    /// 设备存在但不支持请求的采样率。
    #[error("device does not support {requested} Hz; supported: {available:?}")]
    UnsupportedSampleRate {
        /// 请求的采样率。
        requested: u32,
        /// 设备声明支持的采样率列表。
        available: Vec<u32>,
    },

    /// 设备不存在任何输入配置。
    #[error("device has no input configuration")]
    NoInputConfig,

    /// 后端不可用（CI / 无音频服务）。
    #[error("audio backend unavailable: {0}")]
    BackendUnavailable(String),

    /// 打开流失败。
    #[error("failed to open input stream: {0}")]
    OpenFailed(String),
}

/// 采集参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    /// 采样率，Hz。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u16,
    /// 缓冲帧数。
    ///
    /// 越小延迟越低，但对调度抖动越敏感。48 kHz 下 256 帧 ≈ 5.3 ms。
    pub buffer_frames: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            buffer_frames: 256,
        }
    }
}

impl CaptureConfig {
    /// 缓冲区对应的毫秒数。
    #[must_use]
    pub fn buffer_millis(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.buffer_frames as f64 * 1000.0 / f64::from(self.sample_rate)
        }
    }

    /// 推导内部格式，供 `AudioFormat` 构造。
    ///
    /// # Errors
    /// 采样率或声道数为 0。
    pub fn to_audio_format(&self) -> Result<auralis_core::AudioFormat, DeviceError> {
        let sr = NonZeroU32::new(self.sample_rate).ok_or(DeviceError::UnsupportedSampleRate {
            requested: self.sample_rate,
            available: Vec::new(),
        })?;
        let ch = NonZeroU32::new(u32::from(self.channels)).ok_or(
            DeviceError::UnsupportedSampleRate {
                requested: self.sample_rate,
                available: Vec::new(),
            },
        )?;
        Ok(auralis_core::AudioFormat {
            sample_rate: sr,
            channels: ch,
        })
    }
}

/// 输入设备信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    /// 稳定标识。
    pub id: String,
    /// 设备名称。
    pub name: String,
    /// 设备声明支持的输入采样率。
    pub supported_sample_rates: Vec<u32>,
    /// 输入声道数。
    pub input_channels: u16,
}

impl AudioDeviceInfo {
    /// 是否支持请求的采样率。
    ///
    /// 空列表视为「未知，按支持处理」——很多驱动不申报采样率，
    /// 此时由打开流时的实际协商决定。
    #[must_use]
    pub fn supports(&self, sample_rate: u32) -> bool {
        self.supported_sample_rates.is_empty() || self.supported_sample_rates.contains(&sample_rate)
    }

    /// 在设备声明的采样率中挑选最接近请求值的那个。
    #[must_use]
    pub fn nearest_sample_rate(&self, desired: u32) -> Option<u32> {
        self.supported_sample_rates
            .iter()
            .copied()
            .min_by_key(|r| r.abs_diff(desired))
    }
}

/// 列出输入设备。
///
/// 无音频后端时返回 `Err(BackendUnavailable)` —— 调用方应把它当作
/// 「这台机器没有声卡」而非致命错误。
///
/// # Errors
/// 后端不可用或枚举失败。
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>, DeviceError> {
    use cpal::traits::HostTrait;

    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| DeviceError::BackendUnavailable(e.to_string()))?;

    Ok(devices.map(|d| AudioDeviceInfo::from_cpal(&d)).collect())
}

impl AudioDeviceInfo {
    /// 从 cpal 设备抽取信息。
    ///
    /// 刻意只用 `default_input_config()`：它是跨平台稳定的最小集合。
    /// `supported_input_configs()` 在不同平台上返回的迭代器类型不同，
    /// 而驱动不申报采样率本来就是常态（此时留空，由 [`Self::supports`]
    /// 按「未知即不拒绝」处理）。少依赖一个 API 少一份跨平台风险。
    fn from_cpal<T>(device: &T) -> Self
    where
        T: cpal::traits::DeviceTrait,
    {
        let id = device
            .id()
            .map(|v| format!("{v:?}"))
            .unwrap_or_else(|_| "(unknown)".to_string());

        // cpal 0.18: sample_rate() returns SampleRate directly (plain u32),
        // channels() returns ChannelCount (plain u16). Neither is a Result.
        let (rates, channels) = match device.default_input_config() {
            Ok(cfg) => {
                let sr: u32 = cfg.sample_rate();
                let ch: u16 = cfg.channels();
                let list = if sr > 0 { vec![sr] } else { Vec::new() };
                (list, ch)
            }
            Err(_) => (Vec::new(), 0),
        };

        Self {
            id,
            name: "(see system mixer)".to_string(),
            supported_sample_rates: rates,
            input_channels: channels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let c = CaptureConfig::default();
        assert_eq!(c.sample_rate, 48_000);
        assert_eq!(c.channels, 2);
        assert!(c.buffer_millis() < 10.0, "默认缓冲应低于 10 ms");
    }

    #[test]
    fn buffer_millis_is_correct() {
        // 256 帧 @ 48 kHz = 5.333 ms
        let c = CaptureConfig {
            sample_rate: 48_000,
            channels: 2,
            buffer_frames: 256,
        };
        assert!((c.buffer_millis() - 5.333_333).abs() < 1e-4);
    }

    #[test]
    fn buffer_millis_of_zero_rate_is_zero_not_infinite() {
        let c = CaptureConfig {
            sample_rate: 0,
            channels: 2,
            buffer_frames: 256,
        };
        assert_eq!(c.buffer_millis(), 0.0);
    }

    #[test]
    fn to_audio_format_succeeds_for_valid_config() {
        let f = CaptureConfig::default().to_audio_format().expect("valid");
        assert_eq!(f.sample_rate.get(), 48_000);
        assert_eq!(f.channels.get(), 2);
    }

    #[test]
    fn to_audio_format_rejects_zero_channels() {
        let c = CaptureConfig {
            channels: 0,
            ..CaptureConfig::default()
        };
        assert!(c.to_audio_format().is_err());
    }

    #[test]
    fn device_without_reported_rates_accepts_anything() {
        // 很多驱动不申报采样率，此时不应直接拒绝
        let d = AudioDeviceInfo {
            id: "x".into(),
            name: "x".into(),
            supported_sample_rates: Vec::new(),
            input_channels: 2,
        };
        assert!(d.supports(44_100));
        assert!(d.supports(192_000));
    }

    #[test]
    fn device_rejects_unsupported_rate() {
        let d = AudioDeviceInfo {
            id: "x".into(),
            name: "x".into(),
            supported_sample_rates: vec![44_100, 48_000],
            input_channels: 2,
        };
        assert!(d.supports(48_000));
        assert!(!d.supports(96_000));
    }

    #[test]
    fn nearest_rate_picks_closest() {
        let d = AudioDeviceInfo {
            id: "x".into(),
            name: "x".into(),
            supported_sample_rates: vec![44_100, 48_000, 96_000],
            input_channels: 2,
        };
        assert_eq!(d.nearest_sample_rate(47_000), Some(48_000));
        assert_eq!(d.nearest_sample_rate(44_000), Some(44_100));
        assert_eq!(d.nearest_sample_rate(192_000), Some(96_000));
    }

    #[test]
    fn nearest_rate_of_unreported_device_is_none() {
        let d = AudioDeviceInfo {
            id: "x".into(),
            name: "x".into(),
            supported_sample_rates: Vec::new(),
            input_channels: 2,
        };
        assert_eq!(d.nearest_sample_rate(48_000), None);
    }

    /// 设备枚举在有无声卡的环境上表现一致。
    ///
    /// TEST_PLAN §4：普通 CI 不应假设一定有音频设备，
    /// 因此「无设备」也必须是可接受的结果。
    #[test]
    fn device_enumeration_never_panics() {
        match list_input_devices() {
            Ok(devs) => {
                for d in &devs {
                    assert!(!d.id.is_empty());
                    assert_eq!(
                        d.supported_sample_rates.len(),
                        d.supported_sample_rates
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len(),
                        "采样率列表不应有重复"
                    );
                }
                eprintln!("本机发现 {} 个输入设备", devs.len());
            }
            Err(DeviceError::BackendUnavailable(_)) => {
                eprintln!("本机无可用音频后端（CI 预期内）");
            }
            Err(e) => panic!("设备枚举不应返回其它错误: {e}"),
        }
    }
}
