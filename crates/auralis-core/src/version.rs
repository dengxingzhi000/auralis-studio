//! 版本与可追溯性标识。
//!
//! `PRD.md` FR-08、`DSP_SPEC.md` §8、`AGENTS.md`「DSP correctness」都要求：
//! 导出结果和 UI 脚注必须携带算法版本，否则历史结果无法解释。

/// 应用版本。与 `Cargo.toml` 的 `version.workspace` 保持一致（M0 手工同步）。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 算法规格修订号。
///
/// **任何改变 DSP 数值约定的改动都必须提升此号**，包括但不限于：
/// 窗函数定义、幅值/功率/PSD 归一化系数、dB 参考值、峰值插值方式、边界帧策略。
pub const DSP_SPEC_REVISION: &str = "2026-10-14";

/// 算法版本标识，写入指标结果与导出文件头。
///
/// 格式：`auralis-dsp/{crate 版本}+spec.{DSP 规格修订号}`
///
/// # 例子
/// `auralis-dsp/0.1.0+spec.2026-10-14`
pub const ALGORITHM_VERSION: &str = concat!(
    "auralis-dsp/",
    env!("CARGO_PKG_VERSION"),
    "+spec.2026-10-14"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_version_has_expected_shape() {
        assert!(ALGORITHM_VERSION.starts_with("auralis-dsp/"));
        assert!(ALGORITHM_VERSION.contains("+spec."));
        assert!(!DSP_SPEC_REVISION.is_empty());
    }
}
