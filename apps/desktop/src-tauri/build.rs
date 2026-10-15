//! Tauri 构建脚本。
//!
//! `tauri_build::build()` 负责生成 `gen/schemas/` 并校验
//! `tauri.conf.json` 与 capability 文件。
//!
//! 注意：本 crate 声明了 `missing_docs = "warn"`，而 build script
//! 是独立 crate，不适用该 lint；此处显式关闭以免产生无意义告警。

#![allow(missing_docs)]

fn main() {
    tauri_build::build()
}
