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
//! | 容器 | symphonia feature | 编译进产物 | **已用签入样本验证** |
//! |---|---|---|---|
//! | WAV  | `wav`（default） | 是 | **是** |
//! | FLAC | `flac`（default） | 是 | **是** |
//! | AIFF | `aiff`（显式开启） | 是 | **是** |
//! | MP3  | `mp3`（显式开启） | 是 | **否** |
//! | OGG/Vorbis | default | 是 | **否** |
//!
//! **「已验证」列是给 UI 的硬约束**：只有 WAV / FLAC / AIFF 可以宣称支持。
//! 其余格式能解码，但没有样本验证，不得在界面上承诺。
//!
//! 样本在 `tests/fixtures/`，需要更新时手动跑一次生成器并提交：
//!
//! ```text
//! cargo test -p auralis-codec -- --ignored regenerate_flac_fixtures
//! cargo test -p auralis-codec -- --ignored regenerate_aiff_fixtures
//! ```
//!
//! **样本来源的强度不同，文档不得混为一谈**：
//! - FLAC 样本由第三方编码器 `flacenc`（仅 dev-dependency）生成；
//! - WAV / AIFF 样本由本 crate 的手写 writer 生成。writer 与 symphonia
//!   是两份独立实现，因此仍能验证「我们对格式的理解与解码器一致」，
//!   但这**不是**第三方工具导出的、或来自现实世界的文件。
//!
//! MP3 之所以仍未验证：本机没有 `ffmpeg` / `lame` / `sox`，
//! 无法在不下载第三方二进制的前提下造出可复现样本。
//!
//! 特别注意：symphonia 的 default features **不含** mp3 / aiff / aacc。
//! 这是实测结论，不是推测 —— 见根 `Cargo.toml` 的注释。

#![forbid(unsafe_code)]

pub mod error;
pub mod info;
pub mod reader;
pub mod wav_writer;

/// FLAC 编码器：只用于生成测试样本（dev-dependency，不进产物）。
#[cfg(test)]
pub mod flac_writer;

/// AIFF 生成器：只用于生成测试样本（手写，不引入额外依赖）。
#[cfg(test)]
pub mod aiff_writer;

/// 测试样本（`tests/fixtures/`）里的文件路径。
///
/// # Panics
/// 不可能：`CARGO_MANIFEST_DIR` 在编译期确定。
#[cfg(test)]
#[must_use]
pub fn fixture_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

pub use error::{CodecError, Result};
pub use info::{AudioFileInfo, ContainerKind, probe, verified_containers};
pub use reader::{BlockDecoder, DecodedBlock};

#[cfg(test)]
mod integration;
