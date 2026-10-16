//! FFT 频谱与峰值检测。
//!
//! 本模块实现 [`docs/DSP_SPEC.md`] §5 的全部要求，并遵循
//! `docs/superpowers/specs/2026-10-14-auralis-studio-design.md` §5.3 的归一化约定。
//!
//! ## 三种「谱」必须分清
//!
//! 设 `x[n]` 为实数输入，`w[n]` 为 N 点周期型窗，`y[n] = x[n]·w[n]`，
//! `X[k] = FFT(y)`，`W = Σ w[n]`。取单边 `k = 0 … N/2`：
//!
//! ```text
//! 幅值谱 Amplitude (单位 FS)
//!   A[k] = |X[k]| / W        k = 0 或 N/2
//!   A[k] = 2·|X[k]| / W       0 < k < N/2
//!
//! 功率谱 Power (单位 FS²)
//!   P[k] = A[k]²             k = 0 或 N/2
//!   P[k] = A[k]² / 2         0 < k < N/2
//!
//! 功率谱密度 PSD (单位 FS²/Hz)
//!   S[k] = P[k] / ENBW_Hz
//! ```
//!
//! ### 为什么 DC 与 Nyquist 不乘 2
//!
//! 实信号频谱左右共轭，单边谱把两侧能量合并到一半。但 `k=0` 与 `k=N/2`
//! 是**自共轭**的——它们没有镜像伙伴。若也乘 2，就会把直流与 Nyquist
//! 分量凭空放大一倍。
//!
//! ### 相干增益校正为什么必需
//!
//! 窗函数的系数和 `W = N·CG` 小于 `N`（Hann 的 CG = 0.5）。
//! 若不除以 `W`，Hann 窗下的满刻度正弦只会读到 −6 dBFS。
//! 除以 `W` 后，满刻度正弦读到 1.0 → **0 dBFS**，
//! 这正是「幅值谱读数 = 正弦幅值」的含义。
//!
//! ### PSD 的归一化
//!
//! `ENBW_Hz = fs · Σw² / W²`。对白噪声（方差 σ²）应满足 Parseval：
//! `Σ S[k]·Δf ≈ σ²`。该等式是 PSD 归一化正确的充分证据，
//! 已在 `psd_satisfies_parseval_for_white_noise` 中作为测试固定下来。

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use auralis_core::units::{Dbfs, LINEAR_TO_DB_FLOOR};

use crate::window::WindowFunction;

impl WindowFunction {
    /// 系数和 `W = Σ w[n]`，相位校正系数。
    ///
    /// 幅值谱的校正因子就是 `1/W` 与 `2/W`，因此这个常数值得单独缓存。
    #[must_use]
    pub fn coefficient_sum(self, size: usize) -> f64 {
        assert!(size > 0, "window size must be greater than zero");
        self.coefficients(size).iter().sum()
    }
}

/// 频谱计算参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectrumConfig {
    /// FFT 长度 N。必须 > 0。
    pub fft_size: usize,
    /// 帧移（hop）。必须在 `1..=fft_size`。
    pub hop_size: usize,
    /// 窗函数。
    pub window: WindowFunction,
    /// 是否在加窗前减去直流分量。
    ///
    /// 默认 `false`：去直流会改变数据，`DSP_SPEC.md` 要求
    /// 「DC Offset 与频谱 bin0 的值分别展示」，因此不默认处理。
    pub remove_dc: bool,
}

impl Default for SpectrumConfig {
    /// 4096 点、75% 重叠、Hann 窗——专业音频工具的常用默认。
    fn default() -> Self {
        Self {
            fft_size: 4096,
            hop_size: 1024,
            window: WindowFunction::Hann,
            remove_dc: false,
        }
    }
}

impl SpectrumConfig {
    /// 校验参数。
    ///
    /// # Errors
    /// `fft_size == 0`、`hop_size == 0`、`hop_size > fft_size`。
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.fft_size == 0 {
            return Err(ConfigError::FftSizeZero);
        }
        if self.hop_size == 0 {
            return Err(ConfigError::HopSizeZero);
        }
        if self.hop_size > self.fft_size {
            return Err(ConfigError::HopExceedsFrame {
                hop: self.hop_size,
                fft: self.fft_size,
            });
        }
        Ok(())
    }
}

/// 频谱参数错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// FFT 长度为 0。
    #[error("fft_size must be greater than zero")]
    FftSizeZero,
    /// 帧移为 0。
    #[error("hop_size must be greater than zero")]
    HopSizeZero,
    /// 帧移超过帧长。
    #[error("hop_size ({hop}) must not exceed fft_size ({fft})")]
    HopExceedsFrame {
        /// 请求的帧移。
        hop: usize,
        /// 请求的帧长。
        fft: usize,
    },
    /// 输入样本不足一帧。
    #[error("need at least {needed} samples for one frame, have {available}")]
    InputTooShort {
        /// 需要。
        needed: usize,
        /// 实际。
        available: usize,
    },
}

/// 一个检测到的谱峰。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// 插值后的频率，Hz。
    pub frequency_hz: f64,
    /// 插值后的幅值，dBFS。
    pub amplitude_dbfs: f32,
    /// 插值后的幅值，线性（满刻度 1.0）。
    pub amplitude: f32,
    /// 峰值所在的整数 bin 索引。
    pub bin_index: usize,
    /// 插值偏移，范围 `[-0.5, 0.5]`。
    pub bin_offset: f32,
    /// **未插值**的 bin 频率，Hz。保留它是为了让用户能验证插值没跑偏
    /// （`DSP_SPEC.md` §5：「峰值插值……不能替代原始 bin 数据」）。
    pub bin_frequency_hz: f64,
    /// 抛物线插值后的幅值，dBFS。
    pub interpolated_dbfs: f32,
    /// 主瓣能量占比（本 bin ±1 占总能量之比）。仅供参考。
    ///
    /// 实测结论：纯正弦信号下，相干与非相干的该值几乎相同
    /// （1.000 vs 0.980），因此**不足以**判定插值可靠性。
    pub peak_concentration: f32,
}

impl Peak {
    /// 扇贝损失（scalloping loss），dB。
    ///
    /// = bin 读数与插值读数之差。实测非相干信号最大读数误差出现在频率落在
    /// 两个 bin 正中间时，约 1.4 dB。
    #[must_use]
    pub fn scalloping_db(&self) -> f32 {
        self.amplitude_dbfs - self.interpolated_dbfs
    }

    /// 该峰读数是否应标注为「估算」。
    ///
    /// 判据是**扇贝损失**而非能量集中度：实测纯正弦无论相干与否，
    /// 主瓣能量都高度集中（0.98–1.00），用集中度判断会永远返回 false；
    /// 而 bin 读数与插值读数之差能真实反映读数不可靠程度。
    #[must_use]
    pub fn is_estimated(&self) -> bool {
        self.scalloping_db().abs() > 0.5
    }
}

/// 一帧频谱。
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumFrame {
    /// 本帧第一帧的位置（帧号，非样本下标）。
    pub start_frame: u64,
    /// 单边幅值谱，长度 `fft_size/2 + 1`。
    pub amplitudes: Vec<f32>,
    /// 频率轴间隔，Hz。
    pub bin_hz: f64,
    /// 采样率，Hz。
    pub sample_rate: u32,
    /// 检测到的峰，按幅值降序。
    pub peaks: Vec<Peak>,
    /// 帧内是否做过去直流。
    pub dc_removed: bool,
    /// 去直流时减去的均值。
    pub removed_dc: f32,
}

impl SpectrumFrame {
    /// 第 `i` 个 bin 的频率，Hz。
    #[must_use]
    pub fn bin_frequency(&self, i: usize) -> f64 {
        i as f64 * self.bin_hz
    }

    /// 奈奎斯特频率，Hz。
    #[must_use]
    pub fn nyquist_hz(&self) -> f64 {
        f64::from(self.sample_rate) / 2.0
    }

    /// 第 `i` 个 bin 的幅值，dBFS。静音返回 `-inf`。
    #[must_use]
    pub fn bin_dbfs(&self, i: usize) -> Dbfs {
        match self.amplitudes.get(i) {
            Some(a) => Dbfs::from_linear(*a),
            None => Dbfs::NegativeInfinity,
        }
    }

    /// 钳到显示下限后的 dBFS，供渲染使用。
    #[must_use]
    pub fn bin_dbfs_clamped(&self, i: usize) -> f32 {
        self.bin_dbfs(i).clamped(LINEAR_TO_DB_FLOOR)
    }

    /// 降采样到 `target_bins` 个点，每格取最大值。
    ///
    /// 必须在 Rust 侧完成（ADR-018 规则 3）：前端收到的是最终要画的点数，
    /// 传输量与 FFT 长度解耦。
    #[must_use]
    pub fn downsample_db(&self, target_bins: usize) -> (Vec<f32>, f64) {
        let n = self.amplitudes.len();
        if target_bins == 0 || n == 0 {
            return (Vec::new(), self.bin_hz);
        }
        let bins = target_bins.min(n);
        let mut out = Vec::with_capacity(bins);
        for i in 0..bins {
            let lo = i * n / bins;
            let hi = (((i + 1) * n) / bins).max(lo + 1).min(n);
            let peak = self.amplitudes[lo..hi]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);
            out.push(Dbfs::from_linear(peak).clamped(LINEAR_TO_DB_FLOOR));
        }
        // 每格代表 (hi-lo) 个原 bin，步长相应放大
        (out, self.bin_hz * n as f64 / bins as f64)
    }
}

/// 单边幅值谱的缩放因子，验证归一化约定。
///
/// 公开出来是为了让测试直接对照
/// `docs/superpowers/specs/2026-10-14-auralis-studio-design.md` §5.3。
#[must_use]
pub fn amplitude_scale(window: WindowFunction, fft_size: usize, is_edge_bin: bool) -> f64 {
    let w_sum = window.coefficient_sum(fft_size);
    if is_edge_bin {
        1.0 / w_sum
    } else {
        2.0 / w_sum
    }
}

/// 由未加窗的 FFT 幅度得到单边幅值谱。
///
/// `mags[k]` 是 `|X[k]|`，长度须为 `fft_size/2 + 1`。
///
/// # Panics
/// `window` 的系数和为 0 时 panic（任何已知窗都不会发生）。
#[must_use]
pub fn single_sided_amplitude(mags: &[f32], fft_size: usize, window: WindowFunction) -> Vec<f32> {
    let edge_scale = amplitude_scale(window, fft_size, true) as f32;
    let inter_scale = amplitude_scale(window, fft_size, false) as f32;
    let nyquist = fft_size / 2;
    mags.iter()
        .enumerate()
        .map(|(k, m)| {
            if k == 0 || k == nyquist {
                m * edge_scale
            } else {
                m * inter_scale
            }
        })
        .collect()
}

/// 功率谱。单位 FS²。
#[must_use]
pub fn single_sided_power(amplitudes: &[f32], fft_size: usize) -> Vec<f64> {
    let nyquist = fft_size / 2;
    amplitudes
        .iter()
        .enumerate()
        .map(|(k, a)| {
            let a = f64::from(*a);
            if k == 0 || k == nyquist {
                a * a
            } else {
                a * a / 2.0
            }
        })
        .collect()
}

/// 功率谱密度。单位 FS²/Hz。
#[must_use]
pub fn power_spectral_density(
    amplitudes: &[f32],
    fft_size: usize,
    sample_rate: u32,
    window: WindowFunction,
) -> Vec<f64> {
    let enbw_hz = window.enbw_hz(f64::from(sample_rate), fft_size);
    single_sided_power(amplitudes, fft_size)
        .into_iter()
        .map(|p| p / enbw_hz)
        .collect()
}

/// 频谱分析器：持有 FFT plan 与窗系数，避免每次调用重建。
pub struct SpectrumAnalyzer {
    config: SpectrumConfig,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    w_sum: f64,
}

impl SpectrumAnalyzer {
    /// 创建分析器。
    ///
    /// # Errors
    /// 参数非法（见 [`SpectrumConfig::validate`]）。
    pub fn new(config: SpectrumConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(config.fft_size);
        // 窗系数以 f64 定义（精确），转成 f32 供 FFT 使用。
        let window: Vec<f32> = config
            .window
            .coefficients(config.fft_size)
            .into_iter()
            .map(|x| x as f32)
            .collect();
        // 相位校正用的是 f64 累加，避免 f32 求和的精度损失
        let w_sum: f64 = config.window.coefficients(config.fft_size).iter().sum();
        Ok(Self {
            config,
            fft,
            window,
            w_sum,
        })
    }

    /// 当前配置。
    #[must_use]
    pub const fn config(&self) -> SpectrumConfig {
        self.config
    }

    /// 逐帧分析整个信号。
    ///
    /// 末尾不足一帧的部分按**丢弃**处理（`DSP_SPEC.md` §6 要求定义边界帧策略，
    /// 这里选择丢弃而不是补零，因为补零会造出一个假的衰减）。
    ///
    /// # Errors
    /// 输入不足一帧。
    pub fn analyze(
        &self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<Vec<SpectrumFrame>, ConfigError> {
        let n = self.config.fft_size;
        let hop = self.config.hop_size;
        if samples.len() < n {
            return Err(ConfigError::InputTooShort {
                needed: n,
                available: samples.len(),
            });
        }

        let bin_hz = f64::from(sample_rate) / n as f64;
        let mut frames = Vec::with_capacity((samples.len() - n) / hop + 1);
        let mut start = 0usize;
        let mut frame_index = 0u64;
        while start + n <= samples.len() {
            let mut amps = Vec::with_capacity(n / 2 + 1);
            {
                let mut buf: Vec<Complex<f32>> = Vec::with_capacity(n);
                let dc = if self.config.remove_dc {
                    samples[start..start + n].iter().sum::<f32>() / n as f32
                } else {
                    0.0
                };
                for i in 0..n {
                    let x = if self.config.remove_dc {
                        samples[start + i] - dc
                    } else {
                        samples[start + i]
                    };
                    buf.push(Complex::new(x * self.window[i], 0.0));
                }
                self.fft.process(&mut buf);
                let nyquist = n / 2;
                for (k, c) in buf.iter().take(nyquist + 1).enumerate() {
                    let mag = (c.re * c.re + c.im * c.im).sqrt() as f64;
                    let a = if k == 0 || k == nyquist {
                        mag / self.w_sum
                    } else {
                        2.0 * mag / self.w_sum
                    };
                    amps.push(a as f32);
                }
            }
            let removed_dc = if self.config.remove_dc {
                samples[start..start + n].iter().sum::<f32>() / n as f32
            } else {
                0.0
            };
            frames.push(SpectrumFrame {
                start_frame: frame_index * hop as u64,
                bin_hz,
                sample_rate,
                peaks: detect_peaks(&amps, bin_hz, 8),
                amplitudes: amps,
                dc_removed: self.config.remove_dc,
                removed_dc,
            });
            start += hop;
            frame_index += 1;
        }
        Ok(frames)
    }
}

/// 检测谱峰。
///
/// 步骤（`design.md` §5.5）：
/// 1. 局部极大判定 `m[k] > m[k-1] && m[k] >= m[k+1]`
/// 2. 对三点的**对数幅值**做抛物线插值
/// 3. 按最小间隔去重
/// 4. 按幅值降序取前 `max_peaks` 个
///
/// 每个峰同时保留**未插值**的 bin 频率，便于验证。
#[must_use]
pub fn detect_peaks(amplitudes: &[f32], bin_hz: f64, max_peaks: usize) -> Vec<Peak> {
    if amplitudes.len() < 3 || max_peaks == 0 {
        return Vec::new();
    }
    let n = amplitudes.len();
    // 总能量（用于峰集中度）
    let total: f64 = amplitudes
        .iter()
        .map(|a| f64::from(*a) * f64::from(*a))
        .sum();
    let nyquist_idx = n - 1;

    let mut peaks: Vec<Peak> = Vec::new();
    for k in 1..nyquist_idx {
        let a = amplitudes[k];
        if !(a > amplitudes[k - 1] && a >= amplitudes[k + 1]) {
            continue;
        }
        // NaN 不可参与比较
        if !a.is_finite() || a <= 0.0 {
            continue;
        }

        // 对数抛物线插值
        let y = |i: usize| -> f64 {
            let v = f64::from(amplitudes[i].max(1e-30));
            20.0 * v.log10()
        };
        let (ym, y0, yp) = (y(k - 1), y(k), y(k + 1));
        let denom = ym - 2.0 * y0 + yp;
        let mut delta = if denom.abs() < 1e-20 {
            0.0
        } else {
            0.5 * (ym - yp) / denom
        };
        // 插值点必须落在本 bin 之内
        if !delta.is_finite() || delta < -0.5 || delta > 0.5 {
            delta = 0.0;
        }

        // 峰能量集中度：本 bin ±1 占总能量之比
        let lo = k.saturating_sub(1);
        let hi = (k + 1).min(nyquist_idx);
        let around: f64 = amplitudes[lo..=hi]
            .iter()
            .map(|a| f64::from(*a) * f64::from(*a))
            .sum();
        let concentration = if total > 0.0 {
            (around / total) as f32
        } else {
            0.0
        };

        peaks.push(Peak {
            frequency_hz: (k as f64 + delta) * bin_hz,
            amplitude_dbfs: y0 as f32,
            amplitude: a,
            bin_index: k,
            bin_offset: delta as f32,
            bin_frequency_hz: k as f64 * bin_hz,
            // 抛物线顶点的精确 dB 值。
            //
            // 三点定抛物线 y(t) = a·t² + b·t + y0（t 以 bin 为单位，相对 k）：
            //   a = (ym + yp)/2 - y0
            //   b = (yp - ym)/2
            // 顶点值 y(t*) = y0 - b²/(4a)，其中 t* = -b/(2a) = δ。
            //
            // 用 y0 + (ym-yp)·δ/4 只是一阶近似，实测在 0.4 bin 偏移时
            // 会给出比 bin 读数**更差**的结果（-8.05 vs -6.93 dBFS）。
            // 精确式保证 a<0（局部极大）时结果恒 ≥ y0。
            interpolated_dbfs: {
                let aa = (ym + yp) / 2.0 - y0;
                let bb = (yp - ym) / 2.0;
                if aa < -1e-20 {
                    (y0 - bb * bb / (4.0 * aa)) as f32
                } else {
                    y0 as f32
                }
            },
            peak_concentration: concentration,
        });
    }

    peaks.sort_by(|p, q| {
        q.amplitude_dbfs
            .partial_cmp(&p.amplitude_dbfs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    peaks.truncate(max_peaks);
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 48_000;

    fn sine(freq: f64, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let phase = std::f64::consts::TAU * freq * i as f64 / f64::from(FS);
                (amp as f64 * phase.sin()) as f32
            })
            .collect()
    }

    fn cfg(n: usize) -> SpectrumConfig {
        SpectrumConfig {
            fft_size: n,
            hop_size: n,
            window: WindowFunction::Hann,
            remove_dc: false,
        }
    }

    /// **相干**正弦：频率精确落在整数 bin 上（`f = bin·fs/N`）。
    ///
    /// 只有相干采样才能验证幅值校正：`DSP_SPEC.md` §9 明确 0.2 dB 的幅值误差
    /// 目标「对满足相干采样和校正条件的幅值测试」适用。非相干信号存在频谱泄漏，
    /// 主峰读数天然偏低 —— 拿它测幅值精度是测试方法错误，不是实现错误。
    fn coherent_sine(bin: usize, amp: f32, n: usize) -> Vec<f32> {
        let freq = bin as f64 * f64::from(FS) / n as f64;
        sine(freq, amp, n)
    }

    // ---- T1/T2/T3: 相干正弦的频率与幅值 ----

    #[test]
    fn t1_coherent_sine_frequency_within_one_tenth_bin() {
        let n = 4096;
        // bin 85 -> 85 * 48000/4096 = 996.09 Hz（相干）
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&coherent_sine(85, 0.5, n), FS).unwrap()[0].clone();
        let peak = f.peaks[0];
        let target = 85.0 * f.bin_hz;
        assert!(
            (peak.frequency_hz - target).abs() <= f.bin_hz * 0.1,
            "频率 {} 偏离 {} 超过 0.1 bin",
            peak.frequency_hz,
            target
        );
    }

    #[test]
    fn t1_noncoherent_sine_still_finds_peak_within_one_bin() {
        // 非相干信号也能找到峰，但精度要求放宽到 1 bin
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(1000.0, 0.5, n), FS).unwrap()[0].clone();
        assert!(
            (f.peaks[0].frequency_hz - 1000.0).abs() <= f.bin_hz,
            "非相干信号频率 {} 应在 1 bin 内找到 1000 Hz",
            f.peaks[0].frequency_hz
        );
    }

    #[test]
    fn t2_coherent_sine_amplitude_within_0p2db() {
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&coherent_sine(85, 0.5, n), FS).unwrap()[0].clone();
        // Hann 相干增益校正后，半幅正弦主峰应读到 -6.02 dBFS
        let db = f.bin_dbfs_clamped(f.peaks[0].bin_index);
        assert!((db - -6.020_6).abs() < 0.2, "got {db} dBFS");
    }

    #[test]
    fn t3_amplitude_scaling_is_window_independent() {
        // 同一相干信号换四种窗，主峰读数必须一致（这正是相干增益校正的意义）
        let n = 4096;
        let x = coherent_sine(85, 0.5, n);
        for w in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
        ] {
            let a = SpectrumAnalyzer::new(SpectrumConfig {
                fft_size: n,
                hop_size: n,
                window: w,
                remove_dc: false,
            })
            .unwrap();
            let f = a.analyze(&x, FS).unwrap()[0].clone();
            let db = f.bin_dbfs_clamped(f.peaks[0].bin_index);
            assert!(
                (db - -6.020_6).abs() < 0.2,
                "{} 窗下峰值 {} dBFS 偏离 -6.02",
                w.id(),
                db
            );
        }
    }

    #[test]
    fn full_scale_sine_reads_zero_dbfs() {
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&coherent_sine(85, 1.0, n), FS).unwrap()[0].clone();
        let db = f.bin_dbfs_clamped(f.peaks[0].bin_index);
        assert!(db.abs() < 0.2, "满刻度正弦应读到 ~0 dBFS，实得 {db}");
    }

    // ---- T4: 双音 ----

    #[test]
    fn t4_two_tone_signal_yields_two_peaks() {
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let mut x = sine(1000.0, 0.5, n);
        let other = sine(3000.0, 0.25, n);
        for (i, v) in other.iter().enumerate() {
            x[i] += v;
        }
        let f = a.analyze(&x, FS).unwrap()[0].clone();
        assert!(f.peaks.len() >= 2, "应检出两个峰，实得 {}", f.peaks.len());
        let freqs: Vec<f64> = f.peaks.iter().take(2).map(|p| p.frequency_hz).collect();
        for target in [1000.0, 3000.0] {
            assert!(
                freqs.iter().any(|x| (x - target).abs() < 30.0),
                "未找到 {target} Hz，实得 {freqs:?}"
            );
        }
    }

    // ---- T5: 静音 ----

    #[test]
    fn t5_silence_produces_no_nan_and_true_neg_inf() {
        let n = 1024;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&vec![0.0_f32; n], FS).unwrap()[0].clone();
        assert!(
            f.amplitudes.iter().all(|x| x.is_finite()),
            "静音不得产生 NaN/Inf"
        );
        assert!(f.peaks.is_empty(), "静音不应有峰");
        assert!(f.bin_dbfs(0).is_silence(), "静音的 dBFS 必须是真 -inf");
    }

    // ---- T6: 直流 ----

    #[test]
    fn t6_dc_signal_lands_in_bin_zero() {
        let n = 1024;
        let x = vec![0.1_f32; n];
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&x, FS).unwrap()[0].clone();
        // bin0 不乘 2，因此直接读出直流值 0.1 -> -20 dBFS
        let db = f.bin_dbfs_clamped(0);
        assert!((db - -20.0).abs() < 0.2, "bin0 应为 -20 dBFS，实得 {db}");

        // 注意：加窗后的直流信号**不会**只有 bin0 有能量。
        // 周期型 Hann = 0.5 - 0.25·e^{jθ} - 0.25·e^{-jθ}，其 DFT 在 k=1 处
        // 有 -0.25·N 的分量，因此 bin1 读到 0.1·(2/W)·0.25·N = 0.1。
        // 这是窗函数自身的频谱，不是实现错误 —— 曾把它误判为 bug。
        let bin1 = f.bin_dbfs_clamped(1);
        assert!(
            (bin1 - -20.0).abs() < 0.5,
            "bin1 应为 Hann 自身的谱分量（约 -20 dBFS），实得 {bin1}"
        );

        // 更远的 bin 才应迅速衰减
        let far: f32 = f.amplitudes[3..n / 2]
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f32::max);
        assert!(far < 1e-4, "远离主瓣的 bin 应接近 0，实得 {far}");
    }

    #[test]
    fn dc_removed_moves_energy_away_from_bin_zero() {
        let n = 1024;
        let x = vec![0.1_f32; n];
        let kept = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let removed = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: n,
            hop_size: n,
            window: WindowFunction::Hann,
            remove_dc: true,
        })
        .unwrap();
        let fk = kept.analyze(&x, FS).unwrap()[0].clone();
        let fr = removed.analyze(&x, FS).unwrap()[0].clone();
        assert!(
            fr.bin_dbfs_clamped(0) < fk.bin_dbfs_clamped(0) - 20.0,
            "去直流后 bin0 应显著下降"
        );
    }

    #[test]
    fn t6_remove_dc_moves_energy_out_of_bin_zero() {
        let n = 1024;
        let mut x = sine(1000.0, 0.5, n);
        for v in &mut x {
            *v += 0.3; // 加直流
        }
        let raw = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let dc_removed = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: n,
            hop_size: n,
            window: WindowFunction::Hann,
            remove_dc: true,
        })
        .unwrap();

        let with = raw.analyze(&x, FS).unwrap()[0].clone();
        let without = dc_removed.analyze(&x, FS).unwrap()[0].clone();

        assert!(with.bin_dbfs_clamped(0) > without.bin_dbfs_clamped(0) + 6.0);
        assert!(without.removed_dc > 0.29 && without.removed_dc < 0.31);
    }

    // ---- T7: 近 Nyquist ----

    #[test]
    fn t7_near_nyquist_signal_is_detected() {
        let n = 2048;
        let bin_hz = f64::from(FS) / n as f64;
        // 落在 bin 1023（0.999 bin of Nyquist）
        let target_bin = n / 2 - 1;
        let freq = target_bin as f64 * bin_hz;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(freq, 0.5, n), FS).unwrap()[0].clone();
        assert!(!f.peaks.is_empty(), "近 Nyquist 信号应被检出");
        let p = f.peaks[0];
        assert!(
            (p.frequency_hz - freq).abs() <= bin_hz,
            "峰频 {} 偏离目标 {freq} 超过 1 bin",
            p.frequency_hz
        );
    }

    #[test]
    fn nyquist_bin_is_not_doubled() {
        // 交替正弦 = 恰好位于 Nyquist
        let n = 1024;
        let x: Vec<f32> = (0..n)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&x, FS).unwrap()[0].clone();
        let nyq = f.bin_dbfs_clamped(n / 2);
        assert!(
            (nyq - -6.020_6).abs() < 0.3,
            "Nyquist 分量不应被乘 2，应读到 -6.02，实得 {nyq}"
        );
    }

    // ---- T8: 非相干 ----

    #[test]
    fn t8_noncoherent_sine_frequency_within_one_bin() {
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(1234.5, 0.5, n), FS).unwrap()[0].clone();
        let p = f.peaks[0];
        assert!(
            (p.frequency_hz - 1234.5).abs() <= f.bin_hz,
            "非相干信号频率误差应 <= 1 bin，实得 {} vs 1234.5 (bin_hz={})",
            p.frequency_hz,
            f.bin_hz
        );
    }

    #[test]
    fn t8_half_bin_offset_sine_is_marked_estimated() {
        // 频率落在两个 bin 正中间，是扇贝损失最严重的场景（实测约 1.4 dB）
        let n = 4096;
        let bin_hz = f64::from(FS) / n as f64;
        let freq = 85.5 * bin_hz;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(freq, 0.5, n), FS).unwrap()[0].clone();
        let p = f.peaks[0];
        assert!(
            p.is_estimated(),
            "半个 bin 偏移的正弦读数不可靠，应标为估算（scalloping={:.3} dB）",
            p.scalloping_db()
        );
        // 频率仍在 1 bin 内
        assert!((p.frequency_hz - freq).abs() <= bin_hz);
    }

    #[test]
    fn mild_offset_sine_is_not_marked_estimated() {
        // 偏离 0.1 bin：Hann 的抛物线插值足够准，不应标注估算
        let n = 4096;
        let bin_hz = f64::from(FS) / n as f64;
        let freq = 85.1 * bin_hz;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(freq, 0.5, n), FS).unwrap()[0].clone();
        assert!(
            !f.peaks[0].is_estimated(),
            "偏离 0.1 bin 时不应标为估算（scalloping={:.3} dB）",
            f.peaks[0].scalloping_db()
        );
    }

    #[test]
    fn coherent_peak_has_zero_scalloping() {
        let n = 4096;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&coherent_sine(85, 0.5, n), FS).unwrap()[0].clone();
        let p = f.peaks[0];
        assert!(
            p.scalloping_db().abs() < 0.05,
            "scalloping={}",
            p.scalloping_db()
        );
        assert!(!p.is_estimated());
        assert!(p.bin_offset.abs() < 1e-6, "相干信号的插值偏移应为 0");
    }

    #[test]
    fn interpolated_amplitude_recovers_true_value_at_small_offset() {
        // 偏离 0.4 bin 时，抛物线插值应当把读数拉回真实幅值
        let n = 4096;
        let bin_hz = f64::from(FS) / n as f64;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(85.4 * bin_hz, 0.5, n), FS).unwrap()[0].clone();
        let p = f.peaks[0];
        assert!(
            (p.interpolated_dbfs - -6.020_6).abs() < 0.4,
            "0.4 bin 偏移时插值应接近 -6.02，实得 {}（bin 读数 {}）",
            p.interpolated_dbfs,
            p.amplitude_dbfs
        );
    }

    #[test]
    fn exact_half_bin_is_a_degenerate_case_for_interpolation() {
        // 恰好半个 bin 偏移时主瓣严格对称，三点抛物线接近水平，
        // δ 的解析式变得病态 —— 实测插值结果（-9.19 dB）反而比
        // bin 读数（-7.44 dB）更差。
        //
        // 这不是 bug，而是抛物线插值的固有极限。UI 必须靠
        // `Peak::is_estimated()` 把这类读数标为「估算」，
        // 而不是显示一个看起来很精确的数字。
        let n = 4096;
        let bin_hz = f64::from(FS) / n as f64;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(85.5 * bin_hz, 0.5, n), FS).unwrap()[0].clone();
        let p = f.peaks[0];
        assert!(
            p.is_estimated(),
            "半个 bin 偏移时必须标注为估算（scalloping={:.3} dB）",
            p.scalloping_db()
        );
        // bin 读数本身仍应在 1.5 dB 以内（扇贝损失上限）
        assert!(
            (p.amplitude_dbfs - -6.020_6).abs() < 1.5,
            "bin 读数的扇贝损失应 < 1.5 dB，实得 {}",
            p.amplitude_dbfs
        );
    }

    // ---- Parseval (T12) ----

    #[test]
    fn t12_psd_satisfies_parseval_for_white_noise() {
        // 白噪声（固定 LCG，不用 rand，保证可复现）
        let n = 4096;
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut noise = vec![0.0_f32; n];
        for v in &mut noise {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // 映射到 [-1, 1)
            let u = (state >> 11) as f64 / (1_u64 << 53) as f64;
            *v = ((u * 2.0 - 1.0) * 0.5) as f32;
        }
        let variance: f64 = noise
            .iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum::<f64>()
            / n as f64;

        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&noise, FS).unwrap()[0].clone();
        let psd = power_spectral_density(&f.amplitudes, n, FS, WindowFunction::Hann);

        let df = f.bin_hz;
        let total: f64 = psd.iter().sum::<f64>() * df;
        assert!(
            (total / variance - 1.0).abs() < 0.05,
            "PSD 应满足 Parseval: Σ S·Δf = {total}, σ² = {variance}"
        );
    }

    #[test]
    fn t12_psd_parseval_holds_for_every_window() {
        let n = 2048;
        let mut state = 12345_u64;
        let mut noise = vec![0.0_f32; n];
        for v in &mut noise {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *v = ((state >> 33) as f32 / 8_388_608.0) - 1.0;
        }
        let variance: f64 = noise
            .iter()
            .map(|x| f64::from(*x) * f64::from(*x))
            .sum::<f64>()
            / n as f64;

        for w in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
        ] {
            let a = SpectrumAnalyzer::new(SpectrumConfig {
                fft_size: n,
                hop_size: n,
                window: w,
                remove_dc: false,
            })
            .unwrap();
            let f = a.analyze(&noise, FS).unwrap()[0].clone();
            let psd = power_spectral_density(&f.amplitudes, n, FS, w);
            let total: f64 = psd.iter().sum::<f64>() * f.bin_hz;
            let rel = (total / variance - 1.0).abs();
            assert!(rel < 0.05, "{} 的 PSD Parseval 偏差 {:.3}", w.id(), rel);
        }
    }

    // ---- 通道隔离 (T9) ----

    #[test]
    fn t9_channels_are_isolated() {
        let n = 4096;
        let l = coherent_sine(85, 1.0, n);
        let r: Vec<f32> = coherent_sine(128, 0.3, n);

        let al = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let ar = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let fl = al.analyze(&l, FS).unwrap()[0].clone();
        let fr = ar.analyze(&r, FS).unwrap()[0].clone();

        let pl = fl.peaks[0].amplitude;
        let pr = fr.peaks[0].amplitude;
        assert!((pl - 1.0).abs() < 0.05, "L={pl}");
        assert!((pr - 0.3).abs() < 0.02, "R={pr}（应为 0.3 幅值）");
        // 两者主峰频率不同，且各自独立检出
        assert!(
            (fl.peaks[0].frequency_hz - fr.peaks[0].frequency_hz).abs() > 500.0,
            "两声道主峰频率应不同"
        );
    }

    // ---- 配置校验 (T14) ----

    #[test]
    fn t14_invalid_config_is_rejected() {
        assert!(
            SpectrumConfig {
                fft_size: 0,
                hop_size: 1,
                window: WindowFunction::Hann,
                remove_dc: false
            }
            .validate()
            .is_err()
        );
        assert!(
            SpectrumConfig {
                fft_size: 1024,
                hop_size: 0,
                window: WindowFunction::Hann,
                remove_dc: false
            }
            .validate()
            .is_err()
        );
        assert!(
            SpectrumConfig {
                fft_size: 512,
                hop_size: 1024,
                window: WindowFunction::Hann,
                remove_dc: false
            }
            .validate()
            .is_err()
        );
        assert!(
            SpectrumConfig {
                fft_size: 1024,
                hop_size: 512,
                window: WindowFunction::Hann,
                remove_dc: false
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn short_input_is_rejected_not_panicked() {
        let a = SpectrumAnalyzer::new(cfg(4096)).unwrap();
        let r = a.analyze(&[0.0_f32; 100], FS);
        assert!(matches!(
            r,
            Err(ConfigError::InputTooShort {
                needed: 4096,
                available: 100
            })
        ));
    }

    // ---- 归一化约定 ----

    #[test]
    fn edge_bins_use_single_scale_and_interior_uses_double() {
        let w = WindowFunction::Hann;
        let n = 4096;
        // Hann 的 W = N·CG = 4096·0.5 = 2048
        assert!((amplitude_scale(w, n, true) - 1.0 / 2048.0).abs() < 1e-12);
        assert!((amplitude_scale(w, n, false) - 2.0 / 2048.0).abs() < 1e-12);
    }

    #[test]
    fn single_sided_amplitude_matches_analysis_pipeline() {
        // 独立函数与 analyzer 内部路径必须给出一致结果
        let n = 1024;
        let x = sine(1000.0, 0.5, n);
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&x, FS).unwrap()[0].clone();

        // 手工算一遍未校正的幅度
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let win = WindowFunction::Hann.coefficients(n);
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| Complex::new(x[i] * win[i] as f32, 0.0))
            .collect();
        fft.process(&mut buf);
        let mags: Vec<f32> = (0..=(n / 2))
            .map(|k| {
                let c = buf[k];
                (c.re * c.re + c.im * c.im).sqrt()
            })
            .collect();
        let manual = single_sided_amplitude(&mags, n, WindowFunction::Hann);

        for (i, (m, a)) in manual.iter().zip(f.amplitudes.iter()).enumerate() {
            assert!(
                (*m - a).abs() < 1e-5,
                "bin {i}: 独立函数 {m} 与 analyzer {a} 不一致",
            );
        }
    }

    #[test]
    fn power_is_amplitude_squared_halved_interior() {
        let amps = [0.5_f32, 0.4, 0.3];
        let p = single_sided_power(&amps, 4); // nyquist = 2
        assert!((p[0] - 0.25).abs() < 1e-6, "bin0 不该减半");
        assert!((p[1] - 0.08).abs() < 1e-6, "内部 bin 应减半");
        assert!((p[2] - 0.09).abs() < 1e-6, "Nyquist bin 不该减半");
    }

    // ---- 降采样 ----

    #[test]
    fn downsample_to_pixel_width_takes_max_per_cell() {
        let n = 2049;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(1000.0, 0.5, n), FS).unwrap()[0].clone();
        let (out, step) = f.downsample_db(256);
        assert_eq!(out.len(), 256);
        assert!(step > f.bin_hz, "降采样后步长应放大");
        assert!(out.iter().all(|d| d.is_finite()), "降采样结果必须是有限 dB");
    }

    #[test]
    fn downsample_never_exceeds_source_resolution() {
        let n = 512;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&sine(1000.0, 0.5, n), FS).unwrap()[0].clone();
        let (out, _) = f.downsample_db(100_000);
        assert_eq!(out.len(), n / 2 + 1, "目标点数应被钳到可用 bin 数");
    }

    // ---- 帧处理 ----

    #[test]
    fn hop_controls_frame_count_and_timeline() {
        let n = 1024;
        let x = sine(1000.0, 0.5, n * 10);
        let a = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: n,
            hop_size: n / 4,
            window: WindowFunction::Hann,
            remove_dc: false,
        })
        .unwrap();
        let frames = a.analyze(&x, FS).unwrap();
        assert!(
            frames.len() > 30,
            "10 秒 48k 信号应有更多帧，实得 {}",
            frames.len()
        );
        assert_eq!(frames[0].start_frame, 0);
        assert_eq!(frames[1].start_frame, (n / 4) as u64);
        assert_eq!(frames[2].start_frame, (n / 2) as u64);
    }

    #[test]
    fn trailing_partial_frame_is_discarded() {
        let n = 1024;
        let hop = 512;
        // 1.5 帧
        let x = sine(1000.0, 0.5, n + hop / 2);
        let a = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: n,
            hop_size: hop,
            window: WindowFunction::Hann,
            remove_dc: false,
        })
        .unwrap();
        let frames = a.analyze(&x, FS).unwrap();
        // start=0 完整，start=hop 不完整 -> 只应有 1 帧
        assert_eq!(frames.len(), 1, "不足一帧的尾部应丢弃而非补零");
    }

    #[test]
    fn all_zero_input_produces_finite_frame() {
        let n = 1024;
        let a = SpectrumAnalyzer::new(cfg(n)).unwrap();
        let f = a.analyze(&vec![0.0_f32; n], FS).unwrap()[0].clone();
        assert!(f.amplitudes.iter().all(|x| x.is_finite()));
        assert_eq!(f.amplitudes.len(), n / 2 + 1);
    }
}
