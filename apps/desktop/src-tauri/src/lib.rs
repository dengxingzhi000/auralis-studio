//! Auralis Studio 桌面应用（Tauri 侧）。
//!
//! ## 职责边界（ADR-016 / ADR-018）
//!
//! 这个 crate 是**音频内核与前端之间的唯一接口层**。
//!
//! 硬规则：
//! 1. React **不得**参与音频计算。dB 换算、降采样、峰值检测全部
//!    在 `auralis-dsp` 完成后才传输。
//! 2. 二进制优先。波形与频谱以 `Uint8Array` 传输，**禁止**序列化为
//!    JSON 数组——那是 M4 推倒重写的典型原因。
//! 3. 频率与数据量成反比。快照必须先按显示像素宽度降采样。
//!
//! 本 crate **不得**包含任何 DSP 算法；那属于 `auralis-dsp`。

#![forbid(unsafe_code)]

pub mod app_state;
pub mod commands;
pub mod file_commands;
pub mod ipc_bench;
pub mod selftest;

pub use selftest::{dump_reference, run_self_test};

use app_state::AppState;

/// 安装全局 `tracing` 订阅者（输出到 stderr）。
///
/// **必须只安装一次**，且 GUI 与 CLI 共用同一条路径。原先 `main.rs` 装
/// `tracing_subscriber`、`run()` 里再注册 `tauri_plugin_log`，两者都写全局
/// logger，于是插件注册时 panic：
///
/// ```text
/// PluginInitialization("log", "attempted to set a logger after the logging
/// system was already initialized")
/// ```
///
/// CLI 子命令（`--version` / `--self-test`）在 `run()` 之前就 `return`/`exit`，
/// 所以这条路径从未被走到——问题只在真正打开窗口时暴露。
///
/// 过滤器默认 `info`，可用 `RUST_LOG` 覆盖。`try_init` 的返回值被有意忽略：
/// 重复安装不是错误，第二次调用应该是无操作而不是 panic。
pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// 桌面应用入口。
///
/// # Panics
/// 图形界面初始化失败时 Tauri 会自行 panic 并退出；
/// CLI 子命令（`--version` / `--self-test`）不会走到这里。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::analyze_synthetic_signal,
            file_commands::probe_file,
            file_commands::analyze_file,
            file_commands::cancel_task,
        ])
        .setup(|_app| {
            tracing::info!(
                version = auralis_core::APP_VERSION,
                algorithm = auralis_core::ALGORITHM_VERSION,
                "Auralis Studio started"
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Auralis Studio");
}
