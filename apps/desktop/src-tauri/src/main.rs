// The `missing_docs` lint is declared in Cargo.toml and is useful for the
// library, but this binary crate has nothing meaningful to document beyond
// this comment. Opt out explicitly rather than adding noise.
#![allow(missing_docs)]
// Auralis Studio 桌面应用入口。
//
// CLI 子命令在启动 Tauri 之前处理，这样在没有图形环境的机器上
// （例如 CI 的 Windows runner）也能验证产物可运行——见 ADR-012。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("auralis-studio {}", auralis_core::APP_VERSION);
        println!("algorithm     {}", auralis_core::ALGORITHM_VERSION);
        println!("dsp spec      {}", auralis_core::DSP_SPEC_REVISION);
        return;
    }

    // These two diverge (`-> !`), so they end `main` without an explicit return.
    if args.iter().any(|a| a == "--self-test") {
        auralis_desktop_lib::run_self_test();
    }

    if let Some(pos) = args.iter().position(|a| a == "--dump-reference") {
        match args.get(pos + 1) {
            Some(path) => auralis_desktop_lib::dump_reference(path),
            None => {
                eprintln!("--dump-reference 需要一个输出路径参数");
                std::process::exit(2);
            }
        }
    }

    init_tracing();
    auralis_desktop_lib::run();
}

/// Initialise Rust-side logging to stderr.
///
/// The Tauri-side log plugin (which also forwards into the webview console)
/// is registered inside `run()` with its own defaults -- configuring both
/// here would double-install a global subscriber.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}
