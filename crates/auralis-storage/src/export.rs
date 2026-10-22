//! 分析结果导出：CSV（长格式，可直接进 pandas / Excel）与 JSON（完整结构）。
//!
//! 两者都**必须**自带可追溯信息：应用版本、算法版本、DSP 规格修订号、
//! 单位、分析参数和数据来源。一份没有这些信息的导出，半年后没人能解释它
//! 是用什么参数算出来的。
//!
//! ## 数值约定
//!
//! - 幅值：**dBFS**（满刻度 1.0 = 0 dBFS），静音是 `-inf`。
//!   非有限值在 JSON 里是 `null`，在 CSV 里是**空字段**（不是 0，也不是 `-999`）。
//!   两者都在元数据里用 `non_finite_values=<n>` 报出计数，避免静默丢信息。
//! - 频率：Hz；时间：秒。
//! - 波形包络：`[-1, 1]`，来源是**第一个声道**的按桶最大值（见
//!   `AnalysisParameters::waveform_source`），不是所有声道的 min/max。

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// 长格式 CSV：`series,index,x_hz,x_s,value,unit,quantity`。
    Csv,
    /// 结构化 JSON（含元数据、指标、峰值、频谱、包络数组）。
    Json,
}

impl ExportFormat {
    /// 解析格式字符串（大小写不敏感）。
    pub fn parse(raw: &str) -> Result<Self, ExportError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            other => Err(ExportError::UnsupportedFormat(other.to_string())),
        }
    }

    /// 由目标路径的扩展名推断格式。
    ///
    /// 返回 `None` 表示扩展名缺失或不受支持；调用方应报错而不是猜一个格式，
    /// 否则用户会得到一个内容和后缀不一致的文件。
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// 规范化名称。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    /// 文件扩展名（不含点）。
    #[must_use]
    pub fn extension(self) -> &'static str {
        self.as_str()
    }
}

/// 导出过程中的错误。
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// 格式字符串不认识。
    #[error("unsupported export format: {0} (expected csv or json)")]
    UnsupportedFormat(String),
    /// 目标路径不可用（没有文件名、指向目录）。
    #[error("invalid destination path: {0}")]
    InvalidDestination(String),
    /// 写盘失败。
    #[error("failed to write export: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 序列化失败（不应发生，除非结构被改坏）。
    #[error("failed to render json: {0}")]
    Json(#[from] serde_json::Error),
}

/// 分析参数的追溯记录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisParameters {
    /// FFT 长度。
    pub fft_size: usize,
    /// 窗函数标识。
    pub window: String,
    /// 频谱点数（已降采样到画布宽度）。
    pub spectrum_bins: usize,
    /// 频谱频率间隔，Hz。
    pub spectrum_step_hz: f64,
    /// 声谱图时间格数。
    pub spectrogram_frames: usize,
    /// 声谱图频率格数。
    pub spectrogram_bins: usize,
    /// 声谱图每列对应的时间，毫秒。
    pub spectrogram_hop_ms: f64,
    /// 波形包络点数。
    pub waveform_points: usize,
    /// 波形包络的来源说明（必须如实：这里只有第一个声道）。
    pub waveform_source: &'static str,
}

/// 数据来源与算法版本。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMetadata {
    /// 应用版本。
    pub app_version: String,
    /// 算法版本（`auralis-dsp` 规格版本）。
    pub algorithm_version: String,
    /// DSP 规格修订号。
    pub dsp_spec_revision: String,
    /// 导出时刻（Unix 毫秒）。不做本地时区格式化，避免日期算法本身出错。
    pub exported_unix_ms: u64,
    /// 源文件完整路径。
    pub source_path: String,
    /// 源文件名。
    pub file_name: String,
    /// 容器标识。
    pub container: String,
    /// 编码标识。
    pub codec: String,
    /// 采样率 Hz。
    pub sample_rate_hz: u32,
    /// 声道数。
    pub channels: u32,
    /// 时长秒；未知时为 `None`（不编造 0）。
    ///
    /// 这是**实际参与分析**的时长。源文件被截断时它小于文件头声明的时长，
    /// 差值见 `declared_duration_s`。
    pub duration_s: Option<f64>,
    /// 文件头声明的时长秒；容器未声明时为 `None`。
    pub declared_duration_s: Option<f64>,
    /// 源文件是否被截断（数据流在解完之前结束）。
    ///
    /// 导出必须记下这一点：拿截断文件的结果去和完整文件比对时，
    /// 差异的来源要能说清楚，否则溯源就断了。
    pub truncated: bool,
    /// 解码帧数。
    pub frames_decoded: u64,
    /// 分析参数。
    pub analysis: AnalysisParameters,
}

impl ExportMetadata {
    /// 参数是否完整到足以复现结果。
    ///
    /// 只检查“结构性”前提（采样率、FFT 长度、声道数非零、窗函数非空），
    /// 不检查数值是否合理——那是 DSP 层的职责。
    #[must_use]
    pub fn is_reproducible(&self) -> bool {
        self.sample_rate_hz > 0
            && self.channels > 0
            && self.analysis.fft_size > 0
            && !self.analysis.window.is_empty()
    }
}

/// 标量指标。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMetrics {
    /// 峰值 dBFS；`None` = 静音。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS；`None` = 静音。
    pub rms_dbfs: Option<f32>,
    /// 波峰因数 dB；`None` = 未定义。
    pub crest_db: Option<f32>,
    /// 直流偏移（线性）。
    pub dc_offset: f32,
    /// 每声道峰值 dBFS。
    pub peak_dbfs_per_channel: Vec<Option<f32>>,
    /// 超满刻度样本数。
    pub over_full_scale_samples: usize,
    /// 削波平顶片段数（启发式）。
    pub clipping_regions: u32,
    /// 被清理的非有限样本数。
    pub sanitized_samples: usize,
}

/// 一个检出峰。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPeak {
    /// 插值后频率 Hz。
    pub frequency_hz: f64,
    /// bin 读数 dBFS。
    pub amplitude_dbfs: f32,
    /// 抛物线插值读数 dBFS。
    pub interpolated_dbfs: f32,
    /// 扇贝损失 dB。
    pub scalloping_db: f32,
    /// 读数是否为估算。
    pub estimated: bool,
}

/// 一次导出的全部内容。
#[derive(Debug, Clone)]
pub struct AnalysisExport<'a> {
    /// 来源与参数。
    pub metadata: ExportMetadata,
    /// 标量指标。
    pub metrics: ExportMetrics,
    /// 频谱 dB 值，索引 * step = 频率。
    pub spectrum_db: &'a [f32],
    /// 波形包络，值域 `[-1, 1]`。
    pub waveform_envelope: &'a [f32],
    /// 检出峰。
    pub peaks: &'a [ExportPeak],
}

impl AnalysisExport<'_> {
    /// 非有限值的总数（频谱 + 包络）。
    ///
    /// 这些值在导出里是 `null`/空字段；计数写进元数据，让读者知道
    /// 有多少格子是静音而不是“缺数据”。
    #[must_use]
    pub fn non_finite_count(&self) -> usize {
        self.spectrum_db
            .iter()
            .chain(self.waveform_envelope.iter())
            .filter(|v| !v.is_finite())
            .count()
    }
}

/// JSON 里的单位声明。单位只写一处，避免每个数组元素重复。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Units {
    spectrum_amplitude: &'static str,
    spectrum_frequency_axis: &'static str,
    waveform_amplitude: &'static str,
    peak_amplitude: &'static str,
    peak_frequency_axis: &'static str,
    dc_offset: &'static str,
}

const UNITS: Units = Units {
    spectrum_amplitude: "dBFS",
    spectrum_frequency_axis: "Hz",
    waveform_amplitude: "full-scale [-1, 1]",
    peak_amplitude: "dBFS",
    peak_frequency_axis: "Hz",
    dc_offset: "linear",
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonDoc<'a> {
    /// 结构版本，供将来变更时识别。
    schema: &'static str,
    metadata: &'a ExportMetadata,
    metrics: &'a ExportMetrics,
    units: &'a Units,
    non_finite_values: usize,
    peaks: &'a [ExportPeak],
    spectrum: SpectrumSeries<'a>,
    waveform: WaveformSeries<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpectrumSeries<'a> {
    frequency_start_hz: f64,
    step_hz: f64,
    amplitude_dbfs: &'a [f32],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WaveformSeries<'a> {
    points: usize,
    envelope: &'a [f32],
}

/// 渲染 JSON 文本。
///
/// # Errors
/// 只在序列化失败时返回错误（结构被改坏才会发生）。
pub fn render_json(export: &AnalysisExport<'_>) -> Result<String, ExportError> {
    let doc = JsonDoc {
        schema: "auralis.export.analysis/2",
        metadata: &export.metadata,
        metrics: &export.metrics,
        units: &UNITS,
        non_finite_values: export.non_finite_count(),
        peaks: export.peaks,
        spectrum: SpectrumSeries {
            frequency_start_hz: 0.0,
            step_hz: export.metadata.analysis.spectrum_step_hz,
            amplitude_dbfs: export.spectrum_db,
        },
        waveform: WaveformSeries {
            points: export.waveform_envelope.len(),
            envelope: export.waveform_envelope,
        },
    };
    // pretty 输出：导出的主要用途是人工核对与 diff。
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// 渲染 CSV 文本（长格式）。
///
/// 结构：
/// 1. `#` 开头的元数据行（`# key=value`）；
/// 2. 一行表头 `series,index,x_hz,x_s,value,unit,quantity`；
/// 3. `spectrum` / `waveform` / `peaks` 三种系列的明细行。
///
/// 长格式比宽表更耐改：将来加一个系列不需要动已有列的含义。
#[must_use]
pub fn render_csv(export: &AnalysisExport<'_>) -> String {
    let m = &export.metadata;
    let a = &m.analysis;
    let mut out = String::with_capacity(
        512 + 24 * (export.spectrum_db.len() + export.waveform_envelope.len() + export.peaks.len()),
    );

    // 先算好全部元数据行，再一次性写入：closure 里同时借 out 会与下面的
    // writeln! 冲突（E0499），而数组字面量既清楚又没有借用问题。
    let meta_rows: [(&str, String); 27] = [
        ("app_version", m.app_version.clone()),
        ("algorithm_version", m.algorithm_version.clone()),
        ("dsp_spec_revision", m.dsp_spec_revision.clone()),
        ("exported_unix_ms", m.exported_unix_ms.to_string()),
        ("source_path", m.source_path.clone()),
        ("file_name", m.file_name.clone()),
        ("container", m.container.clone()),
        ("codec", m.codec.clone()),
        ("sample_rate_hz", m.sample_rate_hz.to_string()),
        ("channels", m.channels.to_string()),
        (
            "duration_s",
            m.duration_s.map_or_else(String::new, |d| format!("{d:.6}")),
        ),
        (
            "declared_duration_s",
            m.declared_duration_s
                .map_or_else(String::new, |d| format!("{d:.6}")),
        ),
        ("truncated", m.truncated.to_string()),
        ("frames_decoded", m.frames_decoded.to_string()),
        ("fft_size", a.fft_size.to_string()),
        ("window", a.window.clone()),
        ("spectrum_bins", a.spectrum_bins.to_string()),
        ("spectrum_step_hz", format!("{:.9}", a.spectrum_step_hz)),
        ("spectrogram_frames", a.spectrogram_frames.to_string()),
        ("spectrogram_bins", a.spectrogram_bins.to_string()),
        ("spectrogram_hop_ms", format!("{:.6}", a.spectrogram_hop_ms)),
        ("waveform_points", a.waveform_points.to_string()),
        ("waveform_source", a.waveform_source.to_string()),
        (
            "peak_dbfs",
            export
                .metrics
                .peak_dbfs
                .map_or_else(String::new, |v| format!("{v:.6}")),
        ),
        (
            "rms_dbfs",
            export
                .metrics
                .rms_dbfs
                .map_or_else(String::new, |v| format!("{v:.6}")),
        ),
        (
            "crest_db",
            export
                .metrics
                .crest_db
                .map_or_else(String::new, |v| format!("{v:.6}")),
        ),
        // 空字段表示非有限值（静音），不是缺数据；计数让读者知道有多少个。
        ("non_finite_values", export.non_finite_count().to_string()),
    ];

    let _ = writeln!(out, "# schema=auralis.export.analysis/2");
    for (key, value) in &meta_rows {
        let _ = writeln!(out, "# {key}={}", csv_escape(value));
    }
    // 这几个放在最后也能读，但顺序无关紧要；分开写是为了保持数组长度固定。
    for (key, value) in [
        ("dc_offset", format!("{:.9}", export.metrics.dc_offset)),
        (
            "over_full_scale_samples",
            export.metrics.over_full_scale_samples.to_string(),
        ),
        (
            "clipping_regions",
            export.metrics.clipping_regions.to_string(),
        ),
        (
            "sanitized_samples",
            export.metrics.sanitized_samples.to_string(),
        ),
    ] {
        let _ = writeln!(out, "# {key}={}", csv_escape(&value));
    }
    let _ = writeln!(out, "series,index,x_hz,x_s,value,unit,quantity");

    let step = a.spectrum_step_hz;
    for (i, v) in export.spectrum_db.iter().enumerate() {
        let _ = writeln!(
            out,
            "spectrum,{i},{:.6},,{},dBFS,amplitude_dbfs",
            i as f64 * step,
            csv_num(f64::from(*v), 6)
        );
    }

    let points = export.waveform_envelope.len();
    let duration = m.duration_s.unwrap_or(0.0);
    for (i, v) in export.waveform_envelope.iter().enumerate() {
        let t = if points > 1 {
            duration * i as f64 / (points - 1) as f64
        } else {
            0.0
        };
        let _ = writeln!(
            out,
            "waveform,{i},,{t:.6},{},FS,envelope_ch0_max",
            csv_num(f64::from(*v), 8)
        );
    }

    for (i, p) in export.peaks.iter().enumerate() {
        let _ = writeln!(
            out,
            "peaks,{i},{:.6},,{},dBFS,peak_amplitude",
            p.frequency_hz,
            csv_num(f64::from(p.amplitude_dbfs), 6)
        );
    }

    out
}

/// 把数值写成 CSV 字段；非有限值写成空字段（不编造 0）。
fn csv_num(v: f64, decimals: usize) -> String {
    if v.is_finite() {
        format!("{v:.decimals$}")
    } else {
        String::new()
    }
}

/// CSV 字段转义：含逗号、引号或换行时加引号，内部引号翻倍。
fn csv_escape(raw: &str) -> String {
    if raw.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

/// 原子写盘：先写同目录临时文件，再 `rename` 覆盖目标。
///
/// 直接写目标文件时，中途失败会留下**半截文件**——分析结果导出被用户当成
/// 事实来源，半截文件比没有文件更危险。`rename` 在同一卷上是原子的。
///
/// 返回写入的字节数。
///
/// # Errors
/// 目标没有文件名、指向目录、或写盘/改名失败。
pub fn write_atomically(path: &Path, contents: &str) -> Result<u64, ExportError> {
    if path.is_dir() {
        return Err(ExportError::InvalidDestination(path.display().to_string()));
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ExportError::InvalidDestination(path.display().to_string()))?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let temp = parent.join(format!(".{file_name}.{}.part", std::process::id()));

    {
        let mut f = std::fs::File::create(&temp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        // 改名失败不能留下垃圾文件
        let _ = std::fs::remove_file(&temp);
        return Err(ExportError::Io(e));
    }
    Ok(contents.len() as u64)
}

/// 渲染并写出一份导出，返回写入字节数。
///
/// # Errors
/// 渲染或写盘失败。
pub fn write_export(
    path: &Path,
    format: ExportFormat,
    export: &AnalysisExport<'_>,
) -> Result<u64, ExportError> {
    let text = match format {
        ExportFormat::Csv => render_csv(export),
        ExportFormat::Json => render_json(export)?,
    };
    write_atomically(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_export<'a>(
        spectrum: &'a [f32],
        waveform: &'a [f32],
        peaks: &'a [ExportPeak],
    ) -> AnalysisExport<'a> {
        AnalysisExport {
            metadata: ExportMetadata {
                app_version: "0.1.0".into(),
                algorithm_version: "auralis-dsp/0.1.0+spec.2026-10-14".into(),
                dsp_spec_revision: "2026-10-14".into(),
                exported_unix_ms: 1_760_000_000_000,
                source_path: r"D:\audio, with comma\tone.wav".into(),
                file_name: "tone, 1k.wav".into(),
                container: "WAV".into(),
                codec: "pcm_s16le".into(),
                sample_rate_hz: 48_000,
                channels: 2,
                duration_s: Some(1.5),
                declared_duration_s: Some(1.5),
                truncated: false,
                frames_decoded: 72_000,
                analysis: AnalysisParameters {
                    fft_size: 8192,
                    window: "hann".into(),
                    spectrum_bins: 3,
                    spectrum_step_hz: 5.86,
                    spectrogram_frames: 94,
                    spectrogram_bins: 512,
                    spectrogram_hop_ms: 10.67,
                    waveform_points: 2,
                    waveform_source: "channel 0, per-bucket maximum",
                },
            },
            metrics: ExportMetrics {
                peak_dbfs: Some(-3.01),
                rms_dbfs: Some(-6.02),
                crest_db: Some(3.01),
                dc_offset: 0.000_001,
                peak_dbfs_per_channel: vec![Some(-3.01), Some(-3.02)],
                over_full_scale_samples: 0,
                clipping_regions: 0,
                sanitized_samples: 0,
            },
            spectrum_db: spectrum,
            waveform_envelope: waveform,
            peaks,
        }
    }

    #[test]
    fn format_parsing_is_case_insensitive_and_strict() {
        assert_eq!(ExportFormat::parse("CSV").unwrap(), ExportFormat::Csv);
        assert_eq!(ExportFormat::parse(" json ").unwrap(), ExportFormat::Json);
        assert!(ExportFormat::parse("xml").is_err(), "未知格式必须报错");
        assert_eq!(ExportFormat::Csv.extension(), "csv");
    }

    #[test]
    fn format_from_path_uses_extension_only() {
        assert_eq!(
            ExportFormat::from_path(Path::new("a/b.c.csv")),
            Some(ExportFormat::Csv)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a/b.JSON")),
            Some(ExportFormat::Json)
        );
        assert_eq!(ExportFormat::from_path(Path::new("b.txt")), None);
        assert_eq!(ExportFormat::from_path(Path::new("b")), None);
    }

    #[test]
    fn csv_carries_provenance_units_and_parameters() {
        let spectrum = [0.0, -6.0, f32::NEG_INFINITY];
        let waveform = [0.5, -0.5];
        let peaks = [];
        let export = sample_export(&spectrum, &waveform, &peaks);
        let csv = render_csv(&export);

        for required in [
            "# app_version=0.1.0",
            "# algorithm_version=auralis-dsp/0.1.0+spec.2026-10-14",
            "# dsp_spec_revision=2026-10-14",
            "# sample_rate_hz=48000",
            "# fft_size=8192",
            "# window=hann",
            "# non_finite_values=1",
            "series,index,x_hz,x_s,value,unit,quantity",
        ] {
            assert!(csv.contains(required), "CSV 缺少 {required}:\n{csv}");
        }
        // 单位出现在行内，不能只在元数据里声明
        assert!(csv.contains(",dBFS,amplitude_dbfs"));
        assert!(csv.contains(",FS,envelope_ch0_max"));
    }

    #[test]
    fn csv_row_counts_match_series_lengths() {
        let spectrum = [0.0, -6.0, -12.0];
        let waveform = [0.1, 0.2, 0.3, 0.4];
        let peaks = [ExportPeak {
            frequency_hz: 1000.0,
            amplitude_dbfs: -6.0,
            interpolated_dbfs: -6.02,
            scalloping_db: 0.02,
            estimated: false,
        }];
        let export = sample_export(&spectrum, &waveform, &peaks);
        let csv = render_csv(&export);

        let count = |prefix: &str| csv.lines().filter(|l| l.starts_with(prefix)).count();
        assert_eq!(count("spectrum,"), 3);
        assert_eq!(count("waveform,"), 4);
        assert_eq!(count("peaks,"), 1);
    }

    #[test]
    fn non_finite_values_become_empty_fields_not_zero() {
        let spectrum = [f32::NEG_INFINITY];
        let export = sample_export(&spectrum, &[], &[]);
        let csv = render_csv(&export);
        let row = csv
            .lines()
            .find(|l| l.starts_with("spectrum,0,"))
            .expect("spectrum 行");
        // 列顺序是 series,index,x_hz,x_s,value,...：频谱行的 x_s 与 value 都为空
        assert!(
            row.contains(",0.000000,,,dBFS,"),
            "非有限幅值必须是空字段，实得：{row}"
        );
        assert!(
            !row.ends_with(",0.000000,dBFS,amplitude_dbfs"),
            "不得把静音写成 0"
        );
    }

    #[test]
    fn csv_escapes_commas_and_quotes() {
        let export = sample_export(&[], &[], &[]);
        let csv = render_csv(&export);
        assert!(
            csv.contains("\"tone, 1k.wav\""),
            "含逗号的文件名必须加引号：\n{csv}"
        );
    }

    #[test]
    fn json_contains_units_parameters_and_series() {
        let spectrum = [0.0, -6.0];
        let waveform = [0.5, -0.5];
        let export = sample_export(&spectrum, &waveform, &[]);
        let json = render_json(&export).expect("json");

        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["schema"], "auralis.export.analysis/2");
        assert_eq!(v["units"]["spectrumAmplitude"], "dBFS");
        assert_eq!(v["metadata"]["analysis"]["window"], "hann");
        assert_eq!(
            v["metadata"]["algorithmVersion"],
            "auralis-dsp/0.1.0+spec.2026-10-14"
        );
        assert_eq!(v["spectrum"]["amplitudeDbfs"].as_array().unwrap().len(), 2);
        assert_eq!(v["waveform"]["envelope"].as_array().unwrap().len(), 2);
        assert_eq!(v["metadata"]["sampleRateHz"], 48_000);
        // 截断标记必须进导出：否则拿截断文件的结果和完整文件比对时，
        // 差异来源说不清楚。
        assert_eq!(v["metadata"]["truncated"], false);
        assert_eq!(v["metadata"]["declaredDurationS"], 1.5);
    }

    #[test]
    fn json_maps_non_finite_to_null_and_counts_them() {
        let spectrum = [f32::NEG_INFINITY, -6.0];
        let export = sample_export(&spectrum, &[], &[]);
        let json = render_json(&export).expect("json");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert!(
            v["spectrum"]["amplitudeDbfs"][0].is_null(),
            "-inf 必须是 null"
        );
        assert_eq!(v["nonFiniteValues"], 1);
    }

    #[test]
    fn write_export_produces_file_without_leaving_temp() {
        let dir = std::env::temp_dir().join(format!("auralis-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let target = dir.join("out.csv");
        let spectrum = [0.0];
        let export = sample_export(&spectrum, &[], &[]);

        let bytes = write_export(&target, ExportFormat::Csv, &export).expect("write");
        assert!(bytes > 0);
        let written = std::fs::read_to_string(&target).expect("read back");
        assert!(written.contains("series,index,x_hz,x_s,value,unit,quantity"));

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("list dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件，实得 {leftovers:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_to_a_directory_is_rejected() {
        let dir = std::env::temp_dir();
        let export = sample_export(&[], &[], &[]);
        let err = write_export(&dir, ExportFormat::Json, &export);
        assert!(matches!(err, Err(ExportError::InvalidDestination(_))));
    }

    #[test]
    fn metadata_reproducibility_requires_core_parameters() {
        let export = sample_export(&[], &[], &[]);
        assert!(export.metadata.is_reproducible());

        let mut broken = export.metadata.clone();
        broken.analysis.fft_size = 0;
        assert!(!broken.is_reproducible(), "FFT 长度为 0 时不可复现");
    }
}
