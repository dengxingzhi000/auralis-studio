//! Auralis Studio 落地层。
//!
//! 职责只有两件：把分析结果**渲染成可追溯的文本**，以及**原子地写盘**。
//! 这里不做任何 DSP：输入都是已经算好的 dB 值与样本值。
//!
//! ## 为什么单独一个 crate
//!
//! `auralis-core` / `auralis-dsp` 被 `check-boundaries.ps1` 禁止接触文件系统
//! （见 AGENTS.md「Architecture rules」）。导出必然要写文件，所以放在这里，
//! 保持内核可测试、无 I/O。
//!
//! ## 可追溯性要求
//!
//! 每份导出都必须带：应用版本、算法版本、DSP 规格修订号、单位、分析参数
//! （FFT 长度、窗函数、采样率、声道数）以及数据来源。缺任何一项，
//! 这份导出就无法在半年后被复现或解释。

#![forbid(unsafe_code)]

pub mod export;

pub use export::{
    AnalysisExport, AnalysisParameters, ExportError, ExportFormat, ExportMetadata, ExportMetrics,
    ExportPeak, render_csv, render_json, write_atomically, write_export,
};
