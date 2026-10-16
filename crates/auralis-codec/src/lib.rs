//! # auralis-codec
//!
//! 音频文件探测与**分块**解码。
//!
//! ## 边界规则（强制）
//!
//! 本 crate 可以访问文件系统（那是它的职责），但**不得**依赖 UI、
//! 设备 API 或数据库。它产出 [`auralis_core::PcmBlock`]，
//! 之后的一切都与文件无关。
//!
//! ## 格式支持矩阵 —— 不要凭库名承诺
//!
//! `TEST_PLAN.md` §3 要求「每种宣称支持的容器/编码组合」都要有真实样本测试。
//! 本 crate 的能力由 `Cargo.toml` 的 symphonia features 决定，**已实测**：
//!
//! | 容器 | symphonia feature | 编译进产物 | **已用真实样本验证** |
//! |---|---|---|---|
//! | WAV  | `wav`（default） | 是 | **是** |
//! | FLAC | `flac`（default） | 是 | **否** |
//! | MP3  | `mp3`（显式开启） | 是 | **否** |
//! | AIFF | `aiff`（显式开启） | 是 | **否** |
//! | OGG/Vorbis | default | 是 | **否** |
//!
//! **「已验证」列是给 UI 的硬约束**：只有 WAV 可以宣称支持。
//! 其余格式能解码，但没有样本验证，不得在界面上承诺。
//!
//! 特别注意：symphonia 的 default features **不含** mp3 / aiff / aacc。
//! 这是实测结论，不是推测 —— 见根 `Cargo.toml` 的注释。

#![forbid(unsafe_code)]

pub mod error;
pub mod info;
pub mod reader;
pub mod wav_writer;

pub use error::{CodecError, Result};
pub use info::{AudioFileInfo, ContainerKind, probe, verified_containers};
pub use reader::{BlockDecoder, DecodedBlock};

#[cfg(test)]
mod integration;
