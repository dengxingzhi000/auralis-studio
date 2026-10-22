//! 解码错误。
//!
//! `docs/TAD.md` §7 要求区分「不支持格式 / 文件损坏或截断 / 输入不足」等，
//! 因此这里用类型化枚举而不是字符串。

use std::path::PathBuf;

/// 解码相关错误。
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// 文件不存在。
    #[error("file not found: {0}")]
    NotFound(PathBuf),

    /// 无法读取文件（权限、被占用等）。
    #[error("cannot read {path}: {source}")]
    Io {
        /// 文件路径。
        path: PathBuf,
        /// 底层 I/O 错误。
        #[source]
        source: std::io::Error,
    },

    /// PCM 块构造失败（样本数与声道数不匹配等）。
    #[error("pcm block invalid: {0}")]
    Pcm(#[from] auralis_core::PcmFormatError),

    /// 无法识别容器格式——**不是**「不支持的编码」，
    /// 而是连文件头都读不出来。
    #[error("unrecognised container format (is this an audio file?)")]
    UnrecognisedFormat,

    /// 容器可识别，但没有可用的音轨。
    #[error("no audio track found in {path}")]
    NoAudioTrack {
        /// 文件路径。
        path: PathBuf,
    },

    /// 容器或编码已启用，但 symphonia 不认识这个 codec。
    #[error("unsupported codec {0:?} (the symphonia feature for it is probably not enabled)")]
    UnsupportedCodec(String),

    /// 文件损坏或被截断。
    #[error("corrupt or truncated file: {0}")]
    Corrupt(String),

    /// 文件被截断：容器头声明的时长/长度大于实际可解码的数据。
    ///
    /// 与 `Corrupt` 分开是因为**处置方式不同**：截断通常是文件没下完或
    /// 拷贝不完整，重新获取即可；损坏才意味着要检查编码器/容器兼容性。
    #[error("truncated file: {0}")]
    Truncated(String),

    /// 采样率或声道数为 0，解码结果无意义。
    #[error("decoded audio has invalid parameters: {0}")]
    InvalidParameters(String),
}

/// 本 crate 的 `Result` 别名。
pub type Result<T> = std::result::Result<T, CodecError>;

impl CodecError {
    /// 把 symphonia 的错误映射成本枚举。
    ///
    /// 映射依据是可观察行为而非错误文本匹配：
    /// - `IoError` 多为读取时底层流出错（文件被截断/占用）
    /// - `DecodeError` 在包边界处出现多为数据损坏
    /// - `Unsupported` 几乎总是「symphonia feature 没开」
    ///
    /// 其余归为 `Corrupt`，并**始终带上底层描述**，
    /// 避免用户只看到一个无信息量的「不支持」。
    pub(crate) fn from_symphonia(err: &symphonia::core::errors::Error) -> Self {
        use symphonia::core::errors::Error as E;
        match err {
            // 读到文件尾但数据还没够 —— 这是「文件被截断」的典型特征，
            // 单独归类比笼统的「损坏」有用得多。
            E::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Self::Truncated(e.to_string())
            }
            E::IoError(e) => Self::Corrupt(format!("I/O while decoding: {e}")),
            E::DecodeError(s) => Self::Corrupt(s.to_string()),
            E::SeekError(_) | E::ResetRequired => {
                Self::Corrupt("stream position invalid (seek or reset required)".to_string())
            }
            E::Unsupported(codec) => {
                Self::UnsupportedCodec(format!("{codec:?} (is its symphonia feature enabled?)"))
            }
            E::LimitError(s) => Self::Corrupt(format!("limit reached: {s}")),
            other => Self::Corrupt(format!("{other}")),
        }
    }
}

/// 把 symphonia 的 `Result` 转换为本 crate 的 `Result`。
pub(crate) fn map_symphonia<T>(
    r: std::result::Result<T, symphonia::core::errors::Error>,
) -> Result<T> {
    r.map_err(|e| CodecError::from_symphonia(&e))
}

#[cfg(test)]
mod tests {
    use super::CodecError;

    /// 造一个 symphonia 的 I/O 错误。
    fn io_error(kind: std::io::ErrorKind, message: &str) -> symphonia::core::errors::Error {
        symphonia::core::errors::Error::IoError(std::io::Error::new(kind, message))
    }

    #[test]
    fn unexpected_eof_is_classified_as_truncated_not_corrupt() {
        let mapped = CodecError::from_symphonia(&io_error(
            std::io::ErrorKind::UnexpectedEof,
            "unexpected end of file",
        ));

        assert!(
            matches!(mapped, CodecError::Truncated(_)),
            "读到文件尾应归类为截断，实得: {mapped}"
        );
        assert!(
            mapped.to_string().starts_with("truncated file"),
            "消息应以「truncated file」开头，实得: {mapped}"
        );
    }

    #[test]
    fn other_io_errors_stay_corrupt() {
        let mapped =
            CodecError::from_symphonia(&io_error(std::io::ErrorKind::PermissionDenied, "denied"));

        assert!(
            matches!(mapped, CodecError::Corrupt(_)),
            "非 EOF 的 I/O 错误仍属损坏，实得: {mapped}"
        );
    }
}
