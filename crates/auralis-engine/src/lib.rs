//! Auralis Studio 音频引擎（Rust 侧）。
//!
//! ## 职责边界（ADR-016 / ADR-018 / ADR-019）
//!
//! 这个 crate 是**音频内核与前端之间的唯一接口层**，并且现在以**独立进程**
//! 的形式运行：Electron 主进程通过 stdio 的 JSON-Lines 协议（`src/rpc.rs`）
//! 调用这里的命令。
//!
//! 硬规则：
//! 1. React **不得**参与音频计算。dB 换算、降采样、峰值检测全部
//!    在 `auralis-dsp` 完成后才传输。
//! 2. 二进制优先。波形、频谱、声谱图以字节传输，**禁止**被序列化成
//!    JSON 数字数组。
//! 3. 频率与数据量成反比。快照必须先按显示像素宽度降采样。
//!
//! 本 crate **不得**包含任何 DSP 算法；那属于 `auralis-dsp`。
//!
//! ## 与容器解耦
//!
//! 迁移前这里叫 `auralis-desktop`，是 Tauri 的 `lib` 目标（`run()` 里建窗口）。
//! 现在只保留命令层：没有任何 UI 类型、没有窗口、没有 Tauri 依赖。
//! 命令函数的参数从 `State<'_, T>` 改成 `&T`，返回类型仍是
//! `Result<T, String>`——容器换成什么都能直接复用。

#![forbid(unsafe_code)]

pub mod app_state;
pub mod b64;
pub mod commands;
pub mod file_commands;
pub mod ipc_bench;
pub mod realtime_commands;
pub mod rpc;
pub mod selftest;

pub use selftest::{dump_reference, run_self_test};

use app_state::AppState;
use realtime_commands::RealtimeSession;

/// 引擎进程内的全部共享状态。
///
/// 两个字段对应迁移前 Tauri 的两次 `.manage(...)`。它们各自内部加锁，
/// 所以 `Engine` 可以安全地被多个工作线程共享（`&Engine` 即可调用命令）。
pub struct Engine {
    /// 任务取消令牌注册表。
    pub state: AppState,
    /// 实时采集会话（设备、工作线程、最新快照）。
    pub realtime: RealtimeSession,
}

impl Engine {
    /// 新建空引擎状态。
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            realtime: RealtimeSession::new(),
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// 安装全局 `tracing` 订阅者（输出到 stderr）。
///
/// **必须只安装一次**。stdout 是 RPC 通道，日志**绝不能**写 stdout——
/// 一行非 JSON 内容会让主进程的解析器直接报错，所以 `tracing_subscriber`
/// 用默认的 stderr 输出。
///
/// 过滤器默认 `info`，可用 `RUST_LOG` 覆盖。`try_init` 的返回值被有意忽略：
/// 重复安装不是错误，第二次调用应该是无操作而不是 panic。
pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}
