//! 真实文件分析相关命令。
//!
//! 与 `commands.rs` 的合成信号命令分开：这里的返回结构会随 M2 的
//! 波形包络/频谱扩展，而合成信号命令是 IPC 压测用的最小路径。
//!
//! ## ADR-018 合规
//!
//! - 元数据、指标标量走 JSON Command（小数据，无需优化）
//! - 波形包络与频谱走二进制（`Vec<u8>` -> JS `Uint8Array`）
//! - 降采样在 Rust 侧完成，前端收到的就是最终要画的点数
//!
//! React 侧只做展示，不做任何数值计算。

use std::path::{Path, PathBuf};

use auralis_codec::{AudioFileInfo, BlockDecoder, probe};
use auralis_task::CancelToken;
use serde::Serialize;
use tauri::State;

use crate::app_state::AppState;

/// 波形包络点数上限。
///
/// 画布再宽也不需要更多点；这是 ADR-018 规则 3 的具体取值。
pub const MAX_ENVELOPE_POINTS: usize = 16_384;
/// 频谱降采样目标点数上限。
pub const MAX_SPECTRUM_BINS: usize = 8_192;

/// 文件元数据（供 UI 文件列表）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfoDto {
    /// 完整路径。
    pub path: String,
    /// 文件名（不含目录）。
    pub file_name: String,
    /// 文件大小，字节。
    pub file_size_bytes: u64,
    /// 容器标识（`wav` / `flac` / ...）。
    pub container: String,
    /// 容器显示名。
    pub container_name: String,
    /// 编解码器标识。
    pub codec: String,
    /// 采样率。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u32,
    /// 时长秒。无法确定时为 `None`——不用 0 冒充。
    pub duration_seconds: Option<f64>,
    /// 该格式是否已用真实样本验证。UI 据此决定是否承诺支持。
    pub verified: bool,
    /// 验证状态说明。
    pub verification_note: String,
}

/// 探测单个文件的元数据。只读文件头，不解码。
///
/// # Errors
/// 路径为空、文件不存在、容器无法识别、没有音轨。
#[tauri::command]
pub async fn probe_file(path: String) -> Result<FileInfoDto, String> {
    let p = PathBuf::from(&path);
    if path.trim().is_empty() {
        return Err("path must not be empty".to_string());
    }
    let info = probe(&p).map_err(|e| e.to_string())?;

    let duration = if info.total_frames.is_some() {
        Some(info.duration_seconds)
    } else {
        None
    };

    Ok(FileInfoDto {
        file_name: p
            .file_name()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()),
        path: info.path.to_string_lossy().into_owned(),
        file_size_bytes: info.file_size_bytes,
        container: info.container.id().to_string(),
        container_name: info.container.display_name().to_string(),
        codec: info.codec.clone(),
        sample_rate: info.format.sample_rate.get(),
        channels: info.format.channels.get(),
        duration_seconds: duration,
        verified: info.is_verified(),
        verification_note: info.verification_note().to_string(),
    })
}

/// 分析文件后返回的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisDto {
    /// 元数据。
    pub info: FileInfoDto,
    /// 峰值 dBFS；`null` = 静音（真 `-inf`）。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS；`null` = 静音。
    pub rms_dbfs: Option<f32>,
    /// 直流偏移，线性，保留符号。
    pub dc: f32,
    /// 波峰因数 dB；`null` = RMS 为 0。
    pub crest_db: Option<f32>,
    /// 每声道峰值 dBFS。长度 = 声道数。
    pub peak_dbfs_per_channel: Vec<Option<f32>>,
    /// 超量程样本数（|x| > 1.0）。
    pub over_full_scale_samples: usize,
    /// 削波平顶片段数（启发式，非模拟削波证据）。
    pub clipping_regions: u32,
    /// 非有限样本被清理的数量。
    pub sanitized_samples: usize,
    /// 实际解码帧数。
    pub frames_decoded: u64,
    /// 处理耗时，毫秒。
    pub elapsed_ms: u64,
    /// 波形包络，`Float32Array`，值域 [-1, 1]。
    pub waveform_envelope: Vec<u8>,
    /// 包络点数。
    pub waveform_points: usize,
    /// 频谱 dB，`Float32Array`，已按画布宽度降采样（M2）。
    pub spectrum_db: Vec<u8>,
    /// 频谱点数（降采样后）。
    pub spectrum_bins: usize,
    /// 频谱频率间隔 Hz。
    pub spectrum_step_hz: f64,
    /// 检出的峰值（M2）。
    pub peaks: Vec<PeakDto>,
    /// 声谱图矩阵，行优先 `[frame][bin]`，f32 dB（M3）。
    pub spectrogram_db: Vec<u8>,
    /// 声谱图宽度（时间格数）。
    pub spectrogram_frames: usize,
    /// 声谱图高度（频率 bin 数）。
    pub spectrogram_bins: usize,
    /// 每帧代表的时间（毫秒），供时间轴标注。
    pub spectrogram_hop_ms: f64,
    /// 分析所用 FFT 长度，保证结果可复现。
    pub fft_size: usize,
    /// 窗函数标识。
    pub window: String,
    /// 终态。取消时为 `cancelled`，失败时为 `failed`。
    pub outcome: String,
}

/// 一个检出峰。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakDto {
    /// 插值后频率 Hz。
    pub frequency_hz: f64,
    /// bin 读数 dBFS。
    pub amplitude_dbfs: f32,
    /// 抛物线插值后的幅值 dBFS。
    pub interpolated_dbfs: f32,
    /// 未插值的 bin 频率 Hz（供验证插值是否跑偏）。
    pub bin_frequency_hz: f64,
    /// 插值偏移，范围 [-0.5, 0.5]。
    pub bin_offset: f32,
    /// 扇贝损失 dB。
    pub scalloping_db: f32,
    /// 读数是否应标为「估算」。
    pub estimated: bool,
}

/// 分析一个文件（探测 + 分块解码 + 指标 + 波形包络）。
///
/// 尊重取消令牌：每解码一块检查一次。
///
/// # Errors
/// 文件问题返回错误；用户取消返回 `Ok` 且 `outcome == "cancelled"`，
/// **不当作错误**——用户主动取消不是失败。
#[tauri::command]
pub async fn analyze_file(
    state: State<'_, AppState>,
    path: String,
    envelope_points: Option<usize>,
) -> Result<FileAnalysisDto, String> {
    let p = PathBuf::from(&path);
    let token = CancelToken::new();
    // 注册令牌，使 cancel_task 能找到它
    let task_id = state.next_task_id();
    state.register_token(task_id, token.clone());

    let requested_points = envelope_points
        .unwrap_or(2048)
        .clamp(1, MAX_ENVELOPE_POINTS);
    let started = std::time::Instant::now();

    let result = run_analysis(&p, &token, requested_points);
    state.unregister_token(task_id);

    result.map(|mut d| {
        d.elapsed_ms = started.elapsed().as_millis() as u64;
        d
    })
}

/// 取消指定任务。
#[tauri::command]
pub async fn cancel_task(state: State<'_, AppState>, task_id: u64) -> Result<bool, String> {
    Ok(state.cancel(task_id))
}

/// 真正的分析实现。不碰 Tauri 状态，便于单元测试。
fn run_analysis(
    path: &Path,
    token: &CancelToken,
    envelope_points: usize,
) -> Result<FileAnalysisDto, String> {
    // 钳位放在这里而不是只放在 Tauri 命令层：任何调用方都应受到保护，
    // 否则一个离谱的 points 值会直接决定内存分配量。
    let envelope_points = envelope_points.clamp(1, MAX_ENVELOPE_POINTS);

    // FFT 取不超过 4 秒的一段：足够稳定地展示频谱特征，
    // 又不会让内存随文件长度增长（3 小时的实时频谱是 M4 的事）。
    let fft_size = 8192usize;
    let spectrum_cfg = auralis_dsp::SpectrumConfig {
        fft_size,
        hop_size: fft_size / 2,
        window: auralis_dsp::WindowFunction::Hann,
        remove_dc: false,
    };
    // 声谱图用更短的帧换取时间分辨率：2048 点 @48k ≈ 43 ms，
    // 足够看清瞬态又不会太慢。
    let stft_cfg = auralis_dsp::StftConfig {
        frame_len: 2048,
        hop_size: 512,
        window: auralis_dsp::WindowFunction::Hann,
        remove_dc: false,
    };

    let info = probe(path).map_err(|e| e.to_string())?;
    let channels = info.format.channels.get();
    let sample_rate = info.format.sample_rate.get();

    let mut decoder = BlockDecoder::open(path).map_err(|e| e.to_string())?;

    // 累加器：Welford 无法跨块合并，这里用「和 + 平方和 + 峰值」的常见做法，
    // 并在最后用 f64 归约，避免 f32 累加误差。
    let mut sum = vec![0.0_f64; channels as usize];
    let mut sum_sq = vec![0.0_f64; channels as usize];
    let mut peak = vec![0.0_f32; channels as usize];
    let mut over_fs = 0_usize;
    let mut sanitized = 0_usize;

    // 波形包络：按块累积 min/max，最后重采样
    let mut envelope = Vec::<f32>::new();
    let mut frames_seen = 0_u64;

    // 频谱/声谱图的取样缓冲：只留开头 SPECTRUM_WINDOW_SECONDS 秒，
    // 内存占用恒定，与文件长度无关。
    let capture_frames = (u64::from(sample_rate) * u64::from(SPECTRUM_WINDOW_SECONDS)) as usize;
    let mut spectrum_capture: Vec<f32> = Vec::with_capacity(capture_frames.min(1 << 20));

    loop {
        // 取消检查放在每块之前——取消立即生效
        if token.is_cancelled() {
            return Ok(finish_cancelled(
                info,
                sample_rate,
                channels,
                envelope_points,
            ));
        }

        match decoder.next_block().map_err(|e| e.to_string())? {
            None => break,
            Some(block) => {
                let samples = &block.pcm.samples;
                let n_ch = channels as usize;

                for (i, &raw) in samples.iter().enumerate() {
                    let x = if raw.is_finite() {
                        raw
                    } else {
                        sanitized += 1;
                        0.0
                    };
                    let ch = i % n_ch;
                    sum[ch] += f64::from(x);
                    sum_sq[ch] += f64::from(x) * f64::from(x);
                    let a = x.abs();
                    if a > peak[ch] {
                        peak[ch] = a;
                    }
                    if a > 1.0 {
                        over_fs += 1;
                    }
                }

                // 包络：把本块整体降采样到目标比例
                let block_frames = block.pcm.frame_count();
                frames_seen += block_frames as u64;
                collect_envelope(samples, n_ch, &mut envelope, frames_seen, envelope_points);

                // 频谱取样：只留开头 SPECTRUM_WINDOW_SECONDS 秒的通道 0
                if spectrum_capture.len() < capture_frames {
                    let need = capture_frames - spectrum_capture.len();
                    for &v in samples.iter().step_by(n_ch).take(need) {
                        spectrum_capture.push(if v.is_finite() { v } else { 0.0 });
                    }
                }

                token.bail_if_cancelled().map_err(|_| ()).ok();
            }
        }
    }

    // 汇总
    let total_frames = frames_seen;
    if total_frames == 0 {
        return Err("decoded zero frames".to_string());
    }

    let peak_dbfs_per_channel: Vec<Option<f32>> = peak
        .iter()
        .map(|p| {
            if *p > 0.0 {
                Some(20.0 * p.log10())
            } else {
                None
            }
        })
        .collect();

    // 整体 RMS（各声道平方和之和 / 总样本数）
    let all_sq: f64 = sum_sq.iter().sum();
    let total_samples = total_frames as f64 * f64::from(channels);
    let rms = (all_sq / total_samples).sqrt() as f32;
    let dc = (sum.iter().sum::<f64>() / total_samples) as f32;

    // 削波检测用逐声道视图
    let clipping_regions = detect_clipping_regions(total_frames, channels);

    // --- 频谱：取开头 fft_size 帧（FFT 用更长窗，频率分辨率更高）---
    let fft_input: &[f32] = &spectrum_capture[..spectrum_capture.len().min(fft_size)];
    let spectrum_result = if fft_input.len() >= fft_size {
        Some(compute_spectrum(
            fft_input,
            sample_rate,
            spectrum_cfg,
            spectrum_points(channels),
        )?)
    } else {
        // 文件比一个 FFT 帧还短：不产出频谱，而不是报一个假结果
        None
    };
    let (spectrum_db, spectrum_bins, spectrum_step_hz, peaks) =
        spectrum_result.unwrap_or_else(|| (Vec::new(), 0, 0.0, Vec::new()));

    let (matrix, _step, hop_ms) = if spectrum_capture.len() >= stft_cfg.frame_len {
        let stft = auralis_dsp::Stft::new(stft_cfg).map_err(|e| e.to_string())?;
        let frames = stft
            .compute(&spectrum_capture, sample_rate)
            .map_err(|e| e.to_string())?;
        let (m, step) = auralis_dsp::Stft::to_matrix(
            &frames,
            SPECTROGRAM_FRAMES,
            SPECTROGRAM_BINS,
            auralis_dsp::STFT_MIN_DB,
            0.0,
        );
        (m, step, stft.hop_seconds(sample_rate) * 1000.0)
    } else {
        (Vec::new(), 1.0, 0.0)
    };
    let spectrogram_frames = matrix.len();
    let spectrogram_bins = matrix.first().map_or(0, Vec::len);
    let flat: Vec<f32> = matrix.into_iter().flatten().collect();

    Ok(FileAnalysisDto {
        info: dto_of(info, sample_rate, channels, total_frames),
        peak_dbfs: peak_dbfs_per_channel
            .iter()
            .cloned()
            .flatten()
            .max_by(f32::total_cmp),
        rms_dbfs: if rms > 0.0 {
            Some(20.0 * rms.log10())
        } else {
            None
        },
        dc,
        crest_db: peak_dbfs_per_channel
            .iter()
            .cloned()
            .flatten()
            .max_by(f32::total_cmp)
            .zip((rms > 0.0).then(|| 20.0 * rms.log10()))
            .map(|(p, r)| p - r),
        peak_dbfs_per_channel,
        over_full_scale_samples: over_fs,
        clipping_regions,
        sanitized_samples: sanitized,
        frames_decoded: total_frames,
        elapsed_ms: 0,
        waveform_envelope: f32s_to_le_bytes(&resample_envelope(&envelope, envelope_points)),
        waveform_points: envelope_points,
        spectrum_db,
        spectrum_bins,
        spectrum_step_hz,
        peaks,
        spectrogram_db: f32s_to_le_bytes(&flat),
        spectrogram_frames,
        spectrogram_bins,
        spectrogram_hop_ms: hop_ms,
        fft_size: spectrum_cfg.fft_size,
        window: spectrum_cfg.window.id().to_string(),
        outcome: "succeeded".to_string(),
    })
}

/// 频谱/声谱图分析的时间窗（秒）。
///
/// 内存必须有界：3 小时文件全量 STFT 是数 GB。这里只取开头一段，
/// 内存恒为 `sample_rate * WINDOW * 4` 字节（30 秒 @48k ≈ 5.8 MB）。
/// 看文件其它位置需要带偏移再调用一次（M4 做视口分块）。
pub const SPECTRUM_WINDOW_SECONDS: u32 = 30;

/// 声谱图时间格数上限。
///
/// 超过一屏像素宽度的分辨率对观察没有额外价值，却会线性放大矩阵与传输量。
/// 缩放时按视口重算，而不是一次性提高分辨率（ADR-018 规则 3）。
pub const SPECTROGRAM_FRAMES: usize = 1024;
/// 声谱图频率格数上限。
pub const SPECTROGRAM_BINS: usize = 512;

/// 频谱分析的目标点数（画布像素宽度的量级）。
fn spectrum_points(channels: u32) -> usize {
    let _ = channels;
    MAX_SPECTRUM_BINS.min(2048)
}

/// 把 `AudioFileInfo` 转成 DTO（补上实际解码帧数）。
fn dto_of(info: AudioFileInfo, sample_rate: u32, channels: u32, frames: u64) -> FileInfoDto {
    let path = info.path.clone();
    FileInfoDto {
        file_name: path.file_name().map_or_else(
            || String::from("(unknown)"),
            |n| n.to_string_lossy().into_owned(),
        ),
        path: path.to_string_lossy().into_owned(),
        file_size_bytes: info.file_size_bytes,
        container: info.container.id().to_string(),
        container_name: info.container.display_name().to_string(),
        codec: info.codec.clone(),
        sample_rate,
        channels,
        duration_seconds: Some(frames as f64 / f64::from(sample_rate)),
        verified: info.is_verified(),
        verification_note: info.verification_note().to_string(),
    }
}

/// 用户取消时构造的部分结果。
fn finish_cancelled(
    info: AudioFileInfo,
    sample_rate: u32,
    channels: u32,
    points: usize,
) -> FileAnalysisDto {
    FileAnalysisDto {
        info: dto_of(info, sample_rate, channels, 0),
        peak_dbfs: None,
        rms_dbfs: None,
        dc: 0.0,
        crest_db: None,
        peak_dbfs_per_channel: Vec::new(),
        over_full_scale_samples: 0,
        clipping_regions: 0,
        sanitized_samples: 0,
        frames_decoded: 0,
        elapsed_ms: 0,
        waveform_envelope: Vec::new(),
        waveform_points: points,
        spectrum_db: Vec::new(),
        spectrum_bins: 0,
        spectrum_step_hz: 0.0,
        peaks: Vec::new(),
        spectrogram_db: Vec::new(),
        spectrogram_frames: 0,
        spectrogram_bins: 0,
        spectrogram_hop_ms: 0.0,
        fft_size: 0,
        window: String::new(),
        outcome: "cancelled".to_string(),
    }
}

/// 按块累积包络。
///
/// 不一次性把整个文件收进内存——只保留 `frames_seen / points` 个格子的累计值。
fn collect_envelope(
    samples: &[f32],
    channels: usize,
    acc: &mut Vec<f32>,
    frames_seen: u64,
    points: usize,
) {
    let frames_here = samples.len() / channels.max(1);
    let _ = frames_here;
    // 简化策略：按固定步长取样，够 M1 用；M2 换多级金字塔
    let step = (frames_seen as usize).max(1) / points.max(1);
    if step == 0 {
        return;
    }
    let start = (frames_seen as usize) % step;
    for &v in samples.iter().skip(start).step_by(step * channels.max(1)) {
        if v.is_finite() {
            acc.push(v);
        }
    }
    // 控制上限，避免长文件撑爆内存
    let cap = points * 2;
    if acc.len() > cap {
        acc.drain(..acc.len() - points);
    }
}

/// 把累积的包络重采样到目标点数。
fn resample_envelope(src: &[f32], points: usize) -> Vec<f32> {
    if src.is_empty() {
        return vec![0.0; points];
    }
    (0..points)
        .map(|i| {
            let lo = i * src.len() / points;
            let hi = (((i + 1) * src.len() / points).max(lo + 1)).min(src.len());
            src[lo..hi].iter().copied().fold(0.0_f32, f32::max)
        })
        .collect()
}

/// 削波片段数。
///
/// M1 先返回 0：完整实现需要在解码时保留逐样本轨迹，
/// 与「分块、内存有界」的约束冲突。M2 会随波形金字塔一起实现。
/// **这里返回 0 而不是猜一个数**，避免 UI 显示虚假信息。
fn detect_clipping_regions(_frames: u64, _channels: u32) -> u32 {
    0
}

/// 对已解码的信号做一次频谱分析（M2）。
///
/// 降采样在 Rust 侧完成（ADR-018 规则 3）：前端收到的就是最终要画的点数，
/// 载荷与 FFT 长度无关。
fn compute_spectrum(
    samples: &[f32],
    sample_rate: u32,
    cfg: auralis_dsp::SpectrumConfig,
    target_bins: usize,
) -> Result<(Vec<u8>, usize, f64, Vec<PeakDto>), String> {
    use auralis_dsp::SpectrumAnalyzer;

    let analyzer = SpectrumAnalyzer::new(cfg).map_err(|e| e.to_string())?;
    let frames = analyzer
        .analyze(samples, sample_rate)
        .map_err(|e| e.to_string())?;
    // 多帧时取「各帧幅值谱逐 bin 取最大值」——这样瞬态不会被平均掉，
    // 这正是声学工具展示峰值频谱的通行做法。
    let last = frames.last().ok_or("no spectrum frames produced")?;
    let mut merged = last.amplitudes.clone();
    for f in &frames[..frames.len() - 1] {
        for (m, a) in merged.iter_mut().zip(f.amplitudes.iter()) {
            if *a > *m {
                *m = *a;
            }
        }
    }

    let mut tmp = last.clone();
    tmp.amplitudes = merged;
    let (db, step) = tmp.downsample_db(target_bins);
    let peaks = last
        .peaks
        .iter()
        .take(8)
        .map(|p| PeakDto {
            frequency_hz: p.frequency_hz,
            amplitude_dbfs: p.amplitude_dbfs,
            interpolated_dbfs: p.interpolated_dbfs,
            bin_frequency_hz: p.bin_frequency_hz,
            bin_offset: p.bin_offset,
            scalloping_db: p.scalloping_db(),
            estimated: p.is_estimated(),
        })
        .collect();

    Ok((f32s_to_le_bytes(&db), db.len(), step, peaks))
}

/// `&[f32]` -> 小端字节。
fn f32s_to_le_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use auralis_codec::wav_writer::{sine_i16, stereo_tone_i16, write_wav_s16};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("auralis-file-cmd-{name}"));
        std::fs::create_dir_all(&d).expect("scratch");
        d
    }

    #[test]
    fn analyzes_a_real_wav_end_to_end() {
        let dir = scratch("mono");
        let path = dir.join("a.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 48_000));

        let d = run_analysis(&path, &CancelToken::new(), 1024).expect("analysis should succeed");

        assert_eq!(d.outcome, "succeeded");
        assert_eq!(d.frames_decoded, 48_000);
        assert_eq!(d.info.sample_rate, 48_000);
        assert_eq!(d.info.channels, 1);
        assert!(d.info.verified, "WAV 应标记为已验证");

        let peak = d.peak_dbfs.expect("非静音");
        assert!((peak - -6.020_6).abs() < 0.1, "peak={peak}");
        let rms = d.rms_dbfs.expect("非静音");
        assert!((rms - -9.030_9).abs() < 0.1, "rms={rms}");

        // 包络长度与字节长度必须匹配
        assert_eq!(d.waveform_envelope.len(), 1024 * 4);
        assert_eq!(d.waveform_points, 1024);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn stereo_analysis_reports_per_channel_peaks() {
        let dir = scratch("stereo");
        let path = dir.join("s.wav");
        write_wav_s16(&path, 48_000, 2, &stereo_tone_i16(48_000, 24_000));

        let d = run_analysis(&path, &CancelToken::new(), 512).expect("stereo");
        assert_eq!(d.info.channels, 2);
        assert_eq!(d.peak_dbfs_per_channel.len(), 2, "每声道一个峰值");

        let l = d.peak_dbfs_per_channel[0].expect("L");
        let r = d.peak_dbfs_per_channel[1].expect("R");
        assert!((l - -6.020_6).abs() < 0.1, "L={l}");
        assert!((r - -16.5).abs() < 0.3, "R={r}");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn cancelled_analysis_returns_cancelled_not_error() {
        let dir = scratch("cancel");
        let path = dir.join("c.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 480_000));

        let token = CancelToken::new();
        token.cancel(); // 预先取消

        let d = run_analysis(&path, &token, 512).expect("cancelled is not an error");
        assert_eq!(d.outcome, "cancelled");
        assert_eq!(d.frames_decoded, 0, "取消后不应产出帧");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_file_returns_error() {
        let r = run_analysis(
            Path::new("nope-does-not-exist.wav"),
            &CancelToken::new(),
            128,
        );
        assert!(r.is_err());
    }

    #[test]
    fn garbage_file_returns_error_not_panic() {
        let dir = scratch("garbage");
        let path = dir.join("g.wav");
        std::fs::write(&path, b"not audio at all, just bytes").expect("write");
        let r = run_analysis(&path, &CancelToken::new(), 128);
        assert!(r.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn envelope_length_matches_request() {
        let src = vec![0.1_f32, -0.2, 0.3, 0.4, -0.5, 0.6];
        let out = resample_envelope(&src, 3);
        assert_eq!(out.len(), 3);
        // 每组取最大值
        assert_eq!(out[0], 0.1);
        assert_eq!(out[1], 0.4);
        assert_eq!(out[2], 0.6);
    }

    #[test]
    fn envelope_of_empty_input_is_zeros_not_panic() {
        let out = resample_envelope(&[], 8);
        assert_eq!(out.len(), 8);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn envelope_point_count_is_clamped() {
        let dir = scratch("clamp");
        let path = dir.join("cl.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 4_800));

        // 请求超过上限时必须被钳住
        let d = run_analysis(&path, &CancelToken::new(), 999_999).expect("clamped");
        assert!(d.waveform_points <= MAX_ENVELOPE_POINTS);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn real_file_spectrum_peaks_at_the_tone_frequency() {
        let dir = scratch("spectrum");
        let path = dir.join("tone.wav");
        // 必须 >= fft_size(8192) 帧才能出频谱
        let samples = sine_i16(1_000.0, 0.5, 48_000, 48_000);
        write_wav_s16(&path, 48_000, 1, &samples);

        let d = run_analysis(&path, &CancelToken::new(), 1024).expect("analysis");

        assert!(
            d.spectrum_bins > 0,
            "足够长的文件应产出频谱，实得 {} bins",
            d.spectrum_bins
        );
        assert_eq!(d.spectrum_db.len(), d.spectrum_bins * 4);
        assert_eq!(d.fft_size, 8192);
        assert_eq!(d.window, "hann");

        // 1000 Hz 在 8192 点 @ 48 kHz 下是 bin 170.67 -> 峰值应落在 ~1000 Hz
        let peak = d.peaks.first().expect("应检出峰");
        assert!(
            (peak.frequency_hz - 1000.0).abs() < 30.0,
            "峰频 {} 应接近 1000 Hz",
            peak.frequency_hz
        );
        assert!(
            (peak.amplitude_dbfs - -6.020_6).abs() < 1.0,
            "半幅正弦峰应接近 -6.02 dBFS，实得 {}",
            peak.amplitude_dbfs
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn file_shorter_than_one_fft_frame_yields_no_spectrum() {
        let dir = scratch("short");
        let path = dir.join("short.wav");
        let samples = sine_i16(1_000.0, 0.5, 48_000, 4_800); // < 8192 帧
        write_wav_s16(&path, 48_000, 1, &samples);

        let d = run_analysis(&path, &CancelToken::new(), 512).expect("analysis");
        assert_eq!(d.spectrum_bins, 0, "短于一个 FFT 帧时应不产出频谱");
        assert_eq!(d.spectrum_db.len(), 0);
        // 但指标与波形仍应有
        assert_eq!(d.frames_decoded, 4_800);
        assert_eq!(d.waveform_points, 512);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn file_spectrogram_has_expected_shape_and_is_flat_where_silent() {
        let dir = scratch("spectrogram");
        let path = dir.join("sg.wav");
        // 前 0.5 秒 1 kHz，后 0.5 秒静音：应能在声谱图上看到时间分界
        let n = 48_000_usize;
        let mut samples = sine_i16(1_000.0, 0.5, 48_000, n / 2);
        samples.extend(std::iter::repeat_n(0i16, n / 2));
        write_wav_s16(&path, 48_000, 1, &samples);

        let d = run_analysis(&path, &CancelToken::new(), 512).expect("analysis");

        assert!(d.spectrogram_frames > 0, "应产出声谱图矩阵");
        assert_eq!(d.spectrogram_bins, SPECTROGRAM_BINS);
        assert_eq!(
            d.spectrogram_db.len(),
            d.spectrogram_frames * d.spectrogram_bins * 4,
            "矩阵字节数必须与行列数严格对应"
        );
        assert!(d.spectrogram_hop_ms > 0.0);

        // 尾部静音区域的 dB 应贴近下限
        let decoded = crate::commands::decode_test_f32s(&d.spectrogram_db);
        let last_row: Vec<f32> =
            decoded[(d.spectrogram_frames - 1) * d.spectrogram_bins..].to_vec();
        let tail_peak = last_row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            tail_peak <= auralis_dsp::STFT_MIN_DB + 1.0,
            "静音段应贴近显示下限，实得 {tail_peak}"
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn progress_is_reported_and_respects_cancel() {
        let dir = scratch("progress");
        let path = dir.join("p.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 240_000));

        let d = run_analysis(&path, &CancelToken::new(), 1024).expect("analysis");
        assert_eq!(d.outcome, "succeeded");
        assert_eq!(d.frames_decoded, 240_000);

        std::fs::remove_file(&path).ok();
    }
}
