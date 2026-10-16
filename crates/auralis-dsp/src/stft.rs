//! STFT（短时傅里叶变换）——声谱图的数据来源。
//!
//! ## 与逐帧 FFT 的区别
//!
//! [`crate::spectrum::SpectrumAnalyzer`] 给出的是「整段信号的频谱」。
//! STFT 把信号切成重叠的短帧，对**每帧**做一次 FFT，得到
//! 「频率 × 时间」的矩阵 —— 这才是声谱图与瀑布图的数据。
//!
//! ## 边界帧策略
//!
//! `DSP_SPEC.md` §6 要求定义边界帧策略。本模块选择：
//! **末尾不足一帧的部分直接丢弃**，不补零。
//!
//! 理由：补零会造出一个假的「信号在此处突然衰减」的竖条纹，
//! 用户会把它当成真实的包络变化。丢弃只是少显示最后几十毫秒，
//! 这个代价明显更小。
//!
//! ## 内存
//!
//! 矩阵大小 = `frames × bins`。`frames = (N - fft) / hop + 1`。
//! 对 3 小时 48 kHz 文件、fft=2048/hop=512：
//! frames ≈ 1,012,500，bins = 1025，f32 约 **4.15 GB** —— 不可接受。
//!
//! 因此本模块提供两级 API：
//! 1. [`Stft::compute`] —— 全量矩阵，仅用于短信号或离线分析
//! 2. [`Stft::compute_range`] —— **只算指定时间范围**，用于按视口渲染
//!
//! 长文件应由上层先建包络/索引再按需取窗口，详见 `auralis-visualization`。
//! 这里保证单次调用的内存与**请求范围**成正比，而非与文件长度成正比。

use crate::spectrum::{ConfigError, Peak, SpectrumConfig};
use crate::window::WindowFunction;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// dB 显示下限。
///
/// 与 `auralis_core::units::LINEAR_TO_DB_FLOOR` 一致，在此重复定义是为了
/// 避免 spectrum 与 units 的循环依赖；`tests` 中有断言锁定两者一致。
pub const MIN_DB: f32 = -140.0;

/// 一帧频谱的 dB 值。
#[derive(Debug, Clone, PartialEq)]
pub struct StftFrame {
    /// 本帧第一帧的位置。
    pub start_frame: u64,
    /// 单边 dB 序列，长度 `bin_count`。
    ///
    /// 静音段为 `f32::NEG_INFINITY`（**不是**被钳过的 -140）。
    /// 渲染层再决定如何显示。
    pub db: Vec<f32>,
    /// 本帧检出的峰。
    pub peaks: Vec<Peak>,
}

impl StftFrame {
    /// 第 `i` 个 bin 的 dB 值。
    #[must_use]
    pub fn db_at(&self, i: usize) -> f32 {
        self.db.get(i).copied().unwrap_or(f32::NEG_INFINITY)
    }

    /// 本帧峰值（dB 最大者）。静音帧返回 `None`。
    #[must_use]
    pub fn peak_db(&self) -> Option<f32> {
        self.db
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(None, |acc: Option<f32>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }
}

/// STFT 参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StftConfig {
    /// 帧长（= FFT 长度）。
    pub frame_len: usize,
    /// 帧移 hop。
    pub hop_size: usize,
    /// 窗函数。
    pub window: WindowFunction,
    /// 是否去掉直流。
    pub remove_dc: bool,
}

impl StftConfig {
    /// 由频谱配置构造，两者共享帧长与窗。
    #[must_use]
    pub const fn from_spectrum(cfg: &SpectrumConfig) -> Self {
        Self {
            frame_len: cfg.fft_size,
            hop_size: cfg.hop_size,
            window: cfg.window,
            remove_dc: cfg.remove_dc,
        }
    }

    /// 校验。
    ///
    /// # Errors
    /// 帧长为 0、hop 为 0、hop 超过帧长。
    pub fn validate(&self) -> Result<(), ConfigError> {
        SpectrumConfig {
            fft_size: self.frame_len,
            hop_size: self.hop_size,
            window: self.window,
            remove_dc: self.remove_dc,
        }
        .validate()
    }
}

impl Default for StftConfig {
    /// 2048 点、75% 重叠、Hann —— 声谱图的常用默认。
    fn default() -> Self {
        Self {
            frame_len: 2048,
            hop_size: 512,
            window: WindowFunction::Hann,
            remove_dc: false,
        }
    }
}

/// STFT 分析器。持有 FFT plan 与窗系数，热路径零分配以外的固定开销。
pub struct Stft {
    config: StftConfig,
    fft: std::sync::Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    w_sum: f64,
}

impl Stft {
    /// 创建分析器。
    ///
    /// # Errors
    /// 参数非法。
    pub fn new(config: StftConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(config.frame_len);
        let coeffs = config.window.coefficients(config.frame_len);
        let w_sum: f64 = coeffs.iter().sum();
        Ok(Self {
            config,
            fft,
            window: coeffs.into_iter().map(|x| x as f32).collect(),
            w_sum,
        })
    }

    /// 当前配置。
    #[must_use]
    pub const fn config(&self) -> StftConfig {
        self.config
    }

    /// 单边 bin 数。
    #[must_use]
    pub const fn bin_count(&self) -> usize {
        self.config.frame_len / 2 + 1
    }

    /// 帧移对应的秒数。
    #[must_use]
    pub fn hop_seconds(&self, sample_rate: u32) -> f64 {
        self.config.hop_size as f64 / sample_rate as f64
    }

    /// 帧数（不含被丢弃的尾帧）。
    #[must_use]
    pub fn frame_count(&self, sample_count: usize) -> usize {
        let n = self.config.frame_len;
        let hop = self.config.hop_size;
        if sample_count < n || hop == 0 {
            0
        } else {
            (sample_count - n) / hop + 1
        }
    }

    /// 计算**整个**信号的 STFT。
    ///
    /// 仅适合短信号；长文件请用 [`Stft::compute_range`]。
    ///
    /// # Errors
    /// 样本不足一帧。
    pub fn compute(
        &self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<Vec<StftFrame>, ConfigError> {
        self.compute_range(samples, sample_rate, 0, samples.len())
    }

    /// 只计算 `[first_sample, last_sample)` 区间的帧。
    ///
    /// 内存与**请求区间**成正比，与文件总长无关 —— 这是长文件可用的关键。
    ///
    /// # Errors
    /// 区间不足一帧。
    pub fn compute_range(
        &self,
        samples: &[f32],
        sample_rate: u32,
        first_sample: usize,
        last_sample: usize,
    ) -> Result<Vec<StftFrame>, ConfigError> {
        let n = self.config.frame_len;
        // hop 在下方对齐时已经声明，这里不再重复绑定
        let _ = sample_rate;

        let hi = last_sample.min(samples.len());
        // 帧起点对齐到**全局** hop 网格（而不是相对 lo 的网格）。
        // 这样同一时间窗口无论从哪里开始请求，得到的帧位置都一致，
        // 结果可缓存、可与全量计算逐帧比对。
        let hop = self.config.hop_size;
        let lo = (first_sample.min(hi)) / hop * hop;
        if hi.saturating_sub(lo) < n {
            return Err(ConfigError::InputTooShort {
                needed: n,
                available: hi.saturating_sub(lo),
            });
        }

        let bin_count = n / 2 + 1;
        let nyquist = n / 2;
        let mut frames = Vec::with_capacity((hi - lo - n) / hop + 1);
        let mut buf: Vec<Complex<f32>> = Vec::with_capacity(n);

        let mut start = lo;
        while start + n <= hi {
            buf.clear();
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

            let mut amps = Vec::with_capacity(bin_count);
            for (k, c) in buf.iter().take(bin_count).enumerate() {
                let mag = (c.re * c.re + c.im * c.im).sqrt() as f64;
                let a = if k == 0 || k == nyquist {
                    mag / self.w_sum
                } else {
                    2.0 * mag / self.w_sum
                };
                amps.push(a as f32);
            }

            let db: Vec<f32> = amps
                .iter()
                .map(|a| {
                    if *a > 0.0 {
                        20.0 * a.log10()
                    } else {
                        f32::NEG_INFINITY
                    }
                })
                .collect();

            let bin_hz = self.bin_hz(sample_rate);
            frames.push(StftFrame {
                start_frame: start as u64,
                db,
                peaks: crate::spectrum::detect_peaks(&amps, bin_hz, 4),
            });

            start += hop;
        }

        Ok(frames)
    }

    /// bin 间隔 Hz。
    #[must_use]
    pub fn bin_hz(&self, sample_rate: u32) -> f64 {
        f64::from(sample_rate) / self.config.frame_len as f64
    }

    /// 把 STFT 帧降采样成二维 dB 矩阵，供纹理上传。
    ///
    /// `target_frames` / `target_bins` 指定**输出网格的尺寸**，与源帧数无关：
    /// 纹理必须有固定尺寸，缩放由渲染层负责，因此这里允许上采样（复制/取最大值）。
    ///
    /// 每格取**最大值**而非平均 —— 峰值不能被平均掉，这是声学工具的惯例。
    ///
    /// 返回 `(matrix, bin_hz_step)`，矩阵按 `[frame][bin]` 行优先。
    #[must_use]
    pub fn to_matrix(
        frames: &[StftFrame],
        target_frames: usize,
        target_bins: usize,
        min_db: f32,
        max_db: f32,
    ) -> (Vec<Vec<f32>>, f64) {
        if frames.is_empty() || target_frames == 0 || target_bins == 0 {
            return (Vec::new(), 1.0);
        }
        let bins = frames[0].db.len();
        let tf = target_frames;
        let tb = target_bins;
        let mut matrix = vec![vec![min_db; tb]; tf];

        for (fi, out_row) in matrix.iter_mut().enumerate() {
            // 源帧区间，clamp 到实际范围
            let f0 = (fi * frames.len() / tf).min(frames.len().saturating_sub(1));
            let f1 = (((fi + 1) * frames.len()) / tf)
                .max(f0 + 1)
                .min(frames.len())
                .max(f0 + 1);
            for (bi, slot) in out_row.iter_mut().enumerate() {
                let b0 = (bi * bins / tb).min(bins.saturating_sub(1));
                let b1 = (((bi + 1) * bins) / tb).max(b0 + 1).min(bins).max(b0 + 1);
                let mut peak = f32::NEG_INFINITY;
                for fr in &frames[f0..f1.min(frames.len())] {
                    for v in &fr.db[b0..b1.min(bins)] {
                        if *v > peak {
                            peak = *v;
                        }
                    }
                }
                *slot = if peak.is_finite() {
                    peak.clamp(min_db, max_db)
                } else {
                    min_db
                };
            }
        }
        (matrix, bins as f64 / tb as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auralis_core::units::LINEAR_TO_DB_FLOOR;

    const FS: u32 = 48_000;

    fn cfg(n: usize, hop: usize) -> StftConfig {
        StftConfig {
            frame_len: n,
            hop_size: hop,
            window: WindowFunction::Hann,
            remove_dc: false,
        }
    }

    fn sine(freq: f64, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let p = std::f64::consts::TAU * freq * i as f64 / f64::from(FS);
                (amp as f64 * p.sin()) as f32
            })
            .collect()
    }

    #[test]
    fn min_db_matches_core_floor() {
        // 防止两处常量漂移
        assert_eq!(MIN_DB, LINEAR_TO_DB_FLOOR);
    }

    #[test]
    fn frame_count_is_correct() {
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        // 精确 10 帧：(10000-1024)/256 + 1 = 36.4 -> 36
        assert_eq!(stft.frame_count(10_000), (10_000 - 1024) / 256 + 1);
        assert_eq!(stft.frame_count(100), 0, "不足一帧应为 0");
    }

    #[test]
    fn produces_expected_frame_and_bin_counts() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(2048, 512)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        assert_eq!(frames.len(), stft.frame_count(x.len()));
        assert_eq!(frames[0].db.len(), stft.bin_count());
        assert_eq!(stft.bin_count(), 1025);
    }

    #[test]
    fn frame_start_offsets_advance_by_hop() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        assert_eq!(frames[0].start_frame, 0);
        assert_eq!(frames[1].start_frame, 256);
        assert_eq!(frames[2].start_frame, 512);
    }

    #[test]
    fn steady_tone_gives_stable_frames() {
        // 稳态单音：每帧峰值应几乎一致
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(2048, 512)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        let peaks: Vec<f32> = frames.iter().filter_map(StftFrame::peak_db).collect();
        assert!(peaks.len() > 50, "应产出足够多帧");
        let mn = peaks.iter().cloned().fold(f32::INFINITY, f32::min);
        let mx = peaks.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            mx - mn < 1.0,
            "稳态信号各帧峰值应基本一致，实得跨度 {} dB",
            mx - mn
        );
    }

    #[test]
    fn tone_frequency_is_found_in_peak() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(4096, 1024)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        let mid = frames.len() / 2;
        let p = frames[mid].peaks.first().expect("应检出峰");
        assert!(
            (p.frequency_hz - 1000.0).abs() < 20.0,
            "峰频 {} 应接近 1000 Hz",
            p.frequency_hz
        );
    }

    #[test]
    fn silence_yields_neg_infinity_not_fake_numbers() {
        let x = vec![0.0_f32; 48_000];
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        assert!(
            frames[0]
                .db
                .iter()
                .all(|v| v.is_infinite() && v.is_sign_negative()),
            "静音必须是真 -inf"
        );
        assert!(frames[0].peak_db().is_none(), "静音帧不应有峰值");
    }

    #[test]
    fn trailing_partial_frame_is_discarded() {
        let n = 1024;
        let hop = 512;
        // 1.5 帧
        let x = sine(1_000.0, 0.5, n + hop / 2);
        let stft = Stft::new(cfg(n, hop)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        assert_eq!(frames.len(), 1, "不足一帧的尾部应丢弃");
    }

    #[test]
    fn range_frames_snap_to_global_hop_grid() {
        // 帧起点对齐到全局 hop 网格：同一时间窗口从哪里开始请求都一样，
        // 这样结果才能与全量计算逐帧比对（也可缓存复用）
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        let all = stft.compute(&x, FS).unwrap();
        // 10000 不是 256 的倍数，应被吸附到 9984
        let part = stft.compute_range(&x, FS, 10_000, 20_000).unwrap();
        assert_eq!(part[0].start_frame % 256, 0, "帧起点必须落在 hop 网格上");
        let offset = part[0].start_frame as usize / 256;
        assert_eq!(part[0].start_frame, all[offset].start_frame);
        assert_eq!(part[0].db, all[offset].db, "对齐后应与全量计算逐值相同");
    }

    #[test]
    fn short_range_is_rejected_not_panicked() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(4096, 512)).unwrap();
        let r = stft.compute_range(&x, FS, 0, 100);
        assert!(matches!(r, Err(ConfigError::InputTooShort { .. })));
    }

    #[test]
    fn dc_offset_appears_as_steady_band() {
        // 直流 + 极低频（近似直流）
        let x: Vec<f32> = (0..48_000)
            .map(|i| 0.5 + 0.2 * (std::f64::consts::TAU * 0.5 * i as f64 / FS as f64).sin() as f32)
            .collect();
        let stft = Stft::new(cfg(2048, 512)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();

        // 注意：不能说「bin0 最高」。周期型 Hann 本身在 bin1 也有分量，
        // 而 bin1 会被 ×2（单边谱），因此 bin1 可以与 bin0 持平 ——
        // 这与 M2 中「加窗直流信号在 bin1 有能量」是同一件事。
        for f in &frames {
            let b0 = f.db_at(0);
            let b1 = f.db_at(1);
            let mid = f.db[10..500]
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(b0 > mid + 20.0, "bin0 应远高于中频（{b0} vs {mid}）");
            assert!(b1 > mid + 20.0, "bin1 应远高于中频（{b1} vs {mid}）");
        }
    }

    #[test]
    fn remove_dc_suppresses_bin_zero() {
        let x: Vec<f32> = (0..48_000)
            .map(|i| {
                0.5 + 0.2 * (std::f64::consts::TAU * 2_000.0 * i as f64 / FS as f64).sin() as f32
            })
            .collect();
        let kept = Stft::new(cfg(2048, 512)).unwrap();
        let removed = Stft::new(StftConfig {
            remove_dc: true,
            ..cfg(2048, 512)
        })
        .unwrap();
        let fk = kept.compute(&x, FS).unwrap()[0].db_at(0);
        let fr = removed.compute(&x, FS).unwrap()[0].db_at(0);
        assert!(fr < fk - 20.0, "去直流后 bin0 应显著下降（{fk} -> {fr}）");
    }

    // ---- 矩阵降采样 ----

    #[test]
    fn matrix_has_requested_shape() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        let (m, _) = Stft::to_matrix(&frames, 200, 128, MIN_DB, 0.0);
        assert_eq!(m.len(), 200);
        assert!(m.iter().all(|row| row.len() == 128));
    }

    #[test]
    fn matrix_values_are_finite_and_clamped() {
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(1024, 256)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        let (m, _) = Stft::to_matrix(&frames, 100, 64, MIN_DB, 0.0);
        assert!(
            m.iter()
                .flatten()
                .all(|v| v.is_finite() && *v >= MIN_DB && *v <= 0.0),
            "矩阵值必须有限且落在显示范围内"
        );
    }

    #[test]
    fn matrix_always_has_requested_shape() {
        // 纹理需要固定尺寸，因此即使目标大于源，也必须产出目标大小
        let x = sine(1_000.0, 0.5, 48_000);
        let stft = Stft::new(cfg(2048, 512)).unwrap();
        let frames = stft.compute(&x, FS).unwrap();
        let (m, step) = Stft::to_matrix(&frames, 500, 900, MIN_DB, 0.0);
        assert_eq!(m.len(), 500);
        assert!(m.iter().all(|row| row.len() == 900));
        assert!(step > 0.0);
    }

    #[test]
    fn matrix_of_empty_input_is_empty() {
        let (m, _) = Stft::to_matrix(&[], 100, 64, MIN_DB, 0.0);
        assert!(m.is_empty());
    }

    #[test]
    fn bin_hz_and_hop_seconds_are_consistent() {
        let stft = Stft::new(cfg(2048, 512)).unwrap();
        assert!((stft.bin_hz(48_000) - 23.4375).abs() < 1e-9);
        assert!((stft.hop_seconds(48_000) - 512.0 / 48_000.0).abs() < 1e-12);
    }
}
