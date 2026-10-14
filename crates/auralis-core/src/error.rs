//! 类型化错误。
//!
//! `docs/TAD.md` §7 要求区分：不支持格式、文件损坏或截断、参数无效、
//! 输入长度不足、设备不可用、格式协商失败、实时缓冲溢出、任务取消、持久化失败。
//!
//! 本模块先落地与领域相关的部分；设备与实时相关错误在 M4 随 `auralis-audio` 加入。

use std::fmt;

use crate::format::PcmFormatError;

/// 领域错误。
///
/// 刻意**不**提供隐式的错误映射：调用方必须显式写出自己知道自己在做什么的
/// 转换（`?` 或 `map_err`）。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuralisError {
    /// 采样率、声道数等格式参数非法。
    #[error("invalid audio format: {0}")]
    Format(#[from] PcmFormatError),

    /// 分析参数非法。附带足以定位问题的字段名。
    #[error("invalid config: {0}")]
    Config(#[from] ConfigError),

    /// 输入样本数量不足以完成所请求的计算。
    #[error("insufficient input: need {needed} sample(s), have {available}")]
    InsufficientInput {
        /// 算法需要的样本数。
        needed: usize,
        /// 实际提供的样本数。
        available: usize,
    },

    /// 计算过程中出现非有限值，且算法未定义该输入的行为。
    #[error("non-finite value in input: {0}")]
    NonFiniteInput(&'static str),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, AuralisError>;

/// 分析参数错误。
///
/// 遵循 `AGENTS.md`「优先使用显式类型化配置和类型化错误，而不是字符串键值映射」：
/// 这里用枚举而不是 `HashMap<String, String>`。
///
/// 注意：不派生 `Eq`，因为变体里含 `f32` / `f64` 字段。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// FFT 长度为 0。
    #[error("fft_size must be greater than zero")]
    FftSizeZero,

    /// Hop size 为 0，会导致分析无法推进。
    #[error("hop_size must be greater than zero")]
    HopSizeZero,

    /// Hop size 超过 FFT 长度，跳帧会跨越整帧，定义不明确。
    #[error("hop_size ({hop}) must not exceed fft_size ({fft})")]
    HopExceedsFrame {
        /// 请求的 hop size。
        hop: usize,
        /// 请求的 fft size。
        fft: usize,
    },

    /// dB 显示范围上下限颠倒。
    #[error("min_db ({min}) must not be greater than max_db ({max})")]
    DbRangeInverted {
        /// 下限。
        min: f32,
        /// 上限。
        max: f32,
    },

    /// 请求的频率超出奈奎斯特频率。
    #[error("requested frequency {requested_hz} Hz exceeds Nyquist {nyquist_hz} Hz")]
    FrequencyOutOfRange {
        /// 请求频率。
        requested_hz: f64,
        /// 该采样率的奈奎斯特频率。
        nyquist_hz: f64,
    },

    /// 采样率为 0 或缺失。
    #[error("sample_rate must be greater than zero")]
    InvalidSampleRate,
}

/// 把错误压成单行文本，供状态栏等固定高度区域使用。
///
/// UI 层用它生成状态栏提示。这里刻意保持无依赖，便于在 UI 缺席时也能打印。
#[must_use]
pub fn describe_error(err: &dyn fmt::Display) -> String {
    let text = err.to_string();
    // 状态栏是单行的。先合并 CRLF 再替换单个控制字符，
    // 否则 "\r\n" 会变成两个空格。
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFormat;

    #[test]
    fn pcm_format_error_converts_into_auralis_error() {
        let err = AudioFormat::new(0, 2).unwrap_err();
        let wrapped: AuralisError = err.into();
        assert!(matches!(wrapped, AuralisError::Format(_)));
    }

    #[test]
    fn config_error_converts_into_auralis_error() {
        let err = ConfigError::FftSizeZero;
        let wrapped: AuralisError = err.into();
        assert_eq!(wrapped, AuralisError::Config(ConfigError::FftSizeZero));
    }

    #[test]
    fn describe_error_flattens_newlines_without_double_spacing() {
        let multi = "line one\nline two\r\nline three";
        let flat = describe_error(&multi);
        assert!(!flat.contains('\n'));
        assert!(!flat.contains('\r'));
        assert_eq!(flat, "line one line two line three");
        assert!(!flat.contains("  "), "CRLF 不应产生两个空格：{flat:?}");
    }

    #[test]
    fn insufficient_input_error_reports_both_numbers() {
        let err = AuralisError::InsufficientInput {
            needed: 4096,
            available: 100,
        };
        let text = err.to_string();
        assert!(text.contains("4096"), "{text}");
        assert!(text.contains("100"), "{text}");
    }
}
