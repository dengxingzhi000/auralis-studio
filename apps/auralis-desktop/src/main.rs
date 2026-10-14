//! Auralis Studio 桌面应用入口。
//!
//! 这个二进制是 workspace 里**唯一**同时认识 egui 和全部领域 crate 的地方
//! （`docs/TAD.md` §2：「`apps/auralis-desktop`：组装应用」）。
//!
//! ## 命令行
//!
//! ```text
//! auralis-studio              启动图形界面
//! auralis-studio --version    打印版本与算法版本后退出
//! auralis-studio --self-test  无窗口运行 DSP 自检后退出（供 CI 使用）
//! ```
//!
//! `--self-test` 存在的理由：CI 机器通常没有显示器，无法验证二进制是否可运行。
//! 它让「Release 构建产物确实能启动」变成一条可自动化的断言，而不是一句承诺。

use auralis_ui::{AppState, Tokens, Workbench};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        print_version();
        return Ok(());
    }
    if args.iter().any(|a| a == "--self-test") {
        return run_self_test();
    }
    if let Some(unknown) = args.first() {
        tracing::warn!(arg = %unknown, "unknown CLI argument ignored");
    }

    init_tracing();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Auralis Studio {}", auralis_core::APP_VERSION))
            .with_inner_size([1600.0, 980.0])
            .with_min_inner_size([1100.0, 700.0])
            // 高 DPI：UI_SPEC §6 要求支持高 DPI 缩放。
            // egui/winit 默认已按显示器 DPI 缩放，这里不额外指定 scale_factor。
            .with_drag_and_drop(true),
        ..Default::default()
    };

    tracing::info!(
        version = auralis_core::APP_VERSION,
        algorithm = auralis_core::ALGORITHM_VERSION,
        "starting Auralis Studio"
    );

    eframe::run_native(
        "Auralis Studio",
        options,
        Box::new(|cc| Ok(Box::new(AuralisApp::new(cc)) as Box<dyn eframe::App>)),
    )
}

fn print_version() {
    println!("auralis-studio {}", auralis_core::APP_VERSION);
    println!("algorithm     {}", auralis_core::ALGORITHM_VERSION);
    println!("dsp spec      {}", auralis_core::DSP_SPEC_REVISION);
}

/// eframe 应用容器。
struct AuralisApp {
    tokens: Tokens,
    state: AppState,
}

impl AuralisApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let tokens = Tokens::dark();
        cc.egui_ctx.set_visuals(themed_visuals(&tokens));
        Self {
            tokens,
            state: AppState::new(),
        }
    }
}

impl eframe::App for AuralisApp {
    /// egui 0.36 用 `ui(&mut Ui, frame)` 取代了 0.31 之前的
    /// `update(&Context, frame)`。根 `Ui` 由 eframe 提供。
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 先克隆 Context：`ui.ctx()` 会不可变借用 ui，
        // 而 Workbench 需要可变借用 ui，二者不能同时存在。
        let ctx = ui.ctx().clone();
        Workbench::new(&self.tokens, &mut self.state).show(&ctx, ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 让窗口底色与 bg.app 一致，避免启动瞬间闪白
        self.tokens.bg_app.to_normalized_gamma_f32()
    }
}

fn themed_visuals(tokens: &Tokens) -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    auralis_ui::theme::apply_to_visuals(&mut v, tokens);
    v
}

/// 初始化 `tracing`。
///
/// 采用 `RUST_LOG` 过滤，默认 `info`。用 try_init 以便测试或多次调用时不 panic。
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// 无窗口 DSP 自检。
///
/// 这一段刻意用真实的 DSP 调用而不是 `assert!` 占位：它证明 release 二进制
/// 能加载、链接并执行数值代码。
/// 无窗口 DSP 自检。
///
/// 刻意调用真实的 DSP 函数并**对结果做断言**，而不是只打印或用占位 `assert!`。
/// 这样 `--self-test` 才是 ADR-012 所说的「可自动化的断言」：
/// 任何数值回归都会让本命令以非零码退出，CI 因此能拦住它。
fn run_self_test() -> eframe::Result<()> {
    use auralis_dsp::{ClippingConfig, WindowFunction, analyze_channel};

    println!("auralis-studio self-test");
    println!("  app       {}", auralis_core::APP_VERSION);
    println!("  algorithm {}", auralis_core::ALGORITHM_VERSION);
    println!();

    let fs = 48_000.0_f64;
    let n = 48_000_usize;
    let sine: Vec<f32> = (0..n)
        .map(|i| 0.5 * (std::f64::consts::TAU * 1_000.0 * i as f64 / fs).sin() as f32)
        .collect();

    let stats = analyze_channel(&sine, ClippingConfig::default());
    let peak_db = stats.peak.dbfs().clamped(-140.0);
    let rms_db = stats.rms.dbfs().clamped(-140.0);

    println!("  1 kHz / 0.5 FS 正弦，{} Hz，{} 采样", fs as u32, n);
    println!("    Peak       {peak_db:.4} dBFS  (期望 -6.0206)");
    // A/√2 = 0.35355 -> -9.0309 dBFS。注意这与正弦的"波峰因数" 3.01 dB 无关，
    // 后者是 peak 与 rms 的差值，不是 rms 的绝对电平。
    println!("    RMS        {rms_db:.4} dBFS  (期望 -9.0309 = -6.0206 - 3.0103)");
    println!(
        "    Crest      {:.4} dB     (期望  3.0103)",
        stats.crest_factor_db()
    );
    println!("    DC offset  {:+.3e}     (期望 ~0)", stats.dc.value);
    println!(
        "    超量程样本 {} / 平顶段 {}",
        stats.over_full_scale_samples, stats.clipping.regions
    );

    // --- 断言（tol 与 crates/auralis-dsp 中的单元测试一致）---
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
        let c = win.coefficients(4096);
        let sum: f64 = c.iter().sum();
        let expected_sum = 4096.0 * win.coherent_gain();
        println!(
            "    window {:<11} CG {:.3}  ENBW {:.3} bin  Σw {:.1} (期望 {:.1})",
            win.id(),
            win.coherent_gain(),
            win.enbw_bins(),
            sum,
            expected_sum
        );
        // 窗系数和必须等于 N · 相干增益（幅值校正的依据）
        if (sum - expected_sum).abs() > 1e-6 {
            eprintln!("  FAIL: {} Σw={sum} 期望 {expected_sum}", win.id());
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
    Ok(())
}

/// 打印一行数值并在超差时退出非零。
fn check(name: &str, actual: f32, expected: f32, tol: f32) {
    if (actual - expected).abs() > tol {
        eprintln!("  FAIL: {name} = {actual:.6}，期望 {expected:.6} ± {tol}");
        std::process::exit(1);
    }
}
