//! 应用状态与工作区布局持久化模型。
//!
//! `docs/PRD.md` FR-01：「面板可隐藏、调整大小；布局在应用重启后恢复。」
//! 本模块只定义**数据模型**；持久化读写在 `auralis-app`（M1/M5）。
//!
//! ## 为什么状态是普通数据
//!
//! 整个状态结构可序列化，因此「保存工作区」不需要单独写一遍字段映射，
//! 「恢复工作区」也不需要反向映射。这是刻意的设计取舍。

use serde::{Deserialize, Serialize};

/// 单个面板的可见性与尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PanelState {
    /// 是否显示。`false` 时面板被折叠成细条。
    pub visible: bool,
    /// 在其所在轴上的相对权重（0.0–1.0，同组面板自动归一化）。
    pub ratio: f32,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            visible: true,
            ratio: 0.5,
        }
    }
}

impl PanelState {
    /// 构造一个面板。
    #[must_use]
    pub const fn new(visible: bool, ratio: f32) -> Self {
        Self { visible, ratio }
    }
}

/// 工作区布局。
///
/// M0 固化五区结构（`docs/UI_SPEC.md` §3）。后续里程碑新增的面板
/// （声谱图、频段能量等）在这里追加字段。
///
/// 结构体带 `#[serde(default)]`，因此**旧配置文件仍能被读取**——
/// 缺失的字段回落到 [`Default`]，而不是报错。这是
/// `docs/PRD.md`「配置具有版本兼容策略」的落地方式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceLayout {
    /// 顶部命令栏。
    pub top_bar: PanelState,
    /// 左侧资源栏。
    pub left_panel: PanelState,
    /// 右侧参数检查器。
    pub right_panel: PanelState,
    /// 底部状态栏。
    pub status_bar: PanelState,
    /// 中央画布上方的频谱面板。
    pub spectrum: PanelState,
    /// 中央画布下方的波形面板。
    pub waveform: PanelState,
}

impl Default for WorkspaceLayout {
    /// M0 默认布局：左右侧栏收窄、中央频谱占约 60%、波形约 40%。
    fn default() -> Self {
        Self {
            top_bar: PanelState::new(true, 0.0),
            left_panel: PanelState::new(true, 0.17),
            right_panel: PanelState::new(true, 0.20),
            status_bar: PanelState::new(true, 0.0),
            spectrum: PanelState::new(true, 0.6),
            waveform: PanelState::new(true, 0.4),
        }
    }
}

/// 中央画布当前聚焦的视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CanvasFocus {
    /// 频谱（默认）。M3 接入真实数据。
    #[default]
    Spectrum,
    /// 波形。
    Waveform,
    /// 声谱图。M2 实现。
    Spectrogram,
}

/// 频谱显示模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrequencyScale {
    /// 线性频率轴。
    Linear,
    /// 对数频率轴（默认，符合专业音频工具惯例）。
    #[default]
    Logarithmic,
}

/// 应用运行时状态。
///
/// 只放 UI 层自己的、易失的交互状态。音频与分析数据是**不可变快照**，
/// 由分析层产出后交给 UI 读取（`AGENTS.md`：「UI 只读不可变快照」）。
#[derive(Debug, Clone, Default)]
pub struct AppState {
    /// 面板布局。
    pub layout: WorkspaceLayout,
    /// 中央画布焦点视图。
    pub focus: CanvasFocus,
    /// 当前 FFT 点数。
    pub fft_size: usize,
    /// 当前窗函数序号（对应 `WindowFunction` 的枚举顺序，M2 改为强类型）。
    pub window_index: usize,
    /// 频率轴模式。
    pub freq_scale: FrequencyScale,
    /// 动态范围下限，dBFS。
    pub min_db: f32,
    /// 动态范围上限，dBFS。
    pub max_db: f32,
    /// 全局时间游标，帧偏移。**M0 未接入真实数据，保持 None。**
    pub cursor_frame: Option<u64>,
    /// 状态栏警告条数。
    pub warning_count: usize,
    /// 状态栏错误条数。
    pub error_count: usize,
    /// 当前帧率（UI 线程测得），用于状态栏。M0 起可用。
    pub fps: f32,
    /// 当前文件的采样率；`None` 表示未打开文件。
    pub sample_rate: Option<u32>,
}

impl AppState {
    /// M0 默认状态。
    #[must_use]
    pub fn new() -> Self {
        Self {
            layout: WorkspaceLayout::default(),
            focus: CanvasFocus::default(),
            fft_size: 4096,
            window_index: 1, // Hann
            freq_scale: FrequencyScale::default(),
            min_db: auralis_core::units::LINEAR_TO_DB_FLOOR,
            max_db: 0.0,
            cursor_frame: None,
            warning_count: 0,
            error_count: 0,
            fps: 0.0,
            sample_rate: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_matches_five_region_spec() {
        let l = WorkspaceLayout::default();
        assert!(l.top_bar.visible);
        assert!(l.left_panel.visible);
        assert!(l.right_panel.visible);
        assert!(l.status_bar.visible);
        assert!(l.spectrum.visible);
        assert!(l.waveform.visible);
        // 频谱占比应大于波形（频谱是主视图）
        assert!(l.spectrum.ratio > l.waveform.ratio);
    }

    #[test]
    fn layout_round_trips_through_json() {
        let mut l = WorkspaceLayout::default();
        l.left_panel.visible = false;
        l.waveform.ratio = 0.25;
        let json = serde_json::to_string(&l).unwrap();
        let back: WorkspaceLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(l, back);
    }

    #[test]
    fn layout_tolerates_missing_future_fields() {
        // 模拟「旧配置文件缺少新增字段」：只反序列化一个已知子集
        let json = r#"{"top_bar":{"visible":true,"ratio":0.0}}"#;
        let l: WorkspaceLayout = serde_json::from_str(json).unwrap();
        assert!(l.top_bar.visible);
        // 缺失字段应回落到 default 而不是报错
        assert!(l.left_panel.visible);
        assert!((l.spectrum.ratio - 0.6).abs() < 1e-6);
    }

    #[test]
    fn app_state_defaults_are_sane() {
        let s = AppState::new();
        assert_eq!(s.fft_size, 4096);
        assert_eq!(s.freq_scale, FrequencyScale::Logarithmic);
        assert!(s.min_db < s.max_db);
        assert!(s.cursor_frame.is_none(), "M0 没有音频数据，不应有游标");
        assert!(s.sample_rate.is_none());
    }
}
