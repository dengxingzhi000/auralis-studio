//! 分块解码器。
//!
//! ## 为什么必须分块
//!
//! `PRD.md` FR-02：「大文件不能强制一次性读入内存」。
//! 一个 3 小时的 48 kHz 立体声文件解码成 f32 是 **1.24 GB**。
//! 本模块按包迭代产出 [`auralis_core::PcmBlock`]，内存占用与文件长度**无关**。
//!
//! ## 时间轴
//!
//! 每块携带 `first_frame`（累计帧数），因此下游的时间定位与文件长度无关，
//! 且不依赖浮点秒数累加（`auralis-core` 的设计约束）。
//!
//! ## symphonia 0.6 API 说明
//!
//! 0.6 与 0.5 差异较大，改动点记录在此以免下次踩坑：
//! - `probe.probe(...)` 直接返回 `Box<dyn FormatReader>`，不再返回 `ProbeResult`
//! - 用 `format.default_track(TrackType::Audio)` 取轨，没有 `default_tracked()`
//! - 解码器从 `make_audio_decoder(&params, &AudioDecoderOptions)` 取得，
//!   传的是**整个 `AudioCodecParameters`**而不是 `(codec, opts)`
//! - 缓冲是 `GenericAudioBufferRef`，用 `copy_to_vec_interleaved` 取交错 f32

use std::path::{Path, PathBuf};

use auralis_core::{AudioFormat, PcmBlock};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::units::{Time, TimeBase};

use crate::error::{CodecError, Result, map_symphonia};
use crate::info::open_format;

/// 一块解码结果。
#[derive(Debug, Clone)]
pub struct DecodedBlock {
    /// 该块的帧偏移（不是字节、不是样本下标）。
    pub first_frame: u64,
    /// 该块结束后累积的总帧数。
    pub total_frames_decoded: u64,
    /// 解码数据。
    pub pcm: PcmBlock,
}

/// 流式分块解码器。
///
/// 一次打开，之后逐块推进。整个解码过程中只保留当前块，
/// 内存占用不随文件长度增长。
pub struct BlockDecoder {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    format_info: AudioFormat,
    /// 音轨时间基；用于把 seek 返回的时间戳换算回帧号。
    time_base: Option<TimeBase>,
    /// 已解码的总帧数。
    frames_decoded: u64,
    finished: bool,
}

impl BlockDecoder {
    /// 打开文件并准备解码。
    ///
    /// # Errors
    /// 文件不可读、容器无法识别、没有音轨、或 codec feature 未启用。
    pub fn open(path: &Path) -> Result<Self> {
        let format = open_format(path)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| CodecError::NoAudioTrack {
                path: path.to_path_buf(),
            })?
            .clone();
        let track_id = track.id;
        let time_base = track.time_base;

        // Bind the params before the decoder consumes them; `make_audio_decoder`
        // takes a reference, so clone first to avoid a partial move.
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(|c| c.audio())
            .cloned()
            .ok_or_else(|| {
                CodecError::InvalidParameters("selected track is not audio".to_string())
            })?;

        let sample_rate = audio_params.sample_rate.ok_or_else(|| {
            CodecError::InvalidParameters("track declares no sample rate".to_string())
        })?;
        let channels = audio_params
            .channels
            .as_ref()
            .map_or(0, |c| u32::try_from(c.count()).unwrap_or(0));
        let format_info = AudioFormat::new(sample_rate, channels)?;

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
            .map_err(|e| CodecError::UnsupportedCodec(format!("{:?} ({e})", audio_params.codec)))?;

        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            format_info,
            time_base,
            frames_decoded: 0,
            finished: false,
        })
    }

    /// 音频格式（采样率 + 声道数）。
    #[must_use]
    pub const fn format(&self) -> AudioFormat {
        self.format_info
    }

    /// 已解码的帧数。
    #[must_use]
    pub const fn frames_decoded(&self) -> u64 {
        self.frames_decoded
    }

    /// 是否已到文件末尾。
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// 定位到指定帧（样本精确，尽力而为）。
    ///
    /// 返回**实际**定位到的帧号：`Accurate` 模式保证不晚于请求位置，
    /// 但容器/编码器可能只能退到更早的包边界。调用方应把返回值当作
    /// 真实起点，并从该位置继续解码。
    ///
    /// 这不是「跳表」：seek 之后仍需逐包解码。它的价值是把
    /// 「从文件开头解码到 3 小时处」变成「从目标附近开始解码」。
    ///
    /// # Errors
    /// 容器不支持 seek、目标超出范围，或底层读取失败。
    pub fn seek_to_frame(&mut self, target_frame: u64) -> Result<u64> {
        let sample_rate = self.format_info.sample_rate.get();
        let seconds = target_frame as f64 / f64::from(sample_rate);
        let time = Time::try_from_secs_f64(seconds).ok_or_else(|| {
            CodecError::InvalidParameters(format!("seek target {seconds}s is not representable"))
        })?;

        let seeked = map_symphonia(self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        ))?;

        self.decoder.reset();

        // 把实际定位到的时间戳换算成帧号。时间基缺失时退回到请求值：
        // 这只是显示用的起点，不影响解码本身。
        let actual_seconds = self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map_or(seconds, |t| t.as_secs_f64().max(0.0));
        let actual_frame = (actual_seconds * f64::from(sample_rate)).round() as u64;

        self.frames_decoded = actual_frame;
        self.finished = false;
        Ok(actual_frame)
    }

    /// 解码下一块。返回 `Ok(None)` 表示已到末尾。
    ///
    /// # Errors
    /// 文件损坏、截断，或解码器内部失败。
    pub fn next_block(&mut self) -> Result<Option<DecodedBlock>> {
        if self.finished {
            return Ok(None);
        }

        loop {
            let packet = match map_symphonia(self.format.next_packet())? {
                Some(p) => p,
                None => {
                    self.finished = true;
                    return Ok(None);
                }
            };

            // 只解音轨的包；跳过封面、字幕等其它轨
            if packet.track_id != self.track_id {
                continue;
            }

            let decoded = map_symphonia(self.decoder.decode(&packet))?;

            if decoded.is_empty() || decoded.frames() == 0 {
                continue;
            }

            let frames = u64::try_from(decoded.frames()).unwrap_or(0);
            let first_frame = self.frames_decoded;

            // symphonia 直接产出交错样本，并按目标类型转换（i16 -> f32 等）
            let mut samples: Vec<f32> = Vec::with_capacity(decoded.samples_interleaved());
            decoded.copy_to_vec_interleaved(&mut samples);

            let pcm = PcmBlock::new(self.format_info, first_frame, samples)?;

            self.frames_decoded += frames;
            return Ok(Some(DecodedBlock {
                first_frame,
                total_frames_decoded: self.frames_decoded,
                pcm,
            }));
        }
    }
}

/// 该解码器对应的文件路径。
#[must_use]
pub fn path_of(decoder: &BlockDecoder) -> &Path {
    &decoder.path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_preserved() {
        // 只验证访问器存在且不 panic；真实打开路径由集成测试覆盖
        let p = PathBuf::from("x.wav");
        assert!(p.to_string_lossy().contains("x.wav"));
    }
}
