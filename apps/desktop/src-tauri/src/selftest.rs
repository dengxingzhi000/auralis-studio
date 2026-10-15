//! 无窗口 DSP 自检与参考值导出。
//!
//! 存在的理由（ADR-012）：CI 的 Windows runner 没有交互式桌面会话，
//! `tauri::Builder::run` 无法开窗，因此「Release 产物确实能启动」
//! 无法被 CI 直接验证。`--self-test` 让它变成一条可自动化的断言。
//!
//! 这些函数刻意调用**真实的 DSP 代码并对结果做断言**，
//! 而不是打印或占位 `assert!`。

use auralis_dsp::{ClippingConfig, WindowFunction, analyze_channel};

/// 标准测试信号。与 `commands::analyze_synthetic_signal` 分开维护：
/// 这里需要覆盖静音、直流、NaN/Inf、削波等 IPC 压测用不到的边界情况。
fn canonical_signals() -> Vec<(&'static str, Vec<f32>)> {
    const FS: u32 = 48_000;
    let sine = |freq: f64, amp: f32, n: usize| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let phase = std::f64::consts::TAU * freq * i as f64 / f64::from(FS);
                (amp as f64 * phase.sin()) as f32
            })
            .collect()
    };

    // 含 NaN/Inf 的无效输入：验证清理 + 计数
    let mut dirty = vec![0.1_f32; 1_000];
    dirty[10] = f32::NAN;
    dirty[20] = f32::INFINITY;
    dirty[30] = f32::NEG_INFINITY;

    // 削波平顶：显式构造，验证启发式检测
    let mut flat = Vec::new();
    flat.extend((0..10).map(|i| -1.0 + 0.2 * i as f32));
    flat.extend(std::iter::repeat_n(0.999_f32, 12));
    flat.extend((0..10).rev().map(|i| 0.8 - 0.18 * i as f32));
    flat.extend(std::iter::repeat_n(-0.999_f32, 12));

    vec![
        // 相干正弦：1000 Hz @ 48 kHz，1 秒 = 48 周期
        ("sine_1khz_amp0p5", sine(1_000.0, 0.5, 48_000)),
        // 幅值为 1.0：峰值恰好落在采样点上
        ("sine_1khz_amp1p0", sine(1_000.0, 1.0, 48_000)),
        // 超量程：1.5 倍满刻度，验证不被裁剪
        ("sine_1khz_amp1p5", sine(1_000.0, 1.5, 48_000)),
        // 全零：静音必须得到真正的 -inf
        ("silence", vec![0.0_f32; 48_000]),
        // 纯直流 0.1
        ("dc_0p1", vec![0.1_f32; 48_000]),
        ("with_nan_inf", dirty),
        ("flat_top_clipped", flat),
    ]
}

/// 打印一行数值并在超差时退出非零。
fn check(name: &str, actual: f32, expected: f32, tol: f32) {
    if (actual - expected).abs() > tol {
        eprintln!("  FAIL: {name} = {actual:.6}，期望 {expected:.6} ± {tol}");
        std::process::exit(1);
    }
}

/// 把 f32 渲染成 JSON 数字或 `null`（`null` 表示 ±inf / NaN）。
fn json_f32(x: f32) -> String {
    if x.is_finite() {
        format!("{x:?}")
    } else {
        "null".to_string()
    }
}

/// 把 f32 序列渲染成 JSON 数组，非有限值写成 `null`。
fn json_f32_list(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 6);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&json_f32(*x));
    }
    s.push(']');
    s
}

/// 无窗口 DSP 自检。数值超差时以非零码退出。
pub fn run_self_test() -> ! {
    println!("auralis-studio self-test");
    println!("  app       {}", auralis_core::APP_VERSION);
    println!("  algorithm {}", auralis_core::ALGORITHM_VERSION);
    println!();

    let fs = 48_000.0_f64;
    let sine: Vec<f32> = (0..48_000)
        .map(|i| 0.5 * (std::f64::consts::TAU * 1_000.0 * i as f64 / fs).sin() as f32)
        .collect();

    let stats = analyze_channel(&sine, ClippingConfig::default());
    let peak_db = stats.peak.dbfs().clamped(-140.0);
    let rms_db = stats.rms.dbfs().clamped(-140.0);

    println!("  1 kHz / 0.5 FS 正弦，{} Hz", fs as u32);
    println!("    Peak       {peak_db:.4} dBFS  (期望 -6.0206)");
    // A/√2 = 0.35355 -> -9.0309 dBFS。这与正弦的"波峰因数" 3.01 dB 无关，
    // 后者是 peak 与 rms 的差值，不是 rms 的绝对电平。
    println!("    RMS        {rms_db:.4} dBFS  (期望 -9.0309 = -6.0206 - 3.0103)");
    println!(
        "    Crest      {:.4} dB     (期望  3.0103)",
        stats.crest_factor_db()
    );
    println!("    DC offset  {:+.3e}     (期望 ~0)", stats.dc.value);

    let tol = 0.02_f32;
    check("Peak dBFS", peak_db, -6.020_6, tol);
    check("RMS dBFS", rms_db, -9.030_9, tol);
    check("Crest dB", stats.crest_factor_db(), 3.010_3, tol);
    check("DC offset", stats.dc.value, 0.0, 1e-6);
    assert_eq!(stats.over_full_scale_samples, 0, "0.5 FS 正弦不应超量程");
    assert!(!stats.clipping.is_suspicious(), "干净正弦不应被报为削波");

    println!();
    let mut failures = 0_usize;
    for win in [
        WindowFunction::Rectangular,
        WindowFunction::Hann,
        WindowFunction::Hamming,
        WindowFunction::Blackman,
    ] {
        let coeff = win.coefficients(4096);
        let sum: f64 = coeff.iter().sum();
        let expected_sum = 4096.0 * win.coherent_gain();
        println!(
            "    window {:<11} CG {:.3}  ENBW {:.3} bin  sum(w) {:.1} (期望 {:.1})",
            win.id(),
            win.coherent_gain(),
            win.enbw_bins(),
            sum,
            expected_sum
        );
        // 窗系数和必须等于 N * 相干增益（幅值校正的依据）
        if (sum - expected_sum).abs() > 1e-6 {
            eprintln!("  FAIL: {} sum(w)={sum} 期望 {expected_sum}", win.id());
            failures += 1;
        }
        // 实测 ENBW 必须等于解析值
        if (win.measured_enbw_bins(4096) - win.enbw_bins()).abs() > 1e-9 {
            eprintln!(
                "  FAIL: {} 实测 ENBW={} 解析={}",
                win.id(),
                win.measured_enbw_bins(4096),
                win.enbw_bins()
            );
            failures += 1;
        }
    }

    // 静音必须是真正的 -inf，而不是一个假装精确的大数字
    let silence = analyze_channel(&vec![0.0_f32; 4096], ClippingConfig::default());
    assert!(silence.rms.value.is_finite(), "静音输入不得产生 NaN");
    assert!(
        silence.rms.dbfs().is_silence(),
        "静音必须是 -inf 而不是假数字"
    );
    println!();
    println!(
        "    silence       rms={} (dbFS = -inf) OK",
        silence.rms.value
    );

    if failures > 0 {
        eprintln!("  self-test FAILED: {failures} 项数值回归");
        std::process::exit(1);
    }
    println!();
    println!("  self-test OK");
    std::process::exit(0);
}

/// 把 DSP 在标准信号上的结果导出为 JSON，供 numpy 独立实现交叉验证
/// （ADR-015）。写入失败同样非零退出，避免 CI 静默跳过。
pub fn dump_reference(path: &str) -> ! {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"algorithm_version\": {:?},\n",
        auralis_core::ALGORITHM_VERSION
    ));
    out.push_str("  \"cases\": [\n");

    let signals = canonical_signals();
    let total = signals.len();
    for (i, (name, signal)) in signals.iter().enumerate() {
        let s = analyze_channel(signal, ClippingConfig::default());
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": {name:?},\n"));
        out.push_str(&format!("      \"sample_count\": {},\n", s.sample_count));
        out.push_str(&format!(
            "      \"peak\": {},\n",
            json_f32(s.peak.amplitude)
        ));
        out.push_str(&format!(
            "      \"peak_dbfs\": {},\n",
            json_f32(s.peak.dbfs().clamped(f32::NEG_INFINITY))
        ));
        out.push_str(&format!("      \"rms\": {},\n", json_f32(s.rms.value)));
        out.push_str(&format!(
            "      \"rms_dbfs\": {},\n",
            json_f32(s.rms.dbfs().clamped(f32::NEG_INFINITY))
        ));
        out.push_str(&format!("      \"dc\": {},\n", json_f32(s.dc.value)));
        out.push_str(&format!(
            "      \"crest_db\": {},\n",
            json_f32(s.crest_factor_db())
        ));
        out.push_str(&format!(
            "      \"over_full_scale\": {},\n",
            s.peak.over_full_scale
        ));
        out.push_str(&format!(
            "      \"over_full_scale_samples\": {},\n",
            s.over_full_scale_samples
        ));
        out.push_str(&format!(
            "      \"clipping_regions\": {},\n",
            s.clipping.regions
        ));
        out.push_str(&format!(
            "      \"clipping_longest\": {},\n",
            s.clipping.longest_region_samples
        ));
        out.push_str(&format!(
            "      \"clipping_total\": {},\n",
            s.clipping.total_samples
        ));
        // sanitized_samples 是最后一个字段，后面**不能**跟逗号
        out.push_str(&format!(
            "      \"sanitized_samples\": {}\n",
            s.sanitized_samples
        ));
        out.push_str(if i + 1 == total {
            "    }\n"
        } else {
            "    },\n"
        });
    }

    out.push_str("  ],\n  \"windows\": [\n");
    let windows = [
        WindowFunction::Rectangular,
        WindowFunction::Hann,
        WindowFunction::Hamming,
        WindowFunction::Blackman,
    ];
    for (i, w) in windows.iter().enumerate() {
        let n = 4096_usize;
        let coeff32: Vec<f32> = w.coefficients(n).iter().map(|x| *x as f32).collect();
        let sum: f64 = w.coefficients(n).iter().sum();
        out.push_str("    {\n");
        out.push_str(&format!("      \"id\": {:?},\n", w.id()));
        out.push_str(&format!("      \"size\": {n},\n"));
        out.push_str(&format!(
            "      \"coherent_gain\": {:?},\n",
            w.coherent_gain() as f32
        ));
        out.push_str(&format!(
            "      \"enbw_bins\": {:?},\n",
            w.enbw_bins() as f32
        ));
        out.push_str(&format!("      \"coeff_sum\": {sum:?},\n"));
        // coeff_first8 是最后一个字段，后面**不能**跟逗号
        out.push_str(&format!(
            "      \"coeff_first8\": {}\n",
            json_f32_list(&coeff32[..8])
        ));
        out.push_str(if i == windows.len() - 1 {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    out.push_str("  ]\n}\n");

    match std::fs::write(path, out) {
        Ok(()) => {
            println!("reference written to {path}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("failed to write {path}: {e}");
            std::process::exit(1);
        }
    }
}
