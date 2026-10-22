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
use auralis_storage::{
    AnalysisExport, AnalysisParameters, ExportFormat, ExportMetadata, ExportMetrics, ExportPeak,
};
use auralis_task::CancelToken;
use serde::Serialize;

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
pub fn probe_file(path: String) -> Result<FileInfoDto, String> {
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
    /// 文件是否被截断：数据流在解完之前就结束了。
    ///
    /// 截断的文件**仍然给出完整结果**——只是结果只覆盖了文件里
    /// 实际存在的那一部分。`declared_duration_seconds` 与
    /// `decoded_duration_seconds` 的差就是缺了多少。
    pub truncated: bool,
    /// 文件头声明的时长（秒）。容器未声明时为 0。
    pub declared_duration_seconds: f64,
    /// 实际解码出的时长（秒）。
    pub decoded_duration_seconds: f64,
    /// 处理耗时，毫秒。
    pub elapsed_ms: u64,
    /// 波形包络，`Float32Array`，值域 [-1, 1]。
    #[serde(serialize_with = "crate::b64::encode")]
    pub waveform_envelope: Vec<u8>,
    /// 包络点数。
    pub waveform_points: usize,
    /// 频谱 dB，`Float32Array`，已按画布宽度降采样（M2）。
    #[serde(serialize_with = "crate::b64::encode")]
    pub spectrum_db: Vec<u8>,
    /// 频谱点数（降采样后）。
    pub spectrum_bins: usize,
    /// 频谱频率间隔 Hz。
    pub spectrum_step_hz: f64,
    /// 检出的峰值（M2）。
    pub peaks: Vec<PeakDto>,
    /// 声谱图矩阵，行优先 `[frame][bin]`，f32 dB（M3）。
    #[serde(serialize_with = "crate::b64::encode")]
    pub spectrogram_db: Vec<u8>,
    /// 声谱图宽度（时间格数）。
    pub spectrogram_frames: usize,
    /// 声谱图高度（频率 bin 数）。
    pub spectrogram_bins: usize,
    /// 每帧代表的时间（毫秒），供时间轴标注。
    ///
    /// 矩阵被降采样时这里是**有效** hop（源 hop × 降采样倍数），
    /// 因此 `spectrogram_frames × spectrogram_hop_ms` 等于矩阵覆盖的真实时长。
    pub spectrogram_hop_ms: f64,
    /// 声谱图矩阵的绝对起点（秒，相对文件开头）。
    pub spectrogram_start_seconds: f64,
    /// 分析所用 FFT 长度，保证结果可复现。
    pub fft_size: usize,
    /// 窗函数标识。
    pub window: String,
    /// 本次分析的任务 ID。
    ///
    /// 由调用方在发起分析时指定（见 `analyze_file` 的 `task_id` 参数），
    /// 前端据此在运行期间调用 `cancel_task`。`run_analysis` 内部构造时填 0，
    /// 命令层在返回前覆盖成真实值。
    pub task_id: u64,
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
/// `task_id` 由**调用方**指定（通常是前端生成的单调计数器），这样分析还没
/// 返回时就能用同一个 ID 调 `cancel_task`。不传时退回服务端分配，行为与旧版一致。
///
/// # Errors
/// 文件问题返回错误；用户取消返回 `Ok` 且 `outcome == "cancelled"`，
/// **不当作错误**——用户主动取消不是失败。
pub fn analyze_file(
    state: &AppState,
    path: String,
    envelope_points: Option<usize>,
    task_id: Option<u64>,
) -> Result<FileAnalysisDto, String> {
    let p = PathBuf::from(&path);
    let label = file_label(&p);
    let token = CancelToken::new();
    // 注册令牌，使 cancel_task 与任务队列都能找到它
    let task_id = match task_id {
        Some(id) => {
            if !state.try_register_task(id, token.clone(), "analyze", &label) {
                return Err(format!("task id {id} is already running"));
            }
            id
        }
        None => {
            let id = state.next_task_id();
            state.register_task(id, token.clone(), "analyze", &label);
            id
        }
    };

    let requested_points = envelope_points
        .unwrap_or(2048)
        .clamp(1, MAX_ENVELOPE_POINTS);
    let started = std::time::Instant::now();

    let result = analyse_isolated(&p, &token, requested_points);
    state.unregister_token(task_id);

    result.map(|mut d| {
        d.elapsed_ms = started.elapsed().as_millis() as u64;
        d.task_id = task_id;
        d
    })
}

/// 在 panic 隔离下执行一次分析闭包。
///
/// 第三方解码器遇到畸形文件时是可能 panic 的（symphonia 的解析路径、
/// 以及我们自己的切片索引）。ADR-009 要求「单个坏文件不能带崩整个应用」，
/// 所以命令层用 `catch_unwind` 兜住：解析错误转成 `Err`，panic 也转成 `Err`，
/// 界面显示错误条而不是整个进程消失。
///
/// 注意：`run_analysis` 内部已经把**可预期**的错误转成了 `Err`，
/// 这里只兜住真正不该发生的 panic。参数化成闭包是为了能单独测这条路径。
fn isolate_task<T, F>(label: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            tracing::error!(label = %label, panic = %detail, "analysis panicked");
            Err(format!("analysis panicked on {label}: {detail}"))
        }
    }
}

/// 分析一个路径，并把 panic 隔离成错误。
fn analyse_isolated(
    path: &Path,
    token: &CancelToken,
    envelope_points: usize,
) -> Result<FileAnalysisDto, String> {
    let label = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    isolate_task(&label, || run_analysis(path, token, envelope_points))
}

/// 声谱图视口结果（M4 视口分块）。
///
/// 只包含一个时间窗口的 STFT 矩阵，与 `analyze_file` 的整文件指标无关。
/// 前端平移视口时重复调用 `spectrogram_view`，不必重跑整文件分析。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrogramViewDto {
    /// 行优先 `[frame][bin]`，f32 dB。
    #[serde(serialize_with = "crate::b64::encode")]
    pub spectrogram_db: Vec<u8>,
    /// 矩阵宽度（时间格数）。
    pub frames: usize,
    /// 矩阵高度（频率 bin 数）。
    pub bins: usize,
    /// 有效 hop（毫秒）：`frames × hop_ms` = 矩阵覆盖的真实时长。
    pub hop_ms: f64,
    /// 矩阵起点（秒，相对文件开头，已对齐到 STFT hop 网格）。
    pub start_seconds: f64,
    /// 矩阵覆盖的时长（秒）。
    pub duration_seconds: f64,
    /// 源采样率，供频率轴使用。
    pub sample_rate: u32,
    /// STFT 帧长。
    pub fft_size: usize,
    /// 窗函数标识。
    pub window: String,
    /// `succeeded` 或 `cancelled`。
    pub outcome: String,
    /// 处理耗时，毫秒。
    pub elapsed_ms: u64,
    /// 任务 ID。
    pub task_id: u64,
}

/// 声谱图视口的最小/最大时间窗（秒）。
pub const MIN_SPECTROGRAM_WINDOW_SECONDS: f64 = 0.1;
/// 上限与 `SPECTRUM_WINDOW_SECONDS` 一致：30 秒 @48k 的捕获缓冲约 5.8 MB。
pub const MAX_SPECTROGRAM_WINDOW_SECONDS: f64 = SPECTRUM_WINDOW_SECONDS as f64;

const STFT_FRAME_LEN: usize = 2048;
const STFT_HOP_SIZE: usize = 512;

/// 计算一个时间窗口的声谱图。
///
/// `start_seconds` 会被钳到 `[0, duration - window]`，并向下对齐到全局
/// hop 网格：同一时间窗口无论从哪个视口请求，帧位置都一致。
///
/// # Errors
/// 文件不可读、解码失败，或参数非法。
pub fn spectrogram_view(
    state: &AppState,
    path: String,
    start_seconds: f64,
    window_seconds: Option<f64>,
    task_id: Option<u64>,
) -> Result<SpectrogramViewDto, String> {
    if !start_seconds.is_finite() {
        return Err("startSeconds must be finite".to_string());
    }
    if window_seconds.is_some_and(|w| !w.is_finite()) {
        return Err("windowSeconds must be finite".to_string());
    }
    let token = CancelToken::new();
    let window = window_seconds
        .unwrap_or(MAX_SPECTROGRAM_WINDOW_SECONDS)
        .clamp(
            MIN_SPECTROGRAM_WINDOW_SECONDS,
            MAX_SPECTROGRAM_WINDOW_SECONDS,
        );
    let label = format!(
        "{} · {:.1}–{:.1} s",
        file_label(Path::new(&path)),
        start_seconds.max(0.0),
        start_seconds.max(0.0) + window
    );
    let task_id = match task_id {
        Some(id) => {
            if !state.try_register_task(id, token.clone(), "spectrogram", &label) {
                return Err(format!("task id {id} is already running"));
            }
            id
        }
        None => {
            let id = state.next_task_id();
            state.register_task(id, token.clone(), "spectrogram", &label);
            id
        }
    };

    let started = std::time::Instant::now();
    let result = isolate_task(&path, || {
        run_spectrogram_view(Path::new(&path), &token, start_seconds, window_seconds)
    });
    state.unregister_token(task_id);

    result.map(|mut dto| {
        dto.elapsed_ms = started.elapsed().as_millis() as u64;
        dto.task_id = task_id;
        dto
    })
}

/// 真正的视口计算实现（便于单元测试与 panic 隔离）。
fn run_spectrogram_view(
    path: &Path,
    token: &CancelToken,
    start_seconds: f64,
    window_seconds: Option<f64>,
) -> Result<SpectrogramViewDto, String> {
    let info = probe(path).map_err(|e| e.to_string())?;
    let sample_rate = info.format.sample_rate.get();
    let duration = info.duration_seconds;

    let window = window_seconds
        .unwrap_or(MAX_SPECTROGRAM_WINDOW_SECONDS)
        .clamp(
            MIN_SPECTROGRAM_WINDOW_SECONDS,
            MAX_SPECTROGRAM_WINDOW_SECONDS,
        );
    // 不允许窗口起点落到文件末尾之后：钳到最后一个完整窗口。
    let max_start = (duration - window).max(0.0);
    let requested_start = start_seconds.clamp(0.0, max_start);

    // 全局 hop 网格对齐：与 `Stft::compute_range` 的约定一致。
    let target_frame = ((requested_start * f64::from(sample_rate)) as u64 / STFT_HOP_SIZE as u64)
        * STFT_HOP_SIZE as u64;
    let capture_frames = (window * f64::from(sample_rate)).ceil() as usize + STFT_FRAME_LEN;

    let (samples, actual_start) = capture_mono_window(path, target_frame, capture_frames, token)?;

    let stft_cfg = auralis_dsp::StftConfig {
        frame_len: STFT_FRAME_LEN,
        hop_size: STFT_HOP_SIZE,
        window: auralis_dsp::WindowFunction::Hann,
        remove_dc: false,
    };
    let stft = auralis_dsp::Stft::new(stft_cfg).map_err(|e| e.to_string())?;

    if token.is_cancelled() {
        return Ok(empty_view(sample_rate, actual_start, "cancelled"));
    }

    let frames = if samples.len() >= STFT_FRAME_LEN {
        stft.compute(&samples, sample_rate)
            .map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };

    let src_frames = frames.len();
    let src_bins = frames.first().map_or(0, |f| f.db.len());
    let target_frames = src_frames.min(SPECTROGRAM_FRAMES);
    let (matrix, _bin_step) = auralis_dsp::Stft::to_matrix(
        &frames,
        target_frames,
        src_bins.min(SPECTROGRAM_BINS),
        auralis_dsp::STFT_MIN_DB,
        0.0,
    );
    let hop_ms = effective_hop_ms(src_frames, target_frames, stft.hop_seconds(sample_rate));
    let flat: Vec<f32> = matrix.into_iter().flatten().collect();
    let bins = if target_frames > 0 {
        src_bins.min(SPECTROGRAM_BINS)
    } else {
        0
    };

    Ok(SpectrogramViewDto {
        spectrogram_db: f32s_to_le_bytes(&flat),
        frames: target_frames,
        bins,
        hop_ms,
        start_seconds: actual_start as f64 / f64::from(sample_rate),
        duration_seconds: target_frames as f64 * hop_ms / 1000.0,
        sample_rate,
        fft_size: STFT_FRAME_LEN,
        window: stft_cfg.window.id().to_string(),
        outcome: "succeeded".to_string(),
        elapsed_ms: 0,
        task_id: 0,
    })
}

/// 空视口（取消或文件太短）。
fn empty_view(sample_rate: u32, actual_start_frame: u64, outcome: &str) -> SpectrogramViewDto {
    SpectrogramViewDto {
        spectrogram_db: Vec::new(),
        frames: 0,
        bins: 0,
        hop_ms: 0.0,
        start_seconds: actual_start_frame as f64 / f64::from(sample_rate),
        duration_seconds: 0.0,
        sample_rate,
        fft_size: STFT_FRAME_LEN,
        window: auralis_dsp::WindowFunction::Hann.id().to_string(),
        outcome: outcome.to_string(),
        elapsed_ms: 0,
        task_id: 0,
    }
}

/// 从 `target_frame` 起捕获最多 `capture_frames` 个**单声道**（通道 0）样本。
///
/// 先 seek 再解码：`Accurate` seek 可能退到目标之前的包边界，
/// 因此这里会丢弃 `[实际起点, target_frame)` 的样本，保证返回的
/// `actual_start` 就是第一个返回样本的绝对帧号。
/// 容器不支持 seek 时退回从头解码（正确但更慢），并记录一条警告。
fn capture_mono_window(
    path: &Path,
    target_frame: u64,
    capture_frames: usize,
    token: &CancelToken,
) -> Result<(Vec<f32>, u64), String> {
    let mut decoder = BlockDecoder::open(path).map_err(|e| e.to_string())?;
    let mut actual_start = match decoder.seek_to_frame(target_frame) {
        Ok(frame) => frame,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                target_frame,
                %error,
                "seek failed, decoding from the start"
            );
            decoder = BlockDecoder::open(path).map_err(|e| e.to_string())?;
            0
        }
    };

    let channels = decoder.format().channels.get().max(1) as usize;
    let mut out: Vec<f32> = Vec::with_capacity(capture_frames.min(1 << 22));
    let mut first_captured: Option<u64> = None;

    while out.len() < capture_frames {
        if token.is_cancelled() {
            break;
        }

        let Some(block) = decoder.next_block().map_err(|e| e.to_string())? else {
            break;
        };
        let pcm = &block.pcm;
        let block_start = pcm.first_frame;
        let block_frames = pcm.frame_count() as u64;
        if block_start + block_frames <= target_frame {
            continue;
        }

        let skip_frames = target_frame.saturating_sub(block_start) as usize;
        let begin = skip_frames.saturating_mul(channels);
        if begin >= pcm.samples.len() {
            continue;
        }
        if first_captured.is_none() {
            first_captured = Some(block_start + skip_frames as u64);
        }

        for &sample in pcm.samples[begin..].iter().step_by(channels) {
            out.push(if sample.is_finite() { sample } else { 0.0 });
            if out.len() == capture_frames {
                break;
            }
        }
    }

    if let Some(start) = first_captured {
        actual_start = start;
    }
    Ok((out, actual_start))
}

/// 矩阵行的有效时间步长。
///
/// `to_matrix` 把 `src_frames` 行聚合成 `target_frames` 行；如果直接沿用
/// 原始 hop，时间轴会把 30 秒的数据标成 10.9 秒（1024 × 10.67 ms）。
fn effective_hop_ms(src_frames: usize, target_frames: usize, hop_seconds: f64) -> f64 {
    if target_frames == 0 {
        0.0
    } else {
        src_frames as f64 * hop_seconds * 1000.0 / target_frames as f64
    }
}

/// 取消指定任务。
pub fn cancel_task(state: &AppState, task_id: u64) -> Result<bool, String> {
    Ok(state.cancel(task_id))
}

/// 导出结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcomeDto {
    /// 实际写入的路径。
    pub destination: String,
    /// 规范化后的格式（`csv` / `json`）。
    pub format: String,
    /// 写入字节数。
    pub bytes_written: u64,
    /// 重新分析 + 渲染 + 写盘的总耗时，毫秒。
    pub elapsed_ms: u64,
}

/// 把一个文件的分析结果导出为 CSV / JSON。
///
/// # 为什么重新分析而不是把前端的数据传回来
///
/// 前端手里只有降采样后的视图数据，且要跨越 IPC 传回几百 KB~几 MB。
/// 导出必须与展示同源，也要能独立复现，所以这里重跑同一条 `run_analysis`
/// 路径；代价是一次解码，收益是「导出内容 = 显示内容」，且格式化逻辑
/// 留在 Rust（前端不碰数值，ADR-018 规则 1）。
///
/// # Errors
/// 格式不认识、目标扩展名与格式不符、分析失败或写盘失败时返回错误。
pub fn export_analysis(
    source: String,
    destination: String,
    format: String,
    envelope_points: Option<usize>,
) -> Result<ExportOutcomeDto, String> {
    let format = ExportFormat::parse(&format).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&destination);

    // 扩展名与格式不一致时直接拒绝：写一个后缀是 .json 的 CSV 文件，
    // 比报错糟糕得多——用户会以为那是 JSON。
    if let Some(from_ext) = ExportFormat::from_path(&dest) {
        if from_ext != format {
            return Err(format!(
                "destination extension .{} does not match requested format {}",
                from_ext.as_str(),
                format.as_str()
            ));
        }
    } else {
        return Err(format!(
            "destination must end with .csv or .json: {}",
            dest.display()
        ));
    }

    let started = std::time::Instant::now();
    let points = envelope_points
        .unwrap_or(2048)
        .clamp(1, MAX_ENVELOPE_POINTS);
    let dto = analyse_isolated(&PathBuf::from(&source), &CancelToken::new(), points)?;

    if dto.outcome != "succeeded" {
        return Err(format!(
            "analysis did not succeed (outcome={})",
            dto.outcome
        ));
    }

    let spectrum = f32s_from_le_bytes(&dto.spectrum_db);
    let waveform = f32s_from_le_bytes(&dto.waveform_envelope);
    let peaks: Vec<ExportPeak> = dto
        .peaks
        .iter()
        .map(|p| ExportPeak {
            frequency_hz: p.frequency_hz,
            amplitude_dbfs: p.amplitude_dbfs,
            interpolated_dbfs: p.interpolated_dbfs,
            scalloping_db: p.scalloping_db,
            estimated: p.estimated,
        })
        .collect();

    let export = AnalysisExport {
        metadata: ExportMetadata {
            app_version: auralis_core::APP_VERSION.to_string(),
            algorithm_version: auralis_core::ALGORITHM_VERSION.to_string(),
            dsp_spec_revision: auralis_core::DSP_SPEC_REVISION.to_string(),
            exported_unix_ms: exported_unix_ms(),
            source_path: dto.info.path.clone(),
            file_name: dto.info.file_name.clone(),
            container: dto.info.container_name.clone(),
            codec: dto.info.codec.clone(),
            sample_rate_hz: dto.info.sample_rate,
            channels: dto.info.channels,
            duration_s: dto.info.duration_seconds,
            declared_duration_s: Some(dto.declared_duration_seconds),
            truncated: dto.truncated,
            frames_decoded: dto.frames_decoded,
            analysis: AnalysisParameters {
                fft_size: dto.fft_size,
                window: dto.window.clone(),
                spectrum_bins: dto.spectrum_bins,
                spectrum_step_hz: dto.spectrum_step_hz,
                spectrogram_frames: dto.spectrogram_frames,
                spectrogram_bins: dto.spectrogram_bins,
                spectrogram_hop_ms: dto.spectrogram_hop_ms,
                waveform_points: waveform.len(),
                waveform_source: "channel 0, per-bucket maximum",
            },
        },
        metrics: ExportMetrics {
            peak_dbfs: dto.peak_dbfs,
            rms_dbfs: dto.rms_dbfs,
            crest_db: dto.crest_db,
            dc_offset: dto.dc,
            peak_dbfs_per_channel: dto.peak_dbfs_per_channel.clone(),
            over_full_scale_samples: dto.over_full_scale_samples,
            clipping_regions: dto.clipping_regions,
            sanitized_samples: dto.sanitized_samples,
        },
        spectrum_db: &spectrum,
        waveform_envelope: &waveform,
        peaks: &peaks,
    };

    let bytes_written = auralis_storage::write_export(&dest, format, &export)
        .map_err(|e| format!("export failed: {e}"))?;

    Ok(ExportOutcomeDto {
        destination: dest.display().to_string(),
        format: format.as_str().to_string(),
        bytes_written,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// 任务队列里显示的文件名；路径没有文件名时退回完整路径。
fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// 当前 Unix 毫秒时间戳。
///
/// 不做本地时区格式化：那需要完整的日期换算，出错时时间戳会静默错位。
/// 导出的读者（人或脚本）自己决定怎么显示。
fn exported_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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

    // 文件头声明的时长（探针读到的元数据）。截断文件里它大于实际解码时长，
    // 两者之差就是「缺了多少」。
    let declared_duration_seconds = info.duration_seconds;

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

    // 数据流提前结束 = 文件被截断（没下完 / 拷贝不完整）。
    // 已解出的部分完全有效，因此**不**当作失败：照常给出结果，
    // 同时把「只分析了这么多」如实告诉调用方。
    let truncated = decoder.is_truncated();

    if total_frames == 0 {
        return Err(if truncated {
            "truncated file: no audio data could be decoded".to_string()
        } else {
            "decoded zero frames".to_string()
        });
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

    let (matrix, hop_ms) = if spectrum_capture.len() >= stft_cfg.frame_len {
        let stft = auralis_dsp::Stft::new(stft_cfg).map_err(|e| e.to_string())?;
        let frames = stft
            .compute(&spectrum_capture, sample_rate)
            .map_err(|e| e.to_string())?;
        // 目标网格取「实际可得」与「上限」的较小者。
        //
        // SPECTROGRAM_FRAMES/BINS 是**上限**，不是固定尺寸。早先直接把上限
        // 传给 `to_matrix`，短文件会被上采样（4 秒音频铺满 1024 列 ≈ 10.9 秒），
        // 时间轴因此与真实时长不符，且每列不再对应一个 hop。
        let src_frames = frames.len();
        let src_bins = frames.first().map_or(0, |f| f.db.len());
        let target_frames = src_frames.min(SPECTROGRAM_FRAMES);
        let (m, _bin_step) = auralis_dsp::Stft::to_matrix(
            &frames,
            target_frames,
            src_bins.min(SPECTROGRAM_BINS),
            auralis_dsp::STFT_MIN_DB,
            0.0,
        );
        (
            m,
            effective_hop_ms(src_frames, target_frames, stft.hop_seconds(sample_rate)),
        )
    } else {
        (Vec::new(), 0.0)
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
        truncated,
        declared_duration_seconds,
        decoded_duration_seconds: total_frames as f64 / f64::from(sample_rate),
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
        spectrogram_start_seconds: 0.0,
        fft_size: spectrum_cfg.fft_size,
        window: spectrum_cfg.window.id().to_string(),
        // 命令层在返回前填真实任务号；直接调用 run_analysis 的测试看到 0
        task_id: 0,
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
        // 时长用**文件头声明**的值（与 `probe_file` 一致）：截断文件若按
        // 实际解码帧数算，界面会显示一个比文件真实长度短的时长，
        // 掩盖了「文件不完整」这件事。容器不声明总帧数时才退回实测长度。
        duration_seconds: Some(if info.total_frames.is_some() {
            info.duration_seconds
        } else {
            frames as f64 / f64::from(sample_rate)
        }),
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
    let declared_duration_seconds = info.duration_seconds;
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
        truncated: false,
        declared_duration_seconds,
        decoded_duration_seconds: 0.0,
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
        spectrogram_start_seconds: 0.0,
        fft_size: 0,
        window: String::new(),
        task_id: 0,
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

/// 小端字节 -> `Vec<f32>`；长度不是 4 的倍数时返回空。
///
/// 只有导出需要把已经在 DTO 里的二进制载荷解码回来（展示时前端直接消费）。
/// 长度非法时**返回空而不是猜**：宁可导出没有频谱，也不能导出一组错位的值。
fn f32s_from_le_bytes(bytes: &[u8]) -> Vec<f32> {
    if !bytes.len().is_multiple_of(4) {
        return Vec::new();
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
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
    fn truncated_file_is_analysed_and_flagged() {
        let dir = scratch("truncated");
        let path = dir.join("truncated.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 48_000));

        // 砍掉一半数据：文件头仍声明 1 秒，实际只剩约 0.5 秒
        let full = std::fs::read(&path).expect("read");
        std::fs::write(&path, &full[..full.len() / 2]).expect("truncate");

        // 关键：截断的文件**仍然给出分析结果**，而不是整体失败。
        let d = run_analysis(&path, &CancelToken::new(), 512).expect("truncated analysis");

        assert_eq!(d.outcome, "succeeded");
        assert!(d.truncated, "截断必须被标记");
        assert!(
            d.frames_decoded > 0,
            "仍应解出可用音频，实得 {} 帧",
            d.frames_decoded
        );
        assert!(
            d.frames_decoded < 48_000,
            "解出的帧数应短于声明的 48000，实得 {}",
            d.frames_decoded
        );

        // 声明时长与实际时长都要如实给出，差值就是缺了多少
        assert!(
            (d.declared_duration_seconds - 1.0).abs() < 1e-6,
            "声明时长应为 1.0s，实得 {}",
            d.declared_duration_seconds
        );
        let decoded = d.decoded_duration_seconds;
        assert!(
            decoded > 0.0 && decoded < d.declared_duration_seconds,
            "实际时长应介于 0 与声明时长之间，实得 {decoded}"
        );

        // 完整文件不得被误标
        let full_path = dir.join("full.wav");
        write_wav_s16(
            &full_path,
            48_000,
            1,
            &sine_i16(1_000.0, 0.5, 48_000, 48_000),
        );
        let ok = run_analysis(&full_path, &CancelToken::new(), 512).expect("full analysis");
        assert!(!ok.truncated, "完整文件不得被标记为截断");
        assert_eq!(ok.frames_decoded, 48_000);

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&full_path).ok();
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

        // 时间轴必须对应**实际分析到的时长**。
        //
        // 回归：曾经把 SPECTROGRAM_FRAMES（上限）直接当目标网格，1 秒的文件
        // 被上采样成 1024 列 ≈ 10.9 秒，横轴与音频完全对不上。
        assert!(
            d.spectrogram_frames < SPECTROGRAM_FRAMES,
            "1 秒文件不应铺满 1024 列（那是上限，不是固定尺寸）"
        );
        let span_ms = d.spectrogram_frames as f64 * d.spectrogram_hop_ms;
        assert!(
            (span_ms - 1_000.0).abs() < 60.0,
            "矩阵时间跨度应约等于 1 秒，实得 {span_ms} ms"
        );

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
    fn spectrogram_time_axis_stays_correct_when_frames_are_downsampled() {
        // 12 秒 > 1024 列 × 10.67 ms ≈ 10.9 秒，矩阵必然被降采样。
        // 有效 hop 必须随降采样倍数放大，否则时间轴会短于真实窗口。
        let dir = scratch("sg-downsample");
        let path = dir.join("long.wav");
        write_wav_s16(
            &path,
            48_000,
            1,
            &sine_i16(1_000.0, 0.5, 48_000, 12 * 48_000),
        );

        let d = run_analysis(&path, &CancelToken::new(), 512).expect("analysis");
        assert_eq!(
            d.spectrogram_frames, SPECTROGRAM_FRAMES,
            "12 秒的矩阵应被降到列数上限"
        );
        let span_s = d.spectrogram_frames as f64 * d.spectrogram_hop_ms / 1000.0;
        assert!(
            (span_s - 12.0).abs() < 0.05,
            "矩阵跨度应约等于 12 秒，实得 {span_s} s（hop={} ms）",
            d.spectrogram_hop_ms
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn spectrogram_view_returns_the_requested_time_window() {
        let dir = scratch("sg-view");
        let path = dir.join("window.wav");
        // 6 秒：0–3 静音，3–4 为 1 kHz，其余静音。
        let n = 6 * 48_000_usize;
        let mut samples = vec![0i16; n];
        let tone = sine_i16(1_000.0, 0.5, 48_000, 48_000);
        samples[3 * 48_000..4 * 48_000].copy_from_slice(&tone);
        write_wav_s16(&path, 48_000, 1, &samples);

        let token = CancelToken::new();
        let quiet = run_spectrogram_view(&path, &token, 0.0, Some(1.0)).expect("quiet view");
        let loud = run_spectrogram_view(&path, &token, 3.0, Some(1.0)).expect("loud view");

        // 起点向下对齐到全局 hop 网格，误差不超过一个 hop。
        assert!(
            (loud.start_seconds - 3.0).abs() < 512.0 / 48_000.0 + 1e-9,
            "起点应对齐到 3 秒附近，实得 {}",
            loud.start_seconds
        );
        assert!(loud.frames > 0 && loud.bins > 0);
        assert_eq!(
            loud.spectrogram_db.len(),
            loud.frames * loud.bins * 4,
            "矩阵字节数必须与行列数对应"
        );
        assert!(loud.hop_ms > 0.0);
        assert!(
            (loud.duration_seconds - 0.98).abs() < 0.05,
            "1 秒窗口的矩阵跨度应约 1 秒，实得 {}",
            loud.duration_seconds
        );

        let peak_db = |view: &SpectrogramViewDto| {
            crate::commands::decode_test_f32s(&view.spectrogram_db)
                .into_iter()
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert!(
            peak_db(&loud) > -20.0,
            "3–4 秒应有 1 kHz 信号，实得峰值 {} dBFS",
            peak_db(&loud)
        );
        assert!(
            peak_db(&quiet) < -80.0,
            "0–1 秒应接近静音，实得峰值 {} dBFS",
            peak_db(&quiet)
        );

        // 起点超过文件末尾时钳到最后一个完整窗口，不 panic。
        let clamped = run_spectrogram_view(&path, &token, 5.9, Some(1.0)).expect("clamped view");
        assert!(
            clamped.start_seconds <= 5.01,
            "起点应被钳到最后一屏，实得 {}",
            clamped.start_seconds
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

    #[test]
    fn export_writes_csv_and_json_with_provenance() {
        let dir = scratch("export");
        let path = dir.join("tone.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 48_000));
        let source = path.to_string_lossy().into_owned();

        let csv_path = dir.join("out.csv");
        let csv_out = export_analysis(
            source.clone(),
            csv_path.to_string_lossy().into_owned(),
            "csv".to_string(),
            Some(512),
        )
        .expect("csv export");
        assert!(csv_out.bytes_written > 0);
        assert_eq!(csv_out.format, "csv");

        let csv = std::fs::read_to_string(&csv_path).expect("read csv");
        for required in [
            "# app_version=",
            "# algorithm_version=",
            "# dsp_spec_revision=",
            "# fft_size=",
            "# window=",
            "series,index,x_hz,x_s,value,unit,quantity",
            ",dBFS,amplitude_dbfs",
        ] {
            assert!(csv.contains(required), "CSV 缺少 {required}");
        }
        assert!(
            csv.contains(auralis_core::ALGORITHM_VERSION),
            "算法版本必须写进导出，否则无法追溯"
        );

        let json_path = dir.join("out.json");
        export_analysis(
            source,
            json_path.to_string_lossy().into_owned(),
            "json".to_string(),
            Some(512),
        )
        .expect("json export");

        let json_text = std::fs::read_to_string(&json_path).expect("read json");
        let v: serde_json::Value = serde_json::from_str(&json_text).expect("valid json");
        assert_eq!(v["schema"], "auralis.export.analysis/2");
        assert_eq!(
            v["metadata"]["algorithmVersion"],
            auralis_core::ALGORITHM_VERSION
        );
        assert_eq!(v["metadata"]["truncated"], false);
        assert!(v["metadata"]["declaredDurationS"].is_number());
        assert_eq!(v["metadata"]["analysis"]["window"], "hann");
        let bins = v["metadata"]["analysis"]["spectrumBins"]
            .as_u64()
            .expect("spectrumBins") as usize;
        assert_eq!(
            v["spectrum"]["amplitudeDbfs"]
                .as_array()
                .expect("spectrum array")
                .len(),
            bins,
            "频谱数组长度必须与元数据声明一致"
        );

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&csv_path).ok();
        std::fs::remove_file(&json_path).ok();
    }

    #[test]
    fn export_rejects_extension_that_contradicts_format() {
        let dir = scratch("export-mismatch");
        let path = dir.join("tone.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 4_800));

        let err = export_analysis(
            path.to_string_lossy().into_owned(),
            dir.join("out.json").to_string_lossy().into_owned(),
            "csv".to_string(),
            Some(64),
        )
        .expect_err("扩展名与格式不符必须报错");
        assert!(err.contains("does not match"), "错误信息应说明原因：{err}");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn export_round_trips_float_bytes() {
        let original = vec![0.0_f32, -6.5, f32::NEG_INFINITY, 1.25];
        let bytes = f32s_to_le_bytes(&original);
        assert_eq!(f32s_from_le_bytes(&bytes), original);
        // 非法长度不能被当成数据
        assert!(f32s_from_le_bytes(&bytes[..bytes.len() - 1]).is_empty());
    }

    #[test]
    fn panicking_analysis_is_isolated_into_an_error() {
        // 隔离器必须把 panic 变成 Err：这才是「一个坏文件不带崩应用」的落点。
        let err =
            isolate_task::<FileAnalysisDto, _>("broken.wav", || panic!("simulated decoder panic"))
                .expect_err("panic 必须被转换成错误");
        assert!(err.contains("panicked"), "错误信息应说明发生了什么：{err}");
        assert!(
            err.contains("broken.wav"),
            "错误信息应指明是哪个文件：{err}"
        );
        assert!(
            err.contains("simulated decoder panic"),
            "应保留 panic 文本：{err}"
        );

        // 正常路径不受影响
        let dir = scratch("isolation");
        let path = dir.join("ok.wav");
        write_wav_s16(&path, 48_000, 1, &sine_i16(1_000.0, 0.5, 48_000, 4_800));
        let d = analyse_isolated(&path, &CancelToken::new(), 64).expect("normal analysis");
        assert_eq!(d.outcome, "succeeded");
        std::fs::remove_file(&path).ok();
    }
}
