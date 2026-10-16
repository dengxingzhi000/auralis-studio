//! 文件元数据探测。
//!
//! **只读文件头，不解码音频数据**。因此对大文件也是 O(1) 级开销，
//! UI 打开文件列表时可以对整个目录快速探测。
//!
//! symphonia 0.6 的 API 差异见 `reader.rs` 的模块文档。

use std::fs::File;
use std::num::NonZeroU32;
use std::path::Path;

use auralis_core::{AudioFormat, ChannelLayout};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, Track, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::error::{CodecError, Result, map_symphonia};

/// 容器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// WAV / RIFF。
    Wav,
    /// FLAC。
    Flac,
    /// MPEG Layer III。
    Mp3,
    /// AIFF。
    Aiff,
    /// Ogg（Vorbis/Opus）。
    Ogg,
    /// Matroska / WebM。
    Mkv,
    /// ISO Base Media（MP4/M4A）。
    IsoMp4,
    /// CAF（Apple）。
    Caf,
    /// 已识别但本项目未枚举的类型。
    Other,
}

impl ContainerKind {
    /// 稳定的字符串标识，用于 IPC 与 UI 显示。
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Aiff => "aiff",
            Self::Ogg => "ogg",
            Self::Mkv => "mkv",
            Self::IsoMp4 => "isomp4",
            Self::Caf => "caf",
            Self::Other => "other",
        }
    }

    /// 用户可读名称。
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
            Self::Aiff => "AIFF",
            Self::Ogg => "OGG",
            Self::Mkv => "MKV",
            Self::IsoMp4 => "MP4/M4A",
            Self::Caf => "CAF",
            Self::Other => "未知容器",
        }
    }
}

/// 音频文件元数据。
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub struct AudioFileInfo {
    /// 文件路径。
    pub path: std::path::PathBuf,
    /// 文件大小，字节。
    pub file_size_bytes: u64,
    /// 容器类型。
    pub container: ContainerKind,
    /// 编解码器标识（如 `pcm_s16le`、`flac`、`mp3`）。
    pub codec: String,
    /// 音频格式（采样率 + 声道数）。
    pub format: AudioFormat,
    /// 声道布局描述。
    pub channel_layout: ChannelLayout,
    /// 位深。解码器未提供时为 `None`——**不猜**。
    pub bits_per_sample: Option<u32>,
    /// 总帧数。流式格式可能不预知长度，此时为 `None`。
    pub total_frames: Option<u64>,
    /// 时长，秒。由 `total_frames / sample_rate` 得出，
    /// 因此**不会**因累加浮点秒数而漂移。
    pub duration_seconds: f64,
}

impl AudioFileInfo {
    /// 该容器是否已用真实样本验证过。
    ///
    /// UI 应当据此决定是否承诺支持 —— 见 crate 文档的「格式支持矩阵」。
    #[must_use]
    pub fn is_verified(&self) -> bool {
        verified_containers().contains(&self.container)
    }

    /// 验证状态的可读说明，供 UI 直接显示。
    #[must_use]
    pub fn verification_note(&self) -> &'static str {
        if self.is_verified() {
            "已用真实样本验证"
        } else {
            "可解码，但尚未用真实样本验证"
        }
    }
}

/// **已通过真实样本验证**的容器。
///
/// 这是 UI 的硬约束：只有这里列出的容器才允许在界面上宣称支持。
/// 新增条目的前提是 `TEST_PLAN.md` §3 的样本测试已补齐。
#[must_use]
pub const fn verified_containers() -> &'static [ContainerKind] {
    &[ContainerKind::Wav]
}

/// 打开文件并返回格式读取器。供 `reader` 与本模块共用。
pub(crate) fn open_format(path: &Path) -> Result<Box<dyn FormatReader>> {
    let file = File::open(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            CodecError::NotFound(path.to_path_buf())
        } else {
            CodecError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;

    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    }

    map_symphonia(symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ))
    .map_err(|e| match e {
        // symphonia 没有专门的「无法识别容器」错误，它把探测失败也报成
        // Unsupported("core (probe): no suitable format reader found")。
        // 若原样透传，用户会看到「是不是没开 feature？」这种误导性提示，
        // 而实际问题是他选的根本不是音频文件。
        CodecError::UnsupportedCodec(msg)
            if msg.contains("probe") || msg.contains("no suitable") =>
        {
            CodecError::UnrecognisedFormat
        }
        other => other,
    })
}

/// 探测文件元数据。只读文件头，不解码。
///
/// # Errors
/// 文件不存在或不可读、容器无法识别、没有音轨、采样率或声道数为 0。
pub fn probe(path: &Path) -> Result<AudioFileInfo> {
    let meta = std::fs::metadata(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            CodecError::NotFound(path.to_path_buf())
        } else {
            CodecError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;

    let format = open_format(path)?;
    let container = classify_container(format.format_info().short_name);

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| CodecError::NoAudioTrack {
            path: path.to_path_buf(),
        })?
        .clone();

    // `AudioCodecParameters::frames_per_block` **不是**总帧数——它是解码器的
    // 缓冲粒度（WAV PCM 通常是 1）。误用它会得到完全错误的时长。
    // 正确的来源是 MediaInfo 的 duration + time_base。
    let total_frames = media_frames(format.as_ref(), &track);

    build_info(path, meta.len(), container, &track, total_frames)
}

/// 从 `MediaInfo` 推导总帧数。
///
/// 刻意**不**使用 `AudioCodecParameters::frames_per_block`——那是解码器的
/// 缓冲粒度（WAV PCM 通常为 1），把它当总帧数会得到完全错误的时长。
/// 这里只认 `duration + time_base(1/sample_rate)` 这一种可换算的形式，
/// 其余情况返回 `None` 而不猜测。
fn media_frames(format: &dyn FormatReader, track: &Track) -> Option<u64> {
    let media = format.media_info();
    let sr = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .and_then(|a| a.sample_rate)?;

    match (media.duration, media.time_base) {
        (Some(duration), Some(tb)) if tb.numer.get() == 1 && tb.denom.get() == sr => {
            Some(duration.get())
        }
        _ => None,
    }
}

/// 从 symphonia 的格式短名推断容器类型。
///
/// 刻意用**内容**而不是扩展名：扩展名可以撒谎，内容不会。
fn classify_container(short_name: &str) -> ContainerKind {
    let s = short_name.to_ascii_uppercase();
    if s.contains("WAV") || s.contains("RIFF") {
        ContainerKind::Wav
    } else if s.contains("FLAC") {
        ContainerKind::Flac
    } else if s.contains("MPEG") || s.contains("MP3") {
        ContainerKind::Mp3
    } else if s.contains("AIFF") || s.contains("AIFC") {
        ContainerKind::Aiff
    } else if s.contains("OGG") {
        ContainerKind::Ogg
    } else if s.contains("MATROSKA") || s.contains("WEBM") {
        ContainerKind::Mkv
    } else if s.contains("MP4") || s.contains("ISO") {
        ContainerKind::IsoMp4
    } else if s.contains("CAF") {
        ContainerKind::Caf
    } else {
        ContainerKind::Other
    }
}

/// 从轨道参数构造 [`AudioFileInfo`]。
///
/// `total_frames` 由调用方从 `MediaInfo` 推导后传入 —— 它不在 `AudioCodecParameters`
/// 里（`frames_per_block` 是缓冲粒度，不是总帧数）。
fn build_info(
    path: &Path,
    file_size: u64,
    container: ContainerKind,
    track: &Track,
    total_frames: Option<u64>,
) -> Result<AudioFileInfo> {
    let audio = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .ok_or_else(|| CodecError::InvalidParameters("selected track is not audio".to_string()))?;

    let sample_rate = audio.sample_rate.ok_or_else(|| {
        CodecError::InvalidParameters("track declares no sample rate".to_string())
    })?;
    let channels = u32::try_from(audio.channels.as_ref().map_or(0, |c| c.count())).unwrap_or(0);

    let sr = NonZeroU32::new(sample_rate)
        .ok_or_else(|| CodecError::InvalidParameters("sample rate is 0".to_string()))?;
    let ch = NonZeroU32::new(channels)
        .ok_or_else(|| CodecError::InvalidParameters("channel count is 0".to_string()))?;

    let duration_seconds = total_frames.map_or(0.0, |f| f as f64 / f64::from(sr.get()));

    Ok(AudioFileInfo {
        path: path.to_path_buf(),
        file_size_bytes: file_size,
        container,
        codec: format!("{:?}", audio.codec).to_ascii_lowercase(),
        format: AudioFormat {
            sample_rate: sr,
            channels: ch,
        },
        channel_layout: ChannelLayout::from_channel_count(channels),
        bits_per_sample: audio.bits_per_sample,
        total_frames,
        duration_seconds,
    })
}
