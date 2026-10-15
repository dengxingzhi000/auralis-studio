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

pub mod commands;
pub mod ipc_bench;
pub mod selftest;

pub use selftest::{dump_reference, run_self_test};

/// 桌面应用入口。
///
/// # Panics
/// 图形界面初始化失败时 Tauri 会自行 panic 并退出；
/// CLI 子命令（`--version` / `--self-test`）不会走到这里。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::analyze_synthetic_signal,
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
