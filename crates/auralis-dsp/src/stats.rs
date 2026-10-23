//! 基础信号指标：Peak、RMS、DC Offset、削波特征。
//!
//! `DSP_SPEC.md` §3、§4、§7 的落地。
//!
//! ## 削波为什么要拆成两个指标
//!
//! `DSP_SPEC.md` §7 与 `PRD.md` §8 明确：**「数字样本峰值达到或超过阈值不等于能够
//! 证明模拟链路发生削波」**。因此本模块把两件事分开报告：
//!
//! 1. [`SignalStats::over_full_scale_samples`] — `|x| > 1.0` 的样本数。这是数字域事实。
//! 2. [`ClippingFeatures`] — 连续贴近极值的「平顶」片段。这是**启发式特征**，
//!    同样可能由限幅器、量化或过载的模拟前端产生，不能单独作为削波证据。
//!
//! UI 层必须把两者显示为两行独立指标，并在第二行标注「启发式」。

use auralis_core::units::{Dbfs, linear_to_dbfs};

/// 数字峰值：`|x|` 的最大值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakLevel {
    /// 峰值幅度，线性，满刻度 ±1.0。
    pub amplitude: f32,
    /// 峰值出现处的样本下标（块内，相对本块起始帧）。
    pub sample_index: usize,
    /// 是否存在 `|x| > 1.0` 的样本。
    ///
    /// `DSP_SPEC.md` §2：「超出 [-1, 1] 的浮点样本可合法存在于处理链中，不能默认裁剪。」
    pub over_full_scale: bool,
}

impl PeakLevel {
    /// 峰值幅度的 dBFS 表示。
    #[must_use]
    pub fn dbfs(&self) -> Dbfs {
        Dbfs::from_linear(self.amplitude)
    }

    /// 人类可读的单行描述。
    #[must_use]
    pub fn describe(&self) -> String {
        let db = self.dbfs().clamped(auralis_core::units::LINEAR_TO_DB_FLOOR);
        let mut s = format!("{db:.2} dBFS");
        if self.over_full_scale {
            s.push_str(" (超量程)");
        }
        s
    }
}

/// 均方根值。
///
/// `DSP_SPEC.md` §3：`RMS = sqrt(Σ x[n]² / N)`。
/// 统计窗口由调用方决定；本类型的文档化窗口语义是「传给本函数的整个样本序列」。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rms {
    /// RMS 线性值，满刻度 ±1.0。
    pub value: f32,
    /// 参与计算的样本数。
    pub sample_count: usize,
}

impl Rms {
    /// RMS 的 dBFS 表示。
    #[must_use]
    pub fn dbfs(&self) -> Dbfs {
        Dbfs::from_linear(self.value)
    }
}

/// 直流偏移：通道均值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DcOffset {
    /// 线性均值，满刻度 ±1.0。可能是负值。
    pub value: f32,
    /// 参与计算的样本数。
    pub sample_count: usize,
}

impl DcOffset {
    /// 直流量的 dBFS 表示（取绝对值，因为 dBFS 只对幅度定义）。
    #[must_use]
    pub fn dbfs(&self) -> Dbfs {
        Dbfs::from_linear(self.value.abs())
    }
}

/// 削波特征（启发式）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClippingFeatures {
    /// 连续达到阈值的片段数量。
    pub regions: u32,
    /// 所有片段累计长度，样本数。
    pub total_samples: usize,
    /// 最长片段长度，样本数。
    pub longest_region_samples: usize,
    /// 最长片段起始处的样本下标。
    pub longest_region_start: usize,
}

/// 跨解码块累积的平顶检测器。
///
/// 离线分析是**流式**的：文件按块解码，一个平顶片段可能横跨两个块。
/// [`detect_flat_top`] 只看一个切片，如果在每个块上分别调用，跨块的那一段
/// 就会被切成两段，甚至因为两段都短于 `min_run` 而完全漏掉。
///
/// 本类型为每个声道保留「当前连续段」的状态，调用方只需按块喂入
/// **交错**样本，最后调用 [`finish`](Self::finish)。
#[derive(Debug, Clone)]
pub struct StreamingFlatTop {
    config: ClippingConfig,
    channels: usize,
    regions: u32,
    total_samples: usize,
    longest_samples: usize,
    longest_start: usize,
    /// 每个声道当前连续段的长度。
    run_len: Vec<usize>,
    /// 每个声道当前连续段的起始样本（该声道自己的样本计数）。
    run_start: Vec<u64>,
    /// 每个声道已经喂入的样本数。
    seen: Vec<u64>,
}

impl StreamingFlatTop {
    /// 新建检测器。`channels == 0` 会按 1 个声道处理。
    #[must_use]
    pub fn new(channels: usize, config: ClippingConfig) -> Self {
        let channels = channels.max(1);
        Self {
            config,
            channels,
            regions: 0,
            total_samples: 0,
            longest_samples: 0,
            longest_start: 0,
            run_len: vec![0; channels],
            run_start: vec![0; channels],
            seen: vec![0; channels],
        }
    }

    /// 喂入一块**交错**样本（`L0,R0,L1,R1,…`）。
    ///
    /// 非有限样本按 0 处理，与 [`analyze_channel`] 的清理策略一致。
    pub fn push_interleaved(&mut self, samples: &[f32]) {
        for (i, &raw) in samples.iter().enumerate() {
            let ch = i % self.channels;
            let a = if raw.is_finite() { raw.abs() } else { 0.0 };
            if a >= self.config.threshold {
                if self.run_len[ch] == 0 {
                    self.run_start[ch] = self.seen[ch];
                }
                self.run_len[ch] += 1;
            } else {
                self.flush(ch);
            }
            self.seen[ch] += 1;
        }
    }

    /// 结束并返回累计特征。
    ///
    /// 最长的片段可能位于任意声道；`longest_region_start` 是**该声道内**
    /// 的样本下标，跨声道比较时不要把它当成全局交错下标。
    #[must_use]
    pub fn finish(mut self) -> ClippingFeatures {
        for ch in 0..self.channels {
            self.flush(ch);
        }
        ClippingFeatures {
            regions: self.regions,
            total_samples: self.total_samples,
            longest_region_samples: self.longest_samples,
            longest_region_start: self.longest_start,
        }
    }

    fn flush(&mut self, ch: usize) {
        let len = self.run_len[ch];
        if len >= self.config.min_run {
            self.regions += 1;
            self.total_samples += len;
            if len > self.longest_samples {
                self.longest_samples = len;
                self.longest_start = self.run_start[ch] as usize;
            }
        }
        self.run_len[ch] = 0;
    }
}

impl ClippingFeatures {
    /// 是否检测到疑似平顶。
    #[must_use]
    pub const fn is_suspicious(&self) -> bool {
        self.regions > 0
    }

    /// 人类可读的单行描述。必须带上「启发式」标记。
    #[must_use]
    pub fn describe(&self) -> String {
        if !self.is_suspicious() {
            return "未检测到疑似平顶".to_string();
        }
        format!(
            "疑似平顶 {} 段 / 共 {} 样本 / 最长 {} 样本 @ {}（启发式，非模拟削波证据）",
            self.regions,
            self.total_samples,
            self.longest_region_samples,
            self.longest_region_start
        )
    }
}

/// 单通道信号统计。
#[derive(Debug, Clone, PartialEq)]
pub struct SignalStats {
    /// 参与统计的样本数。
    pub sample_count: usize,
    /// 数字峰值。
    pub peak: PeakLevel,
    /// RMS。
    pub rms: Rms,
    /// 直流偏移。
    pub dc: DcOffset,
    /// 超量程样本数（`|x| > 1.0`）。
    pub over_full_scale_samples: usize,
    /// 削波特征。
    pub clipping: ClippingFeatures,
    /// 非有限样本（NaN / ±Inf）被替换为 0 的个数。
    ///
    /// `DSP_SPEC.md` §2：「NaN/Inf 的处理策略必须显式定义：拒绝、标记或清理，
    /// 不允许静默传播。」本实现选择**清理 + 计数**，因为拒绝会让整个文件无法分析，
    /// 而静默传播会污染所有下游指标。
    pub sanitized_samples: usize,
}

impl SignalStats {
    /// 波形因数，dB。
    ///
    /// `CrestFactor = 20·log10(peak / rms)`。静音声义为 `-inf`，由调用方决定如何显示。
    #[must_use]
    pub fn crest_factor_db(&self) -> f32 {
        if self.rms.value == 0.0 {
            f32::NEG_INFINITY
        } else {
            linear_to_dbfs(self.peak.amplitude / self.rms.value)
        }
    }
}

/// 削波检测阈值配置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClippingConfig {
    /// 判定为「贴近满刻度」的阈值。
    ///
    /// 默认 0.999 而非 1.0：浮点解码误差、24-bit 文件归一化都会让
    /// 未削波样本略微越过 1.0。
    pub threshold: f32,
    /// 构成一个「片段」所需的最少连续样本数。
    ///
    /// 默认 4：单样本越界更可能是解码噪声而非真实平顶。
    pub min_run: usize,
}

impl Default for ClippingConfig {
    fn default() -> Self {
        Self {
            threshold: 0.999,
            min_run: 4,
        }
    }
}

/// 计算单通道（已去交织）的统计量。
///
/// # 数值行为
///
/// - 非有限样本（NaN / ±Inf）被替换为 0，并计入 `sanitized_samples`。
/// - 空输入返回各指标为「零样本」的合法结构，不 panic、不返回错误——
///   静音文件是正常输入，不是错误（`DSP_SPEC.md` §3）。
///
/// # 复杂度
///
/// 单遍 O(n)。使用 **Welford 在线算法**求和方与均值，避免二次累加导致的精度损失。
#[must_use]
pub fn analyze_channel(samples: &[f32], clipping: ClippingConfig) -> SignalStats {
    let n = samples.len();
    let mut peak_amp = 0.0_f32;
    let mut peak_idx = 0_usize;
    let mut mean = 0.0_f64; // Welford
    let mut m2 = 0.0_f64;
    let mut over_fs = 0_usize;
    let mut sanitized = 0_usize;

    for (i, &raw) in samples.iter().enumerate() {
        let x = if raw.is_finite() {
            raw
        } else {
            sanitized += 1;
            0.0
        };
        let a = x.abs();
        if a > peak_amp {
            peak_amp = a;
            peak_idx = i;
        }
        if a > 1.0 {
            over_fs += 1;
        }
        let xf = x as f64;
        let delta = xf - mean;
        mean += delta / (i as f64 + 1.0);
        m2 += delta * (xf - mean);
    }

    // RMS = sqrt(Σx²/N)。用 Welford 的 m2 加回均值项避免灾难性抵消：
    // Σx² = m2 + N·mean²
    let variance = if n > 0 {
        (m2 + n as f64 * mean * mean) / n as f64
    } else {
        0.0
    };
    let rms = if variance > 0.0 {
        variance.sqrt() as f32
    } else {
        0.0
    };

    SignalStats {
        sample_count: n,
        peak: PeakLevel {
            amplitude: peak_amp,
            sample_index: peak_idx,
            over_full_scale: over_fs > 0,
        },
        rms: Rms {
            value: rms,
            sample_count: n,
        },
        dc: DcOffset {
            value: mean as f32,
            sample_count: n,
        },
        over_full_scale_samples: over_fs,
        clipping: detect_flat_top(samples, clipping),
        sanitized_samples: sanitized,
    }
}

/// 检测连续贴近满刻度的「平顶」片段。
///
/// 这是**启发式**：它只能说明数字样本里存在连续极值，不能证明模拟链路削波。
/// 阈值与最短连续长度都可在 [`ClippingConfig`] 中调整。
#[must_use]
pub fn detect_flat_top(samples: &[f32], config: ClippingConfig) -> ClippingFeatures {
    let mut features = ClippingFeatures::default();
    let mut run_start = 0_usize;
    let mut run_len = 0_usize;

    for (i, &raw) in samples.iter().enumerate() {
        let a = if raw.is_finite() { raw.abs() } else { 0.0 };
        if a >= config.threshold {
            if run_len == 0 {
                run_start = i;
            }
            run_len += 1;
        } else {
            features.absorb_run(run_start, run_len, config.min_run);
            run_len = 0;
        }
    }
    features.absorb_run(run_start, run_len, config.min_run);
    features
}

impl ClippingFeatures {
    fn absorb_run(&mut self, start: usize, len: usize, min_run: usize) {
        if len >= min_run {
            self.regions += 1;
            self.total_samples += len;
            if len > self.longest_region_samples {
                self.longest_region_samples = len;
                self.longest_region_start = start;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 满刻度正弦 `A·sin(2πft)`，逐样本可复现。
    fn sine(freq_hz: f64, amp: f32, sample_rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let phase = std::f64::consts::TAU * freq_hz * i as f64 / f64::from(sample_rate);
                (amp as f64 * phase.sin()) as f32
            })
            .collect()
    }

    const FS: u32 = 48_000;

    #[test]
    fn rms_of_sine_is_amplitude_over_sqrt_two() {
        // RMS = A/√2
        let x = sine(1_000.0, 0.5, FS, 48_000);
        let s = analyze_channel(&x, ClippingConfig::default());
        let expected = 0.5 / std::f32::consts::SQRT_2;
        assert!(
            (s.rms.value - expected).abs() < 1e-4,
            "got {} expected {expected}",
            s.rms.value
        );
        // 0.5/√2 = 0.35355 -> 20log10(0.35355) = -9.0309 dBFS。
        // 注意这不是 -3.01 dB：-3.01 dB 是正弦的波峰因数（peak 与 rms 之差），
        // 而不是 RMS 的绝对电平。A 自身是 -6.02 dBFS，-6.02 - 3.01 = -9.03。
        let db = s.rms.dbfs().clamped(-140.0);
        assert!((db - -9.030_9).abs() < 0.01, "got {db} dBFS");
    }

    #[test]
    fn peak_of_sine_tracks_amplitude() {
        // 整数个周期，采样点必然落在波峰上
        let x = sine(1_000.0, 0.5, FS, 4_800);
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!((s.peak.amplitude - 0.5).abs() < 1e-3);
        assert!(!s.peak.over_full_scale);
        assert!((s.peak.dbfs().clamped(-140.0) - -6.020_6).abs() < 0.02);
    }

    #[test]
    fn dc_offset_is_the_mean() {
        let x = vec![0.1_f32; 100];
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!((s.dc.value - 0.1).abs() < 1e-6);
        assert!((s.dc.dbfs().clamped(-140.0) - -20.0).abs() < 1e-3);
    }

    #[test]
    fn dc_offset_is_signed() {
        let x = vec![-0.25_f32; 64];
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!((s.dc.value + 0.25).abs() < 1e-6, "直流分量必须保留符号");
    }

    #[test]
    fn crest_factor_of_full_scale_sine_is_about_3_01_db() {
        // crest = 20log10(A / (A/√2)) = 20log10(√2) = 3.0103 dB
        let x = sine(1_000.0, 1.0, FS, 48_000);
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!(
            (s.crest_factor_db() - 3.010_3).abs() < 0.02,
            "got {}",
            s.crest_factor_db()
        );
    }

    #[test]
    fn silence_produces_zeroes_not_nan() {
        let x = vec![0.0_f32; 1024];
        let s = analyze_channel(&x, ClippingConfig::default());
        assert_eq!(s.peak.amplitude, 0.0);
        assert_eq!(s.rms.value, 0.0);
        assert_eq!(s.dc.value, 0.0);
        assert!(s.rms.value.is_finite());
        assert!(s.dc.value.is_finite());
        // 静音声义为 -inf，而不是一个假装精确的大数字
        assert!(s.rms.dbfs().is_silence());
        assert!(s.crest_factor_db().is_infinite());
        assert!(!s.clipping.is_suspicious());
    }

    #[test]
    fn empty_input_is_not_an_error() {
        let s = analyze_channel(&[], ClippingConfig::default());
        assert_eq!(s.sample_count, 0);
        assert_eq!(s.peak.amplitude, 0.0);
        assert_eq!(s.rms.value, 0.0);
        assert!(s.rms.value.is_finite());
    }

    #[test]
    fn non_finite_samples_are_sanitized_and_counted() {
        let mut x = vec![0.1_f32; 10];
        x[3] = f32::NAN;
        x[7] = f32::INFINITY;
        x[8] = f32::NEG_INFINITY;
        let s = analyze_channel(&x, ClippingConfig::default());
        assert_eq!(s.sanitized_samples, 3);
        assert!(s.rms.value.is_finite());
        assert!(s.dc.value.is_finite());
        assert!(s.peak.amplitude.is_finite());
        // 3 个样本被清理为 0，均值应为 7 * 0.1 / 10 = 0.07
        assert!((s.dc.value - 0.07).abs() < 1e-6, "got {}", s.dc.value);
    }

    #[test]
    fn over_full_scale_is_counted_not_clipped() {
        // 1.5 倍满刻度：DSP_SPEC §2 说不允许默认裁剪
        let x = sine(1_000.0, 1.5, FS, 4_800);
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!(s.peak.over_full_scale);
        // 峰值必须如实报告 1.5，而不是被裁到 1.0
        assert!(
            (s.peak.amplitude - 1.5).abs() < 1e-3,
            "峰值被裁剪了：{}",
            s.peak.amplitude
        );
        assert!(
            s.over_full_scale_samples > 0,
            "应当统计出超量程样本，而不是裁剪掉"
        );
        assert!(s.sanitized_samples == 0);
    }

    #[test]
    fn flat_topped_waveform_is_detected_as_suspicious() {
        // 直接构造带平顶的信号，而不是"正弦再 clamp"。
        // 原因：1 kHz @ 48 kHz 每周期只有 48 个采样点，clamp 到 0.999 后
        // 只有一个采样点恰好落在阈值上，达不到 min_run = 4，测不出真实情况。
        let mut x: Vec<f32> = Vec::new();
        x.extend((0..10).map(|i| -1.0 + 0.2 * i as f32)); // 上升段 -1.0 .. 0.8
        x.extend(std::iter::repeat_n(0.999_f32, 12)); // 正平顶
        x.extend((0..10).rev().map(|i| 0.8 - 0.18 * i as f32)); // 下降段
        x.extend(std::iter::repeat_n(-0.999_f32, 12)); // 负平顶

        let s = analyze_channel(&x, ClippingConfig::default());
        assert_eq!(s.clipping.regions, 2, "{}", s.clipping.describe());
        assert_eq!(s.clipping.longest_region_samples, 12);
        assert_eq!(s.clipping.longest_region_start, 10);
        assert_eq!(s.clipping.total_samples, 24);
        // 平顶不等于超量程：没有任何样本的绝对值超过 1.0
        assert_eq!(s.over_full_scale_samples, 0);
        assert!(!s.peak.over_full_scale);
        assert!(s.clipping.describe().contains("启发式"));
    }

    #[test]
    fn streaming_flat_top_counts_a_run_split_across_blocks_once() {
        // 一个 12 样本的平顶，被块边界切成 3 段。逐块调用 detect_flat_top
        // 会得到 0 段（每段都短于 min_run=4）；流式检测器必须仍然看到 1 段。
        let mut x: Vec<f32> = Vec::new();
        x.extend((0..10).map(|i| -1.0 + 0.2 * i as f32));
        x.extend(std::iter::repeat_n(0.999_f32, 12));
        x.extend((0..10).rev().map(|i| 0.8 - 0.18 * i as f32));

        let expected = detect_flat_top(&x, ClippingConfig::default());
        assert_eq!(expected.regions, 1);

        let mut streaming = StreamingFlatTop::new(1, ClippingConfig::default());
        for block in x.chunks(5) {
            streaming.push_interleaved(block);
        }
        let got = streaming.finish();
        assert_eq!(got.regions, expected.regions, "跨块平顶只能计一次");
        assert_eq!(got.total_samples, expected.total_samples);
        assert_eq!(got.longest_region_samples, expected.longest_region_samples);
        assert_eq!(got.longest_region_start, expected.longest_region_start);
    }

    #[test]
    fn streaming_flat_top_keeps_channels_separate() {
        // L 平顶、R 干净的立体声：只能报 1 段，不能把 R 的样本也并进来。
        let mut interleaved = Vec::new();
        for i in 0..20 {
            let l = if (6..14).contains(&i) { 0.999 } else { 0.0 };
            interleaved.push(l);
            interleaved.push(0.0);
        }
        let mut tracker = StreamingFlatTop::new(2, ClippingConfig::default());
        tracker.push_interleaved(&interleaved);
        let got = tracker.finish();
        assert_eq!(got.regions, 1);
        assert_eq!(got.total_samples, 8);
        assert_eq!(got.longest_region_start, 6, "起始下标是 L 声道内的下标");
    }

    #[test]
    fn clipped_sine_at_low_frequency_shows_flat_top() {
        // 低频使 clamp 后的平顶跨越多个采样点，更接近真实使用场景
        let x: Vec<f32> = (0..48_000)
            .map(|i| {
                let v = (std::f64::consts::TAU * 50.0 * i as f64 / FS as f64).sin() as f32;
                v.clamp(-0.99, 0.99)
            })
            .collect();
        let s = analyze_channel(
            &x,
            ClippingConfig {
                threshold: 0.95,
                min_run: 4,
            },
        );
        assert!(
            s.clipping.is_suspicious(),
            "低频硬削波应被检出：{}",
            s.clipping.describe()
        );
        // 0.99 < 1.0，仍属数字域内的平顶而非超量程
        assert_eq!(s.over_full_scale_samples, 0);
    }

    #[test]
    fn clean_sine_is_not_reported_as_clipping() {
        let x = sine(1_000.0, 0.8, FS, 48_000);
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!(!s.clipping.is_suspicious(), "{}", s.clipping.describe());
        assert_eq!(s.clipping.regions, 0);
    }

    #[test]
    fn min_run_filters_single_sample_spikes() {
        // 单个越界样本不构成片段
        let mut x = vec![0.0_f32; 100];
        x[50] = 1.0;
        let f = detect_flat_top(
            &x,
            ClippingConfig {
                threshold: 0.999,
                min_run: 4,
            },
        );
        assert_eq!(f.regions, 0);

        // 连续 4 个则构成
        let mut y = vec![0.0_f32; 100];
        y[50..54].fill(1.0);
        let f2 = detect_flat_top(
            &y,
            ClippingConfig {
                threshold: 0.999,
                min_run: 4,
            },
        );
        assert_eq!(f2.regions, 1);
        assert_eq!(f2.longest_region_samples, 4);
        assert_eq!(f2.longest_region_start, 50);
    }

    #[test]
    fn channel_isolation_l_and_r_are_independent() {
        let l = sine(1_000.0, 1.0, FS, 4_800);
        let r: Vec<f32> = l.iter().map(|s| s * 0.3).collect();
        let sl = analyze_channel(&l, ClippingConfig::default());
        let sr = analyze_channel(&r, ClippingConfig::default());

        assert!((sl.peak.amplitude - 1.0).abs() < 1e-3);
        assert!((sr.peak.amplitude - 0.3).abs() < 1e-3);
        // 两声道独立计算，互不影响
        assert!(sl.peak.amplitude > sr.peak.amplitude);
    }

    #[test]
    fn rms_is_numerically_stable_for_tiny_signals() {
        // 朴素 sum(x²)/N 在极小信号上会下溢；Welford 不应
        let x = vec![1e-20_f32; 100_000];
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!((s.rms.value - 1e-20).abs() < 1e-25, "got {}", s.rms.value);
    }

    #[test]
    fn constant_dc_signal_has_zero_rms_error() {
        // 常量信号的 RMS 应恰好等于常量值，验证 m2 + N·mean² 的重构无抵消误差
        let x = vec![0.3_f32; 10_000];
        let s = analyze_channel(&x, ClippingConfig::default());
        assert!((s.rms.value - 0.3).abs() < 1e-6, "got {}", s.rms.value);
        assert!((s.dc.value - 0.3).abs() < 1e-6);
    }
}
