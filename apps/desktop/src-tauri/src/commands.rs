//! Tauri IPC 命令层。
//!
//! 每个命令都遵守 ADR-018 的三条硬规则。注意签名上的选择：
//! 视图数据返回 `Vec<u8>`（序列化后是 JS 的 `Uint8Array`），
//! 而不是 `Vec<f32>`——后者会被序列化成 JSON 数组，逐点变成
//! JSON 数字，是本架构明令禁止的。

use rustfft::num_complex::Complex;
use serde::Serialize;

/// 应用元信息。前端用它显示版本并校验前后端是否同版本。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 应用版本。
    pub version: &'static str,
    /// DSP 算法规格版本。
    pub algorithm_version: &'static str,
    /// DSP 规格修订号。
    pub dsp_spec_revision: &'static str,
}

/// 返回应用与算法版本。
///
/// 这是 ADR-018 表中的「元数据 / 配置 / 指标标量」，走 JSON Command 即可，
/// 无需优化。
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: auralis_core::APP_VERSION,
        algorithm_version: auralis_core::ALGORITHM_VERSION,
        dsp_spec_revision: auralis_core::DSP_SPEC_REVISION,
    }
}

/// 合成信号分析结果（标量部分走 JSON）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticMetrics {
    /// 样本率，Hz。
    pub sample_rate: u32,
    /// 样本数。
    pub sample_count: usize,
    /// 峰值 dBFS。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS。
    pub rms_dbfs: Option<f32>,
    /// 直流偏移，线性。
    pub dc: f32,
    /// 波峰因数，dB。
    pub crest_db: Option<f32>,
    /// 超量程样本数。
    pub over_full_scale_samples: usize,
    /// 削波片段数。
    pub clipping_regions: u32,
    /// 非有限值清理计数。
    pub sanitized_samples: usize,
}

/// IPC 压测用的返回体：标量 + 两个二进制视图数据。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticAnalysis {
    /// 标量指标。
    pub metrics: SyntheticMetrics,
    /// 频谱快照，`Float32Array`，长度为 `spectrum_bins`。
    /// 内容是**已降采样到目标像素宽度**的 dB 值，不是原始 FFT bin。
    pub spectrum_db: Vec<u8>,
    /// `spectrum_db` 对应的频率起点 Hz（等间隔频率轴）。
    pub spectrum_start_hz: f32,
    /// `spectrum_db` 对应的频率间隔 Hz。
    pub spectrum_step_hz: f32,
    /// 频谱点数。
    pub spectrum_bins: usize,
    /// 波形包络，`Float32Array`，通道内交织，值域 [-1, 1]。
    pub waveform_envelope: Vec<u8>,
    /// 包络点数（每声道）。
    pub waveform_points: usize,
    /// Rust 侧完成分析 + 编码所用的微秒数。
    pub compute_micros: u64,
}

/// 分析一个合成信号，用于验证 IPC 通路与压测。
///
/// # 为什么用合成信号
///
/// 这条命令是 **M1 之前的 IPC 冒烟测试**：它不需要文件、不需要解码器，
/// 就能把「Rust 算 -> 序列化 -> WebView 渲染」整条链路跑通并计时。
/// 真实文件分析在 M1 接入 `auralis-codec` 后复用同样的返回结构。
///
/// # 参数
///
/// - `frequency_hz` — 信号频率
/// - `amplitude` — 幅值（线性，满刻度 1.0）
/// - `sample_rate` — 采样率
/// - `fft_size` — FFT 长度（必须是 2 的幂）
/// - `target_bins` — 频谱要降采样到的点数，通常等于画布像素宽度
/// - `envelope_points` — 波形包络的目标点数
#[tauri::command]
pub fn analyze_synthetic_signal(
    frequency_hz: f64,
    amplitude: f32,
    sample_rate: u32,
    fft_size: usize,
    target_bins: usize,
    envelope_points: usize,
) -> Result<SyntheticAnalysis, String> {
    use auralis_dsp::{ClippingConfig, WindowFunction, analyze_channel};

    let start = std::time::Instant::now();

    if fft_size == 0 || !fft_size.is_power_of_two() {
        return Err(format!("fft_size must be a power of two, got {fft_size}"));
    }
    if sample_rate == 0 {
        return Err("sample_rate must be greater than zero".to_string());
    }
    // 生成一个整数个周期的信号，保证频谱峰值落在 bin 上
    let cycles = (frequency_hz * fft_size as f64 / sample_rate as f64).round();
    if cycles < 1.0 {
        return Err(format!(
            "frequency {frequency_hz} Hz at {sample_rate} Hz with fft_size {fft_size} \
             does not fit a full cycle; lower the frequency or raise fft_size"
        ));
    }
    let coherent_freq = cycles * sample_rate as f64 / fft_size as f64;

    // --- 生成信号（Rust 侧完成，前端不参与）---
    let signal: Vec<f32> = (0..fft_size)
        .map(|i| {
            let phase = std::f64::consts::TAU * coherent_freq * i as f64 / sample_rate as f64;
            (amplitude as f64 * phase.sin()) as f32
        })
        .collect();

    // --- 标量指标 ---
    let stats = analyze_channel(&signal, ClippingConfig::default());

    // --- 频谱：加窗 + FFT + 降采样到目标点数（ADR-018 规则 3）---
    let window = WindowFunction::Hann;
    let coeffs = window.coefficients(fft_size);
    // rustfft requires Complex<f32> buffers for in-place transforms; real input
    // goes in the real part. Windowing is folded in here, so no second copy.
    let mut buf: Vec<Complex<f32>> = signal
        .iter()
        .zip(coeffs.iter())
        .map(|(s, w)| Complex::new(*s * *w as f32, 0.0))
        .collect();

    let mut planner = rustfft::FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buf);

    // Single-sided amplitude spectrum with coherent-gain correction.
    // DC and Nyquist bins are NOT doubled (DSP_SPEC 5).
    let w_sum: f64 = coeffs.iter().sum();
    let half = fft_size / 2 + 1;
    let mut amps = vec![0.0_f32; half];
    for (k, c) in buf.iter().take(half).enumerate() {
        let mag = (c.re * c.re + c.im * c.im).sqrt() as f64;
        let a = if k == 0 || k == fft_size / 2 {
            mag / w_sum
        } else {
            2.0 * mag / w_sum
        };
        amps[k] = a as f32;
    }

    // 降采样：把 half 个 bin 聚合成 target_bins 个点，取每组最大值。
    // 降采样在 Rust 侧完成，前端收到的就是最终要画的点数。
    let bins = target_bins.clamp(1, half);
    let mut db = vec![f32::NEG_INFINITY; bins];
    let bin_hz = sample_rate as f64 / fft_size as f64;
    for (i, slot) in db.iter_mut().enumerate() {
        let lo = i * half / bins;
        let hi = (((i + 1) * half) / bins).max(lo + 1).min(half);
        let peak = amps[lo..hi].iter().copied().fold(0.0_f32, f32::max);
        *slot = if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            f32::NEG_INFINITY
        };
    }

    // --- 波形包络：min/max 降采样到目标点数 ---
    let points = envelope_points.clamp(1, fft_size);
    let mut env = vec![0.0_f32; points];
    let per = fft_size / points;
    for (i, slot) in env.iter_mut().enumerate() {
        let lo = i * per;
        let hi = ((i + 1) * per).min(fft_size);
        if lo >= hi {
            continue;
        }
        let mut mx = f32::NEG_INFINITY;
        let mut mn = f32::INFINITY;
        for v in &signal[lo..hi] {
            mx = mx.max(*v);
            mn = mn.min(*v);
        }
        // 单声道时画幅值；双声道由后续 M2 的 PcmBlock 支持
        *slot = (mx + mn) * 0.5;
    }

    // --- 编码为二进制（前端收到 Uint8Array 而非 JSON 数组）---
    let spectrum_bytes = f32_slice_to_le_bytes(&db);
    let envelope_bytes = f32_slice_to_le_bytes(&env);
    let compute_micros = start.elapsed().as_micros() as u64;

    Ok(SyntheticAnalysis {
        metrics: SyntheticMetrics {
            sample_rate,
            sample_count: stats.sample_count,
            peak_dbfs: Some(
                stats
                    .peak
                    .dbfs()
                    .clamped(auralis_core::units::LINEAR_TO_DB_FLOOR),
            ),
            rms_dbfs: Some(
                stats
                    .rms
                    .dbfs()
                    .clamped(auralis_core::units::LINEAR_TO_DB_FLOOR),
            ),
            dc: stats.dc.value,
            crest_db: if stats.crest_factor_db().is_finite() {
                Some(stats.crest_factor_db())
            } else {
                None
            },
            over_full_scale_samples: stats.over_full_scale_samples,
            clipping_regions: stats.clipping.regions,
            sanitized_samples: stats.sanitized_samples,
        },
        spectrum_db: spectrum_bytes,
        spectrum_start_hz: 0.0,
        spectrum_step_hz: (bin_hz * half as f64 / bins as f64) as f32,
        spectrum_bins: bins,
        waveform_envelope: envelope_bytes,
        waveform_points: points,
        compute_micros,
    })
}

/// `&[f32]` -> 小端字节序列。WebView 端用 `new Float32Array(buf)` 还原。
///
/// 显式按小端编码，不依赖宿主字节序，保证跨机器一致。
fn f32_slice_to_le_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_encoding_is_platform_independent() {
        let bytes = f32_slice_to_le_bytes(&[1.0_f32]);
        assert_eq!(bytes.len(), 4);
        assert_eq!(&bytes, &[0x00, 0x00, 0x80, 0x3F]);
    }

    #[test]
    fn empty_slice_encodes_to_empty_bytes() {
        assert!(f32_slice_to_le_bytes(&[]).is_empty());
    }

    #[test]
    fn rejects_non_power_of_two_fft_size() {
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 1000, 512, 256);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_zero_sample_rate() {
        let r = analyze_synthetic_signal(1000.0, 0.5, 0, 1024, 256, 128);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_frequency_that_does_not_fit_a_cycle() {
        // 256 点 @ 48 kHz -> 187.5 Hz/bin，取整为 0 的条件是 freq < 93.75 Hz。
        //   50 Hz : 50*256/48000 = 0.267 -> round = 0 -> 拒绝
        //   100 Hz: 100*256/48000 = 0.533 -> round = 1 -> 接受（半个 bin 仍算 1 周期）
        // 所以反例必须是 50 Hz 而不是 100 Hz。
        let r = analyze_synthetic_signal(50.0, 0.5, 48_000, 256, 64, 32);
        assert!(r.is_err(), "50 Hz 在 256 点下放不下一个完整周期，应当拒绝");
        assert!(
            r.unwrap_err().contains("does not fit a full cycle"),
            "错误信息应说明原因"
        );
    }

    #[test]
    fn accepts_frequency_that_fits_whole_cycles() {
        // 与上一个测试对照：1000 Hz 在 256 点下容纳 5.33 -> 5 个整周期，应当通过
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 256, 64, 32);
        assert!(r.is_ok(), "1000 Hz 应被接受");
    }

    #[test]
    fn bin_rounding_boundary_is_at_half_a_bin() {
        // 边界恰好在半个 bin 处：fft_size*fs 的关系下
        //   93 Hz : 93 * 256 / 48000  = 0.4960 -> round = 0 -> 拒绝
        //   94 Hz : 94 * 256 / 48000  = 0.5013 -> round = 1 -> 接受
        // 临界频率 = 0.5 * fs / fft_size = 0.5 * 48000 / 256 = 93.75 Hz
        assert!(
            analyze_synthetic_signal(93.0, 0.5, 48_000, 256, 64, 32).is_err(),
            "93 Hz = 0.496 bin，取整为 0，应当拒绝"
        );
        assert!(
            analyze_synthetic_signal(94.0, 0.5, 48_000, 256, 64, 32).is_ok(),
            "94 Hz = 0.501 bin，取整为 1，应当接受"
        );
    }

    #[test]
    fn produces_correct_byte_lengths() {
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 4096, 800, 1000).unwrap();
        assert_eq!(r.spectrum_db.len(), 800 * 4);
        assert_eq!(r.waveform_envelope.len(), 1000 * 4);
        assert_eq!(r.spectrum_bins, 800);
        assert_eq!(r.waveform_points, 1000);
    }

    #[test]
    fn coherent_sine_peak_lands_at_expected_frequency() {
        // 1000 Hz @ 48 kHz，4096 点 -> bin 85.33，取整到 85 -> 995.6 Hz
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 4096, 4096, 512).unwrap();
        let db = decode_f32s(&r.spectrum_db);
        let peak_bin = db
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .expect("应有有限峰值");
        let hz = f64::from(r.spectrum_step_hz) * peak_bin as f64;
        // 相干采样后峰值频率误差应在 1 bin 内
        assert!(
            (hz - 1000.0).abs() <= f64::from(r.spectrum_step_hz),
            "peak at {hz} Hz"
        );
    }

    #[test]
    fn amplitude_scaling_is_corrected() {
        // 满刻度正弦的主峰应读到 ~0 dBFS
        let r = analyze_synthetic_signal(1000.0, 1.0, 48_000, 4096, 4096, 256).unwrap();
        let db = decode_f32s(&r.spectrum_db);
        let peak = db
            .iter()
            .filter(|v| v.is_finite())
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            peak.abs() < 1.0,
            "Hann 相干增益校正后主峰应接近 0 dBFS，实得 {peak}"
        );
    }

    #[test]
    fn downsampling_never_exceeds_source_resolution() {
        // 目标点数大于可用 bin 时应被钳到 half
        let r = analyze_synthetic_signal(1000.0, 0.5, 48_000, 1024, 100_000, 64).unwrap();
        assert!(r.spectrum_bins <= 1024 / 2 + 1);
        assert_eq!(r.spectrum_db.len(), r.spectrum_bins * 4);
    }

    /// 测试辅助：把 little-endian 字节解回 f32。
    fn decode_f32s(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    }
}
