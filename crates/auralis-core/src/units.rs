//! 单位与对数换算。
//!
//! `DSP_SPEC.md` §3、§5 要求：dBFS 转换需要明确参考值；零值使用显示下限或
//! 明确的 `-inf` 数值语义，避免 UI 非法数值。
//!
//! ## 本项目的约定
//!
//! | 量     | 换算                        | 参考值                        |
//! |--------|-----------------------------|-------------------------------|
//! | 幅值    | `20·log10(A / 1.0)`        | 满刻度正弦（`A = 1.0`）= 0 dBFS |
//! | 功率    | `10·log10(P / 1.0)`        | `1.0 FS²` = 0 dB               |
//! | 密度    | `10·log10(S / 1.0)`        | `1.0 FS²/Hz` = 0 dBFS/Hz        |
//!
//! **FS 是数字满刻度，不是声压级。** `PRD.md` §8 明确本产品不保证校准级 SPL 测量，
//! 因此本模块不提供、也不应新增任何 `dB SPL` 换算。

/// 线性幅度为 0 时的 dBFS 下限。
///
/// `DSP_SPEC.md` §5「对数显示需定义零值下限与最小显示电平」。
/// 注意这是**显示下限**：DSP 层不把它写进结果，只在渲染时钳制。
pub const LINEAR_TO_DB_FLOOR: f32 = -140.0;

/// dB 值。
///
/// 区分两态是本项目的核心要求：
/// - 真正为零（静音）→ `Dbfs::NegativeInfinity`
/// - 只是很小但非零    → 正常的有限 `f32`
///
/// 渲染层把 `NegativeInfinity` 映射到显示下限；有值的负无穷不会被误当成静音。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dbfs {
    /// 幅度确为 0，`log10(0) = -inf`。
    NegativeInfinity,
    /// 有限 dB 值。
    Value(f32),
}

impl Dbfs {
    /// 由线性幅度构造。
    #[must_use]
    pub fn from_linear(linear: f32) -> Self {
        if linear == 0.0 {
            return Self::NegativeInfinity;
        }
        // 负幅度在物理上不存在（幅度定义上非负），但负值说明上游出了 bug。
        // 这里取绝对值而不是返回 NaN，让问题在数值上可见。
        Self::Value(20.0 * linear.abs().log10())
    }

    /// 钳到显示下限，返回可直接绘制的 `f32`。
    #[must_use]
    pub fn clamped(&self, floor: f32) -> f32 {
        match self {
            Self::NegativeInfinity => floor,
            Self::Value(v) if v.is_nan() => floor,
            Self::Value(v) => v.max(floor),
        }
    }

    /// 是否真的是静音（而非「很小」）。
    #[must_use]
    pub const fn is_silence(&self) -> bool {
        matches!(self, Self::NegativeInfinity)
    }
}

/// 线性幅度 → dBFS 浮点值，零值返回 `f64::NEG_INFINITY`。
///
/// 保留 `-inf` 供结果对象使用；需要绘制时用 [`Dbfs::clamped`]。
#[must_use]
pub fn linear_to_dbfs(linear: f32) -> f32 {
    if linear == 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.abs().log10()
    }
}

/// 线性功率 → dB，零值返回 `f64::NEG_INFINITY`。
#[must_use]
pub fn power_to_db(power: f64) -> f64 {
    if power == 0.0 {
        f64::NEG_INFINITY
    } else {
        10.0 * power.abs().log10()
    }
}

/// dBFS → 线性幅度。
#[must_use]
pub fn dbfs_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_is_zero_db() {
        assert!((linear_to_dbfs(1.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn half_scale_is_about_minus_six_db() {
        // 20·log10(0.5) = -6.0206
        assert!((linear_to_dbfs(0.5) - -6.020_6).abs() < 1e-3);
    }

    #[test]
    fn zero_produces_true_negative_infinity_not_a_big_number() {
        let db = linear_to_dbfs(0.0);
        assert!(db.is_infinite() && db.is_sign_negative(), "got {db}");
        assert_eq!(Dbfs::from_linear(0.0), Dbfs::NegativeInfinity);
        assert!(Dbfs::from_linear(0.0).is_silence());
    }

    #[test]
    fn clamped_maps_silence_to_display_floor() {
        assert_eq!(
            Dbfs::NegativeInfinity.clamped(LINEAR_TO_DB_FLOOR),
            LINEAR_TO_DB_FLOOR
        );
    }

    #[test]
    fn clamped_does_not_destroy_small_but_finite_values() {
        // 1e-7 对应 -140 dB，恰好是显示下限
        let v = Dbfs::from_linear(1e-7);
        assert!(!v.is_silence());
        assert!((v.clamped(LINEAR_TO_DB_FLOOR) - -140.0).abs() < 0.01);
    }

    #[test]
    fn round_trip_linear_dbfs() {
        for lin in [1.0_f32, 0.25, 0.001] {
            let db = linear_to_dbfs(lin);
            let back = dbfs_to_linear(db as f64) as f32;
            assert!((back - lin).abs() < 1e-6, "{lin} -> {db} -> {back}");
        }
    }

    #[test]
    fn power_uses_ten_log_not_twenty() {
        // 功率 0.25 -> -6.02 dB（与幅值 0.5 一致）
        assert!((power_to_db(0.25) - -6.020_6).abs() < 1e-3);
        assert!(power_to_db(0.0).is_infinite());
    }
}
