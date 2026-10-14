//! 设计 Token。
//!
//! `docs/UI_SPEC.md` §2：「将 token 集中定义，禁止将颜色常量散落在所有组件中。」
//! 本模块是该要求的唯一落点。组件只允许引用 [`Tokens`] 的字段。
//!
//! ## 颜色之外的第二编码通道
//!
//! `docs/UI_SPEC.md` §6 要求重要状态「不能只通过颜色表达」。
//! 因此声道配色采用 **颜色 + 线型** 双编码（[`ChannelStyle`]），
//! 声道在色觉差异下仍可区分；状态标签同时带图标字符。

use egui::{Color32, Stroke, Visuals};

/// 声道视觉样式：颜色 + 线型。
///
/// 仅靠颜色区分声道对色觉差异用户不可用；`UI_SPEC.md` §2 明确要求兼顾。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelStyle {
    /// 主色。
    pub color: Color32,
    /// 线型。
    pub dash: DashPattern,
    /// 该声道的显示名后缀。
    pub label: &'static str,
}

/// 线型。避免依赖 egui 私有类型，保持本 crate 的样式定义自洽。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashPattern {
    /// 实线。
    Solid,
    /// 虚线。
    Dashed,
    /// 点线。
    Dotted,
    /// 点划线。
    DashDot,
    /// 密实线。
    Dense,
    /// 疏线。
    Sparse,
}

impl DashPattern {
    /// 返回以像素为单位的虚线分段（`[画, 留, 画, 留, ...]`）。
    ///
    /// [`DashPattern::Solid`] 返回空 `Vec`，交给 egui 画实线。
    #[must_use]
    pub fn stroke_dash(self) -> Vec<f32> {
        match self {
            Self::Solid => Vec::new(),
            Self::Dashed => vec![8.0, 5.0],
            Self::Dotted => vec![2.0, 3.0],
            Self::DashDot => vec![8.0, 4.0, 2.0, 4.0],
            Self::Dense => vec![5.0, 2.0],
            Self::Sparse => vec![12.0, 8.0],
        }
    }

    /// 仅用于测试的稳定排序键。
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Solid => 0,
            Self::Dense => 1,
            Self::Dashed => 2,
            Self::Sparse => 3,
            Self::Dotted => 4,
            Self::DashDot => 5,
        }
    }
}

/// Auralis Studio 深色主题 Token。
///
/// 取值来自 `docs/UI_SPEC.md` §2。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// `bg.app` — 应用背景。
    pub bg_app: Color32,
    /// `bg.panel` — 面板背景。
    pub bg_panel: Color32,
    /// `bg.elevated` — 弹层、悬浮工具栏。
    pub bg_elevated: Color32,
    /// `border.subtle` — 分割线与面板边框。
    pub border_subtle: Color32,
    /// 边框聚焦色（accent 的暗化变体）。
    pub border_focus: Color32,
    /// `text.primary` — 主文本。
    pub text_primary: Color32,
    /// `text.secondary` — 辅助信息。
    pub text_secondary: Color32,
    /// `text.disabled` — 禁用态。
    pub text_disabled: Color32,
    /// `accent.primary` — 主操作、选中态。
    pub accent_primary: Color32,
    /// `signal.positive` — 正常状态。
    pub signal_positive: Color32,
    /// `signal.warning` — 阈值警告。
    pub signal_warning: Color32,
    /// `signal.critical` — 削波、设备异常。
    pub signal_critical: Color32,

    /// 图表主网格线。
    pub grid_major: Color32,
    /// 图表次网格线。
    pub grid_minor: Color32,
    /// 坐标轴线。
    pub axis_line: Color32,
    /// 坐标轴标签。
    pub axis_label: Color32,
    /// 游标线。
    pub cursor: Color32,
    /// 选区覆盖层。
    pub selection_fill: Color32,

    /// 声谱图色阶（inferno）。
    pub spectrogram_lut: &'static [Color32],

    /// dB 显示下限。
    pub min_db: f32,
    /// dB 显示上限。
    pub max_db: f32,
}

impl Tokens {
    /// 由 `docs/UI_SPEC.md` §2 定义的取值构造。
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            bg_app: Color32::from_rgb(0x10, 0x13, 0x18),
            bg_panel: Color32::from_rgb(0x18, 0x1D, 0x25),
            bg_elevated: Color32::from_rgb(0x20, 0x27, 0x33),
            border_subtle: Color32::from_rgb(0x30, 0x39, 0x46),
            border_focus: Color32::from_rgb(0x2F, 0x63, 0xA8),
            text_primary: Color32::from_rgb(0xE7, 0xEB, 0xF1),
            text_secondary: Color32::from_rgb(0x98, 0xA3, 0xB3),
            text_disabled: Color32::from_rgb(0x5B, 0x64, 0x72),
            accent_primary: Color32::from_rgb(0x57, 0xA6, 0xFF),
            signal_positive: Color32::from_rgb(0x54, 0xD6, 0xA0),
            signal_warning: Color32::from_rgb(0xF2, 0xB7, 0x5B),
            signal_critical: Color32::from_rgb(0xFF, 0x65, 0x7A),

            grid_major: Color32::from_rgb(0x26, 0x2D, 0x38),
            grid_minor: Color32::from_rgb(0x1D, 0x23, 0x2C),
            axis_line: Color32::from_rgb(0x3A, 0x43, 0x53),
            axis_label: Color32::from_rgb(0x98, 0xA3, 0xB3),
            cursor: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            selection_fill: Color32::from_rgba_premultiplied(0x57, 0xA6, 0xFF, 38),

            spectrogram_lut: &INFERNO_LUT,
            min_db: -140.0,
            max_db: 0.0,
        }
    }

    /// 生成第 `index` 个声道的样式，循环复用前 6 组。
    ///
    /// 超过 6 声道时压低 alpha 并改用虚线，使重复样式仍可辨。
    #[must_use]
    pub fn channel(&self, index: usize) -> ChannelStyle {
        const STYLES: [ChannelStyle; 6] = [
            ChannelStyle {
                color: Color32::from_rgb(0x57, 0xA6, 0xFF),
                dash: DashPattern::Solid,
                label: "1",
            },
            ChannelStyle {
                color: Color32::from_rgb(0xF2, 0xB7, 0x5B),
                dash: DashPattern::Dashed,
                label: "2",
            },
            ChannelStyle {
                color: Color32::from_rgb(0x54, 0xD6, 0xA0),
                dash: DashPattern::Dotted,
                label: "3",
            },
            ChannelStyle {
                color: Color32::from_rgb(0xC7, 0x92, 0xEA),
                dash: DashPattern::DashDot,
                label: "4",
            },
            ChannelStyle {
                color: Color32::from_rgb(0xFF, 0x8F, 0xA3),
                dash: DashPattern::Dense,
                label: "5",
            },
            ChannelStyle {
                color: Color32::from_rgb(0x7F, 0xD1, 0xD9),
                dash: DashPattern::Sparse,
                label: "6",
            },
        ];
        let base = STYLES[index % STYLES.len()];
        let cycle = index / STYLES.len();
        if cycle == 0 {
            base
        } else {
            ChannelStyle {
                color: base.color.gamma_multiply(1.0 - 0.18 * cycle as f32),
                dash: DashPattern::Dashed,
                label: base.label,
            }
        }
    }

    /// 把 dB 值映射成声谱图色阶上的颜色。
    #[must_use]
    pub fn map_db_to_color(&self, db: f32) -> Color32 {
        let span = self.max_db - self.min_db;
        if span <= 0.0 {
            return self.spectrogram_lut[0];
        }
        let t = ((db - self.min_db) / span).clamp(0.0, 1.0);
        let idx = ((t * (self.spectrogram_lut.len() - 1) as f32).round()) as usize;
        self.spectrogram_lut[idx.min(self.spectrogram_lut.len() - 1)]
    }

    /// 面板边框描边。
    #[must_use]
    pub fn panel_stroke(&self) -> Stroke {
        Stroke::new(1.0, self.border_subtle)
    }

    /// 数值（dB、频率、采样率）使用的字体族。
    ///
    /// `UI_SPEC.md` §2 要求数值用等宽字体，便于对齐比较小数点与位数。
    #[must_use]
    pub const fn mono_text(&self) -> egui::TextStyle {
        egui::TextStyle::Monospace
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::dark()
    }
}

/// 把 Token 应用到 egui 全局 [`Visuals`]。
///
/// `dark()` 主题在此集中构造；组件不应直接修改 `Visuals`。
///
/// 字段名与 egui 0.36 的 `Visuals` / `WidgetVisuals` 严格对应。
/// 0.36 移除了 `err_strip_color`、`warn_strip_color`、`window_fill_stroke`、
/// `WidgetVisuals::weak_bg_stroke` 与 `Widgets::dark`，因此不能设置这些字段。
pub fn apply_to_visuals(visuals: &mut Visuals, tokens: &Tokens) {
    // --- 背景与文本 ---
    visuals.override_text_color = Some(tokens.text_primary);
    visuals.panel_fill = tokens.bg_app;
    visuals.window_fill = tokens.bg_elevated;
    visuals.extreme_bg_color = tokens.bg_panel;
    visuals.faint_bg_color = tokens.bg_panel;
    visuals.code_bg_color = tokens.bg_elevated;
    visuals.hyperlink_color = tokens.accent_primary;

    // --- 选中态 ---
    visuals.selection.bg_fill = tokens.accent_primary.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, tokens.accent_primary);

    // --- 禁用态 ---
    visuals.disabled_alpha = 0.45;

    // --- 警告 / 错误前景色（0.36 用 fg_color 而非 strip 色带）---
    visuals.warn_fg_color = tokens.signal_warning;
    visuals.error_fg_color = tokens.signal_critical;

    // --- 窗口与菜单 ---
    visuals.window_stroke = Stroke::new(1.0, tokens.border_subtle);
    visuals.window_corner_radius = egui::CornerRadius::same(4);
    visuals.menu_corner_radius = egui::CornerRadius::same(4);

    // --- 杂项 ---
    visuals.striped = false;
    visuals.slider_trailing_fill = true;

    // --- 控件各状态 ---
    // 0.36 的 WidgetVisuals 只有 bg_fill / weak_bg_fill / bg_stroke /
    // corner_radius / fg_stroke / expansion，没有 weak_bg_stroke。
    let w = &mut visuals.widgets;

    w.noninteractive.bg_fill = tokens.bg_panel;
    w.noninteractive.weak_bg_fill = tokens.bg_panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border_subtle);
    w.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text_secondary);

    w.inactive.bg_fill = tokens.bg_elevated;
    w.inactive.weak_bg_fill = tokens.bg_elevated;
    w.inactive.bg_stroke = Stroke::new(1.0, tokens.border_subtle);
    w.inactive.fg_stroke = Stroke::new(1.0, tokens.text_primary);

    w.hovered.bg_fill = tokens.bg_elevated.gamma_multiply(1.12);
    w.hovered.weak_bg_fill = tokens.bg_elevated.gamma_multiply(1.12);
    w.hovered.bg_stroke = Stroke::new(1.0, tokens.border_focus);
    w.hovered.fg_stroke = Stroke::new(1.0, tokens.text_primary);

    w.active.bg_fill = tokens.accent_primary.gamma_multiply(0.55);
    w.active.weak_bg_fill = tokens.accent_primary.gamma_multiply(0.55);
    w.active.bg_stroke = Stroke::new(1.0, tokens.accent_primary);
    w.active.fg_stroke = Stroke::new(1.5, tokens.text_primary);

    w.open.bg_fill = tokens.bg_elevated;
    w.open.weak_bg_fill = tokens.bg_elevated;
    w.open.bg_stroke = Stroke::new(1.0, tokens.border_focus);
    w.open.fg_stroke = Stroke::new(1.0, tokens.text_primary);
}

/// inferno 色阶，256 级，感知均匀且色觉安全。
///
/// 索引 0 = 最低 dB（近黑紫），索引 255 = 最高 dB（亮黄白）。
static INFERNO_LUT: [Color32; 256] = build_inferno_lut();

const fn build_inferno_lut() -> [Color32; 256] {
    // 17 个控制点的 inferno 采样，线性插值到 256 级。
    const STOPS: [(f32, f32, f32); 17] = [
        (0.001, 0.000, 0.014),
        (0.044, 0.033, 0.141),
        (0.142, 0.058, 0.276),
        (0.258, 0.039, 0.406),
        (0.367, 0.075, 0.432),
        (0.472, 0.110, 0.428),
        (0.578, 0.148, 0.404),
        (0.682, 0.195, 0.359),
        (0.780, 0.254, 0.300),
        (0.865, 0.332, 0.229),
        (0.929, 0.437, 0.142),
        (0.968, 0.553, 0.080),
        (0.988, 0.673, 0.056),
        (0.988, 0.789, 0.146),
        (0.976, 0.878, 0.310),
        (0.949, 0.960, 0.585),
        (0.988, 0.998, 0.645),
    ];
    let n = STOPS.len();
    let mut lut = [Color32::from_rgb(0, 0, 0); 256];
    let mut i = 0;
    while i < 256 {
        let t = i as f32 / 255.0 * (n - 1) as f32;
        let lo = t as usize;
        let hi = if lo + 1 < n { lo + 1 } else { n - 1 };
        let f = t - lo as f32;
        let (r0, g0, b0) = STOPS[lo];
        let (r1, g1, b1) = STOPS[hi];
        lut[i] = Color32::from_rgb(
            ((r0 + (r1 - r0) * f) * 255.0 + 0.5) as u8,
            ((g0 + (g1 - g0) * f) * 255.0 + 0.5) as u8,
            ((b0 + (b1 - b0) * f) * 255.0 + 0.5) as u8,
        );
        i += 1;
    }
    lut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_styles_are_distinguishable_by_line_pattern_not_just_color() {
        let t = Tokens::dark();
        let mut ranks: Vec<u8> = (0..6).map(|i| t.channel(i).dash.rank()).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), 6, "6 个声道应有 6 种可区分的线型");
    }

    #[test]
    fn channel_styles_wrap_but_stay_distinguishable() {
        let t = Tokens::dark();
        // 第 7 声道复用第 1 个色相但改线型并降低 alpha
        assert_ne!(t.channel(0).color, t.channel(6).color);
        assert_eq!(t.channel(0).label, t.channel(6).label);
    }

    #[test]
    fn inferno_lut_is_monotonically_brighter() {
        let lut = Tokens::dark().spectrogram_lut;
        assert_eq!(lut.len(), 256);
        let luma =
            |c: &Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
        for i in 1..lut.len() {
            assert!(
                luma(&lut[i]) >= luma(&lut[i - 1]) - 1.0,
                "色阶在 {i} 处亮度回退，感知均匀性被破坏"
            );
        }
        assert!(luma(&lut[0]) < 12.0);
        assert!(luma(&lut[255]) > 220.0);
    }

    #[test]
    fn db_maps_to_lut_bounds_and_clamps() {
        let t = Tokens::dark();
        assert_eq!(t.map_db_to_color(t.min_db), t.spectrogram_lut[0]);
        assert_eq!(t.map_db_to_color(t.max_db), t.spectrogram_lut[255]);
        assert_eq!(t.map_db_to_color(-9999.0), t.spectrogram_lut[0]);
        assert_eq!(t.map_db_to_color(9999.0), t.spectrogram_lut[255]);
    }

    #[test]
    fn dash_patterns_produce_valid_stroke_arrays() {
        assert!(DashPattern::Solid.stroke_dash().is_empty());
        for p in [
            DashPattern::Dashed,
            DashPattern::Dotted,
            DashPattern::DashDot,
            DashPattern::Dense,
            DashPattern::Sparse,
        ] {
            let d = p.stroke_dash();
            assert!(!d.is_empty());
            assert_eq!(d.len() % 2, 0, "{p:?} 的虚线数组应为画/留交替");
            assert!(d.iter().all(|v| *v > 0.0));
        }
    }

    #[test]
    fn applying_visuals_sets_expected_fields() {
        let t = Tokens::dark();
        let mut v = Visuals::dark();
        apply_to_visuals(&mut v, &t);
        assert_eq!(v.panel_fill, t.bg_app);
        assert_eq!(v.override_text_color, Some(t.text_primary));
        assert_eq!(v.warn_fg_color, t.signal_warning);
        assert_eq!(v.error_fg_color, t.signal_critical);
    }
}
