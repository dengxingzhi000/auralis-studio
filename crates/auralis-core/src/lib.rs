//! # auralis-core
//!
//! Auralis Studio 的共享词汇表：领域类型、错误、时间轴和单位约定。
//!
//! ## 边界规则（强制）
//!
//! 本 crate **不得**依赖 UI、设备 API、文件系统或数据库。
//! 具体地说：不得出现 `egui` / `eframe` / `cpal` / `symphonia` / `std::fs`。
//! CI 中的 `scripts/check-boundaries.ps1` 会检查这一点。
//!
//! 允许的依赖只有 `serde`、`thiserror`，以及在后续里程碑中经过论证后加入的类型库。

#![forbid(unsafe_code)]

pub mod error;
pub mod format;
pub mod time;
pub mod units;
pub mod version;

pub use error::{AuralisError, ConfigError, Result};
pub use format::{AudioFormat, ChannelLayout, PcmBlock, PcmFormatError};
pub use time::AudioTime;
pub use units::{Dbfs, LINEAR_TO_DB_FLOOR};
pub use version::{ALGORITHM_VERSION, APP_VERSION, DSP_SPEC_REVISION};
