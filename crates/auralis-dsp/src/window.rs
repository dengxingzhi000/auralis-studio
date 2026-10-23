//! 窗函数。
//!
//! `DSP_SPEC.md` §5：「明确窗函数及其 coherent gain / 能量归一化」「每种窗函数
//! 应有已知系数和数值测试」。
//!
//! ## 为什么只用周期型（periodic / DFT-even）
//!
//! 周期型窗定义为 `w[n] = f(2πn/N)`，`n = 0..N`。
//! 对称型则是 `w[n] = f(2πn/(N-1))`，两端系数相同。
//!
//! 谱分析中**必须**用周期型：对称型在非整数周期输入下会在频谱两端
//! 产生额外的镜像旁瓣（因为窗本身相对 DFT 基不对称）。对称型只用于
//! FIR 滤波器设计。
//!
//! ## 归一化常数
//!
//! 对周期型 N 点窗 `w[n]`：
//!
//! ```text
//! W       = Σ w[n]                  (窗系数和)
//! W2      = Σ w[n]²                 (窗能量)
//! CG      = W / N                   (coherent gain, 相干增益)
//! ENBW_bins = N · W2 / W²           (等效噪声带宽，单位: bin)
//! ENBW_Hz   = fs · W2 / W²          (等效噪声带宽，单位: Hz)
//! ```
//!
//! 幅值谱的相干增益校正因子为 `2 / W`（DC 与 Nyquist bin 为 `1 / W`），
//! 见 `docs/DSP_SPEC.md` §5 与本项目的频谱模块（M3 实现）。

use std::f64::consts::TAU;

/// 窗函数类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum WindowFunction {
    /// 矩形窗，`w[n] = 1`。
    ///
    /// 相干增益最高（1.0），但主瓣最窄（2 bin），旁瓣最高（-13 dB）。
    /// 用于瞬态冲击、需要最精确频率分辨率的场景。
    #[default]
    Rectangular,
    /// 汉宁窗，`w[n] = 0.5 - 0.5·cos(2πn/N)`。
    ///
    /// 通用默认：主瓣宽 4 bin，第一旁瓣 -31 dB。
    /// `DSP_SPEC.md` §5 推荐的首选之一。
    Hann,
    /// 哈明窗，`w[n] = 0.54 - 0.46·cos(2πn/N)`。
    ///
    /// 比 Hann 略窄的主瓣（3 bin），但旁瓣只抑制到 -42.7 dB。
    /// 适合既想要一定分辨率又不想有太高旁瓣的场景。
    Hamming,
    /// 布莱克曼窗，`w[n] = 0.42 - 0.5cos(2πn/N) + 0.08cos(4πn/N)`。
    ///
    /// 旁瓣低至 -58 dB，但主瓣宽达 6 bin，频率分辨率最差。
    /// 适合需要看微小频率成分、且可以接受分辨率下降的场景。
    Blackman,
    /// 五项平顶窗（flat-top）。
    ///
    /// `w[n] = a₀ - a₁cos(θ) + a₂cos(2θ) - a₃cos(3θ) + a₄cos(4θ)`，
    /// 系数取自 SciPy `signal.windows.flattop` 的周期型定义。
    ///
    /// **用途**：幅值测量。主瓣很宽（ENBW ≈ 3.77 bin），但扇贝损失
    /// 最坏仅约 0.01 dB（Hann 约 1.42 dB）。谱线读数需要准确时用它，
    /// 需要频率分辨率时用 Hann。
    FlatTop,
    /// 四项 Blackman-Harris 窗。
    ///
    /// `w[n] = a₀ - a₁cos(θ) + a₂cos(2θ) - a₃cos(3θ)`。
    /// 旁瓣约 -92 dB，ENBW ≈ 2.00 bin；扇贝损失约 0.83 dB。
    /// 适合既要低旁瓣、又不想像 flat-top 那样牺牲太多分辨率的场景。
    BlackmanHarris,
}

impl WindowFunction {
    /// 生成 N 点周期型窗系数。
    ///
    /// # Panics
    /// `size == 0` 时 panic。零长窗没有意义，属于调用方编程错误。
    #[must_use]
    pub fn coefficients(self, size: usize) -> Vec<f64> {
        assert!(size > 0, "window size must be greater than zero");
        let n = size as f64;
        (0..size)
            .map(|i| {
                let phase = TAU * i as f64 / n;
                match self {
                    Self::Rectangular => 1.0,
                    Self::Hann => 0.5 - 0.5 * phase.cos(),
                    Self::Hamming => 0.54 - 0.46 * phase.cos(),
                    Self::Blackman => 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos(),
                    // 系数与 SciPy signal.windows.flattop（periodic）一致。
                    Self::FlatTop => {
                        0.215_578_95 - 0.416_631_58 * phase.cos()
                            + 0.277_263_158 * (2.0 * phase).cos()
                            - 0.083_578_947 * (3.0 * phase).cos()
                            + 0.006_947_368 * (4.0 * phase).cos()
                    }
                    // 系数与 SciPy signal.windows.blackmanharris（periodic）一致。
                    Self::BlackmanHarris => {
                        0.358_75 - 0.488_29 * phase.cos() + 0.141_28 * (2.0 * phase).cos()
                            - 0.011_68 * (3.0 * phase).cos()
                    }
                }
            })
            .collect()
    }

    /// 相干增益 `W / N` 的解析值。
    ///
    /// 解析值比 `coefficients` 数值求和更精确，可用于归一化计算和测试断言。
    #[must_use]
    pub const fn coherent_gain(self) -> f64 {
        match self {
            Self::Rectangular => 1.0,
            Self::Hann => 0.5,
            Self::Hamming => 0.54,
            Self::Blackman => 0.42,
            // 五项余弦窗的 coherent gain 就是 a₀。
            Self::FlatTop => 0.215_578_95,
            Self::BlackmanHarris => 0.358_75,
        }
    }

    /// 等效噪声带宽，单位 bin（解析值）。
    ///
    /// 解析推导：`ENBW = N·Σw²/W² = (Σw²/N) / (Σw/N)²`，
    /// 对周期型窗该比值与 N 无关。
    #[must_use]
    pub const fn enbw_bins(self) -> f64 {
        match self {
            // Σw/N = 1,        Σw²/N = 1              -> 1 / 1
            Self::Rectangular => 1.0,
            // Σw/N = 0.5,      Σw²/N = 3/8 = 0.375      -> 0.375 / 0.25 = 1.5
            Self::Hann => 1.5,
            // Σw/N = 0.54,     Σw²/N = 0.54² + 0.46²/2 = 0.3974
            //                                    -> 0.3974 / 0.2916
            Self::Hamming => 1.362_825_788_751_714_6,
            // Σw/N = 0.42,     Σw²/N = 0.42² + 0.5²/2 + 0.08²/2 = 0.3046
            //                                    -> 0.3046 / 0.1764
            Self::Blackman => 1.726_757_369_614_512_5,
            // Σw²/N = a₀² + (a₁²+a₂²+a₃²+a₄²)/2，Σw/N = a₀
            Self::FlatTop => 3.770_246_447_443_428_7,
            // Σw²/N = a₀² + (a₁²+a₂²+a₃²)/2
            Self::BlackmanHarris => 2.004_352_938_217_047_5,
        }
    }

    /// 给定采样率与 FFT 长度下的等效噪声带宽，单位 Hz。
    ///
    /// `ENBW_Hz = ENBW_bins · fs / N`。`ENBW_bins` 与 N 无关，但总带宽随 N 变宽。
    ///
    /// # Panics
    /// `fft_size == 0` 时 panic。
    #[must_use]
    pub fn enbw_hz(self, sample_rate: f64, fft_size: usize) -> f64 {
        assert!(fft_size > 0, "fft_size must be greater than zero");
        self.enbw_bins() * sample_rate / fft_size as f64
    }

    /// 从 `coefficients` 数值求得的相干增益，用于验证解析值。
    pub fn measured_coherent_gain(self, size: usize) -> f64 {
        let c = self.coefficients(size);
        c.iter().sum::<f64>() / size as f64
    }

    /// 从 `coefficients` 数值求得的 ENBW（bin），用于验证解析值。
    pub fn measured_enbw_bins(self, size: usize) -> f64 {
        let c = self.coefficients(size);
        let w: f64 = c.iter().sum();
        let w2: f64 = c.iter().map(|x| x * x).sum();
        size as f64 * w2 / (w * w)
    }

    /// 稳定标识，用于序列化和 UI 下拉列表。
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Rectangular => "rectangular",
            Self::Hann => "hann",
            Self::Hamming => "hamming",
            Self::Blackman => "blackman",
            Self::FlatTop => "flat-top",
            Self::BlackmanHarris => "blackman-harris",
        }
    }

    /// UI 显示名。
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Rectangular => "Rectangular (矩形)",
            Self::Hann => "Hann (汉宁)",
            Self::Hamming => "Hamming (哈明)",
            Self::Blackman => "Blackman (布莱克曼)",
            Self::FlatTop => "Flat-top (平顶，幅值测量)",
            Self::BlackmanHarris => "Blackman-Harris (四项)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 64;

    /// 每种窗的已知系数，用于发现定义层面的改动。
    ///
    /// 参考值由定义式在 N=64 上逐点计算，不是从本实现反推。
    #[test]
    fn known_coefficients_are_exact_at_endpoints() {
        let rect = WindowFunction::Rectangular.coefficients(N);
        assert!((rect[0] - 1.0).abs() < 1e-12);
        assert!((rect[N / 2] - 1.0).abs() < 1e-12);

        let hann = WindowFunction::Hann.coefficients(N);
        assert!(hann[0].abs() < 1e-12, "周期型 Hann 首项为 0");
        assert!((hann[N / 4] - 0.5).abs() < 1e-12);
        assert!((hann[N / 2] - 1.0).abs() < 1e-12);

        let hamming = WindowFunction::Hamming.coefficients(N);
        assert!((hamming[0] - 0.08).abs() < 1e-12);
        assert!((hamming[N / 4] - 0.54).abs() < 1e-12);
        assert!((hamming[N / 2] - 1.0).abs() < 1e-12);

        let blackman = WindowFunction::Blackman.coefficients(N);
        assert!((blackman[0] - 0.0).abs() < 1e-12);
        // n = N/4 时 cos(pi/2)=0 且 cos(pi)=-1，故 0.42 - 0 + 0.08*(-1) = 0.34
        assert!((blackman[N / 4] - 0.34).abs() < 1e-12);
        assert!((blackman[N / 2] - 1.0).abs() < 1e-12);

        let flat_top = WindowFunction::FlatTop.coefficients(N);
        // 五项系数代数和 = 1：n=0 时正负交替，端点约 -4.2e-4。
        assert!((flat_top[0] - (-0.000_421_05)).abs() < 1e-6);
        assert!((flat_top[N / 2] - 1.0).abs() < 1e-6);

        let bh = WindowFunction::BlackmanHarris.coefficients(N);
        assert!((bh[0] - 0.000_06).abs() < 1e-6);
        assert!((bh[N / 2] - 1.0).abs() < 1e-6);
    }

    /// 平顶窗的 ENBW 必须明显大于 Hann；这是「幅值精度换频率分辨率」的代价。
    #[test]
    fn flat_top_trades_resolution_for_amplitude_accuracy() {
        let ft = WindowFunction::FlatTop;
        assert!((ft.coherent_gain() - 0.215_578_95).abs() < 1e-12);
        assert!((ft.enbw_bins() - 3.770_246_447).abs() < 1e-9);
        assert!(ft.enbw_bins() > WindowFunction::Hann.enbw_bins() * 2.0);
        assert_eq!(ft.id(), "flat-top");
        assert!(ft.display_name().contains("幅值"));
    }

    /// 周期型窗是循环对称的：`w[n] = w[N-n]`，因此 `w[1] == w[N-1]`。
    /// 对称型则不满足——这正是我们选用周期型的判别特征。
    #[test]
    fn periodic_windows_are_cyclically_symmetric() {
        for win in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::FlatTop,
            WindowFunction::BlackmanHarris,
        ] {
            let c = win.coefficients(N);
            for n in 1..N / 2 {
                assert!(
                    (c[n] - c[N - n]).abs() < 1e-12,
                    "{} 在 n={n} 处不满足循环对称",
                    win.id()
                );
            }
        }
    }

    /// 测量得到的相干增益应与解析值一致。
    #[test]
    fn measured_coherent_gain_matches_analytic() {
        for win in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::FlatTop,
            WindowFunction::BlackmanHarris,
        ] {
            assert!(
                (win.measured_coherent_gain(N) - win.coherent_gain()).abs() < 1e-12,
                "{} 相干增益不符",
                win.id()
            );
        }
    }

    /// 测量得到的 ENBW 应与解析值一致。
    #[test]
    fn measured_enbw_matches_analytic() {
        for win in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::FlatTop,
            WindowFunction::BlackmanHarris,
        ] {
            assert!(
                (win.measured_enbw_bins(N) - win.enbw_bins()).abs() < 1e-9,
                "{} ENBW 不符: measured={} analytic={}",
                win.id(),
                win.measured_enbw_bins(N),
                win.enbw_bins()
            );
        }
    }

    /// Hann 的 ENBW = 1.5 bin 是教科书值，单独断言以防回归。
    #[test]
    fn hann_enbw_is_one_and_a_half_bins() {
        assert!((WindowFunction::Hann.enbw_bins() - 1.5).abs() < 1e-12);
        // 48 kHz / 4096 点 = 11.71875 Hz/bin → Hann ENBW = 17.578 Hz
        let hz = WindowFunction::Hann.enbw_hz(48_000.0, 4096);
        assert!((hz - 17.578_125).abs() < 1e-6);
    }

    /// 系数和 W 的数值形式，供幅值校正使用。
    #[test]
    fn coefficient_sum_matches_length_times_cg() {
        for win in [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::FlatTop,
            WindowFunction::BlackmanHarris,
        ] {
            let w: f64 = win.coefficients(N).iter().sum();
            let expected = N as f64 * win.coherent_gain();
            assert!((w - expected).abs() < 1e-9, "{} 系数和不符", win.id());
        }
    }

    #[test]
    #[should_panic(expected = "window size must be greater than zero")]
    fn zero_length_window_panics() {
        let _ = WindowFunction::Hann.coefficients(0);
    }

    #[test]
    fn ids_are_unique() {
        let ids: Vec<&str> = [
            WindowFunction::Rectangular,
            WindowFunction::Hann,
            WindowFunction::Hamming,
            WindowFunction::Blackman,
            WindowFunction::FlatTop,
            WindowFunction::BlackmanHarris,
        ]
        .iter()
        .map(|w| w.id())
        .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }
}
