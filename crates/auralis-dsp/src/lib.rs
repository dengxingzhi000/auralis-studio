//! # auralis-dsp
//!
//! Auralis Studio 的纯数值内核。**不得**依赖 UI、设备 API、文件系统或数据库。
//!
//! 本 crate 存在的首要理由：让数值算法可以在没有图形界面的情况下被完整测试。
//! 任何时候 `cargo test -p auralis-dsp` 都应当在无显示环境下全绿。
//!
//! ## 数值约定（M0 已实现部分）
//!
//! 详见 `docs/DSP_SPEC.md` 与模块文档。所有约定都带确定性单元测试。

#![forbid(unsafe_code)]

pub mod stats;
pub mod window;

pub use stats::{ClippingConfig, ClippingFeatures, DcOffset, PeakLevel, Rms, SignalStats};
pub use stats::{analyze_channel, detect_flat_top};
pub use window::WindowFunction;
