//! 布局骨架：五区工作台。
//!
//! `docs/UI_SPEC.md` §3 要求的五个区域：
//!
//! ```text
//! +-----------------------------------------------+
//! | 顶部命令栏  导入 / 采集 / 导出                  |
//! +--------+---------------------------+---------+
//! | 资源栏 |  中央画布                 | 检查器  |
//! | 文件   |  +---------+------------+  | 参数    |
//! | 设备   |  | 频谱     | 指标       |  | 显示    |
//! | 任务   |  +---------+------------+  |         |
//! |        |  | 波形                  |  |         |
//! +--------+---------------------------+---------+
//! | 状态栏  采样率 / 设备 / 延迟 / 任务 / 警告       |
//! +-----------------------------------------------+
//! ```
//!
//! ## egui 0.36 的面板模型
//!
//! 0.36 把 `SidePanel` / `TopBottomPanel` 合并为单一的 [`egui::Panel`]，
//! 且 `Panel::show` 接收父 `Ui` 而非 `Context`。因此本模块先取一个根 `Ui`
//! （由调用方，即 `eframe::App::ui`，提供），再在其内部依次摆放五个面板。
//!
//! ## 为什么手写而不是用 dock 库
//!
//! M0 的布局是**固定的五区 + 一个竖向分隔条**，不是自由停靠。
//! 引入 dock 库会带来一个额外依赖、一套自己的主题接入问题，
//! 以及和 [`crate::WorkspaceLayout`] 序列化模型冲突的第二套状态。
//! 等到 M4 真的需要自由停靠时再评估（记录在 `docs/ADR.md` ADR-010）。

use crate::app::AppState;
use crate::theme::Tokens;
use crate::views::{CentralCanvas, InspectorPanel, ResourcePanel, StatusBar, TopCommandBar};

/// 五区工作台。
pub struct Workbench<'a> {
    /// 只读 Token。
    pub tokens: &'a Tokens,
    /// 可变应用状态。
    pub state: &'a mut AppState,
}

impl<'a> Workbench<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制整个工作台。
    ///
    /// `ui` 由 `eframe::App::ui` 提供，是本帧的根 `Ui`。
    pub fn show(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let tokens = self.tokens;

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(tokens.bg_app).inner_margin(0.0))
            .show(ui, |ui| {
                TopCommandBar::new(tokens, self.state).show(ui);
                ResourcePanel::new(tokens, self.state).show(ui);
                InspectorPanel::new(tokens, self.state).show(ui);
                CentralCanvas::new(tokens, self.state).show(ui);
                StatusBar::new(tokens, self.state).show(ctx, ui);
            });
    }
}

/// 面板是否可见的辅助查询，供折叠按钮复用。
#[must_use]
pub fn is_panel_visible(layout: &crate::WorkspaceLayout, name: &str) -> bool {
    match name {
        "left" => layout.left_panel.visible,
        "right" => layout.right_panel.visible,
        "status" => layout.status_bar.visible,
        "spectrum" => layout.spectrum.visible,
        "waveform" => layout.waveform.visible,
        _ => true,
    }
}

/// 顶部命令栏高度（像素）。[`crate::views::TopCommandBar`] 通过
/// `Panel::top(...).exact_size(TOP_BAR_HEIGHT)` 使用它，因此改这里即可改布局，
/// 不存在"常量与实际值不同步"的隐患。
pub const TOP_BAR_HEIGHT: f32 = 32.0;

/// 底部状态栏高度（像素）。使用方式同上。
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkspaceLayout;

    #[test]
    fn workbench_constructs_without_a_context() {
        // 布局数据本身不触碰 egui Context，可以脱离 GUI 测试
        let tokens = Tokens::dark();
        let mut state = AppState::new();
        let wb = Workbench::new(&tokens, &mut state);
        assert!(wb.state.layout.top_bar.visible);
    }

    #[test]
    fn panel_visibility_lookup_matches_defaults() {
        let l = WorkspaceLayout::default();
        for name in ["left", "right", "status", "spectrum", "waveform"] {
            assert!(is_panel_visible(&l, name), "{name} 应默认可见");
        }
        assert!(is_panel_visible(&l, "unknown-panel"));
    }
}
