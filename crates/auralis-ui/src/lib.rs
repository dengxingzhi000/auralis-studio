//! # auralis-ui
//!
//! egui 界面层：设计 Token、布局骨架与视图组件。
//!
//! ## 边界规则
//!
//! - 本 crate **不得**执行解码、FFT 或任何阻塞计算（`AGENTS.md` 架构规则）。
//!   它只消费已经算好的不可变快照。
//! - 只允许本 crate 与 `apps/auralis-desktop` 依赖 `eframe` / `egui`。
//! - 所有颜色必须来自 [`theme`]`，禁止在组件里硬编码颜色字面量
//!   （`docs/UI_SPEC.md` §2）。

#![forbid(unsafe_code)]

pub mod app;
pub mod layout;
pub mod theme;
pub mod views;

pub use app::{AppState, WorkspaceLayout};
pub use layout::Workbench;
pub use theme::{ChannelStyle, Tokens};
pub use views::{CentralCanvas, InspectorPanel, ResourcePanel, StatusBar, TopCommandBar};
