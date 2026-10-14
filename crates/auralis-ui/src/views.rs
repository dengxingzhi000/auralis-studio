//! 视图组件。
//!
//! `docs/UI_SPEC.md` §4 要求每个视图是**独立组件**，而不是塞进一个巨型
//! `AudioChart`。本文件是 M0 的骨架版本：布局、坐标轴、空状态与参数控件
//! 都已就位，数据在 M1–M3 接入。
//!
//! ## M0 的诚实状态
//!
//! 本模块**不绘制任何假数据**。空画布显示坐标轴网格与「尚未加载音频」的提示，
//! 而不是用随机波形/合成频谱充数。理由：`AGENTS.md` 要求「不得虚构未经验证的
//! 功能」，一张漂亮但没有数据来源的图会掩盖「数据管线还没接上」这个事实。

use egui::{Align, CornerRadius, Layout, Panel, RichText, ScrollArea, Stroke, Ui, Vec2};

use crate::app::{AppState, CanvasFocus, FrequencyScale};
use crate::layout::{STATUS_BAR_HEIGHT, TOP_BAR_HEIGHT};
use crate::theme::Tokens;

/// 顶部命令栏。
pub struct TopCommandBar<'a> {
    /// Token。
    pub tokens: &'a Tokens,
    /// 应用状态。
    pub state: &'a mut AppState,
}

impl<'a> TopCommandBar<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制。
    pub fn show(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        Panel::top("auralis.top_bar")
            .exact_size(TOP_BAR_HEIGHT)
            .show_separator_line(false)
            .frame(panel_frame(t.bg_elevated, t))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("AURALIS")
                            .color(t.accent_primary)
                            .strong()
                            .monospace(),
                    );
                    ui.label(RichText::new("STUDIO").color(t.text_secondary).monospace());
                    ui.separator();

                    // M1-M5 才启用的命令。M0 明确置灰并标注里程碑，
                    // 避免用户以为按钮坏了。
                    for (label, milestone) in
                        [("导入文件", "M1"), ("开始采集", "M4"), ("导出", "M5")]
                    {
                        let response = ui.add_enabled(
                            false,
                            egui::Button::new(RichText::new(label).color(t.text_disabled)),
                        );
                        response.on_hover_text(format!("{label}：计划在 {milestone} 实现"));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{}", auralis_core::APP_VERSION))
                                .color(t.text_disabled)
                                .monospace()
                                .size(11.0),
                        );
                        ui.label(
                            RichText::new(auralis_core::ALGORITHM_VERSION)
                                .color(t.text_disabled)
                                .monospace()
                                .size(10.0),
                        );
                    });
                });
            });
    }
}

fn panel_frame(fill: egui::Color32, t: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(8.0)
        .stroke(t.panel_stroke())
}

/// 左侧资源栏：文件、实时设备、分析任务。
pub struct ResourcePanel<'a> {
    /// Token。
    pub tokens: &'a Tokens,
    /// 应用状态。
    pub state: &'a mut AppState,
}

impl<'a> ResourcePanel<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制。
    pub fn show(&mut self, ui: &mut Ui) {
        if !self.state.layout.left_panel.visible {
            return;
        }
        let t = self.tokens;
        Panel::left("auralis.left")
            .default_size(240.0)
            .resizable(true)
            .min_size(180.0)
            .frame(panel_frame(t.bg_panel, t))
            .show(ui, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    section(ui, "文件", "空 — M1 支持导入 WAV");
                    ui.add_space(8.0);
                    section(ui, "实时输入", "空 — M4 接入 WASAPI 采集");
                    ui.add_space(8.0);
                    section(ui, "分析任务", "空 — M1 接入任务调度");
                    ui.add_space(8.0);
                    section(ui, "工作区", "保存的布局将在 M5 出现");
                });
            });
    }
}

fn section(ui: &mut Ui, title: &str, empty_hint: &str) {
    let t = Tokens::dark();
    ui.label(
        RichText::new(title.to_uppercase())
            .color(t.text_secondary)
            .size(11.0)
            .strong(),
    );
    ui.label(RichText::new(empty_hint).color(t.text_disabled).size(11.5));
    ui.add_space(4.0);
}

/// 右侧参数检查器。
pub struct InspectorPanel<'a> {
    /// Token。
    pub tokens: &'a Tokens,
    /// 应用状态。
    pub state: &'a mut AppState,
}

impl<'a> InspectorPanel<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制。
    pub fn show(&mut self, ui: &mut Ui) {
        if !self.state.layout.right_panel.visible {
            return;
        }
        let t = self.tokens;
        Panel::right("auralis.right")
            .default_size(280.0)
            .resizable(true)
            .min_size(220.0)
            .frame(panel_frame(t.bg_panel, t))
            .show(ui, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    ui.label(
                        RichText::new("检查器")
                            .color(t.text_secondary)
                            .size(11.0)
                            .strong(),
                    );
                    ui.separator();

                    // FFT 点数：M0 可调（M2 接入后影响实际谱计算）
                    labeled(ui, t, "FFT 点数");
                    egui::ComboBox::from_id_salt("fft_size")
                        .selected_text(format!("{}", self.state.fft_size))
                        .show_ui(ui, |ui| {
                            for n in [1024_usize, 2048, 4096, 8192, 16_384] {
                                ui.selectable_value(&mut self.state.fft_size, n, n.to_string());
                            }
                        });

                    // 窗函数
                    labeled(ui, t, "窗函数");
                    let windows = [
                        auralis_dsp::WindowFunction::Rectangular,
                        auralis_dsp::WindowFunction::Hann,
                        auralis_dsp::WindowFunction::Hamming,
                        auralis_dsp::WindowFunction::Blackman,
                    ];
                    let selected = windows
                        .get(self.state.window_index.min(windows.len() - 1))
                        .copied()
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt("window")
                        .selected_text(selected.display_name())
                        .show_ui(ui, |ui| {
                            for (i, w) in windows.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.state.window_index,
                                    i,
                                    w.display_name(),
                                );
                            }
                        });
                    // 展示窗的归一化常数，让「专业」不只停留在下拉框
                    ui.label(
                        RichText::new(format!(
                            "CG {:.3} · ENBW {:.3} bin",
                            selected.coherent_gain(),
                            selected.enbw_bins()
                        ))
                        .color(t.text_disabled)
                        .monospace()
                        .size(10.5),
                    );

                    ui.add_space(6.0);
                    labeled(ui, t, "频率轴");
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.state.freq_scale,
                            FrequencyScale::Logarithmic,
                            "对数",
                        );
                        ui.selectable_value(
                            &mut self.state.freq_scale,
                            FrequencyScale::Linear,
                            "线性",
                        );
                    });

                    ui.add_space(6.0);
                    labeled(ui, t, "动态范围");
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:.0}", self.state.min_db))
                                .color(t.text_primary)
                                .monospace(),
                        );
                        ui.label(RichText::new("...").color(t.text_disabled));
                        ui.label(
                            RichText::new(format!("{:.0}", self.state.max_db))
                                .color(t.text_primary)
                                .monospace(),
                        );
                        ui.label(RichText::new("dBFS").color(t.text_disabled));
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.label(
                        RichText::new("信号指标")
                            .color(t.text_secondary)
                            .size(11.0)
                            .strong(),
                    );
                    for (name, milestone) in [
                        ("Peak", "M2"),
                        ("RMS", "M2"),
                        ("DC Offset", "M2"),
                        ("削波特征", "M2"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).color(t.text_disabled).monospace());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("- {milestone}"))
                                        .color(t.text_disabled)
                                        .monospace()
                                        .size(10.5),
                                );
                            });
                        });
                    }

                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("spec {}", auralis_core::DSP_SPEC_REVISION))
                            .color(t.text_disabled)
                            .size(10.0),
                    );
                });
            });
    }
}

fn labeled(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.text_secondary).size(11.5));
}

/// 中央画布：频谱 + 波形，可拖动分隔条调整比例。
pub struct CentralCanvas<'a> {
    /// Token。
    pub tokens: &'a Tokens,
    /// 应用状态。
    pub state: &'a mut AppState,
}

impl<'a> CentralCanvas<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制中央区域（不含外层 CentralPanel，由 [`crate::Workbench`] 提供）。
    pub fn show(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let spectrum_on = self.state.layout.spectrum.visible;
        let waveform_on = self.state.layout.waveform.visible;

        egui::Frame::new()
            .fill(t.bg_app)
            .inner_margin(6.0)
            .show(ui, |ui| {
                if spectrum_on && waveform_on {
                    self.split_view(ui);
                } else if spectrum_on {
                    let (rect, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    self.pane(ui, rect, "频谱", CanvasFocus::Spectrum);
                } else if waveform_on {
                    let (rect, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    self.pane(ui, rect, "波形", CanvasFocus::Waveform);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("所有中央面板已隐藏").color(t.text_disabled));
                    });
                }
            });
    }

    fn split_view(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let total = ui.available_height();
        let ratio = self.state.layout.spectrum.ratio.clamp(0.15, 0.85);
        let top_h = total * ratio;
        let sep_h = 6.0;
        let bottom_h = (total - top_h - sep_h).max(0.0);
        let width = ui.available_width();

        // 上：频谱
        let (top_rect, _) = ui.allocate_exact_size(Vec2::new(width, top_h), egui::Sense::hover());
        self.pane(ui, top_rect, "频谱", CanvasFocus::Spectrum);

        // 分隔条（可拖动）
        // egui 0.36 的 allocate_exact_size 返回 (Rect, Response)，
        // 但该 Response 是 hover 语义的；拖拽需要单独用 ui.interact 申请。
        let (sep_rect, _) = ui.allocate_exact_size(Vec2::new(width, sep_h), egui::Sense::hover());
        let sep_resp = ui.interact(
            sep_rect,
            ui.id().with("spectrum_splitter"),
            egui::Sense::drag(),
        );
        ui.painter().line_segment(
            [
                sep_rect.left_center() + Vec2::new(16.0, 0.0),
                sep_rect.right_center() - Vec2::new(16.0, 0.0),
            ],
            Stroke::new(2.0, t.border_subtle),
        );
        if sep_resp.dragged()
            && let Some(pos) = sep_resp.interact_pointer_pos()
        {
            let y = pos.y - ui.clip_rect().min.y - top_h;
            self.state.layout.spectrum.ratio = (y / total).clamp(0.15, 0.85);
        }

        // 下：波形
        let (bottom_rect, _) =
            ui.allocate_exact_size(Vec2::new(width, bottom_h), egui::Sense::hover());
        self.pane(ui, bottom_rect, "波形", CanvasFocus::Waveform);
    }

    fn pane(&mut self, ui: &mut Ui, rect: egui::Rect, title: &str, focus: CanvasFocus) {
        let t = self.tokens;
        let selected = self.state.focus == focus;
        let header_bg = if selected {
            t.accent_primary.gamma_multiply(0.18)
        } else {
            t.bg_elevated
        };

        let painter = ui.painter().clone();
        // 面板底
        painter.rect_filled(rect, CornerRadius::same(4), t.bg_panel);
        let border = if selected {
            t.accent_primary
        } else {
            t.border_subtle
        };
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, border),
            egui::StrokeKind::Middle,
        );

        // 标题条
        let header = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top() + 22.0),
        );
        painter.rect_filled(header, CornerRadius::same(4), header_bg);
        painter.line_segment(
            [header.left_bottom(), header.right_bottom()],
            Stroke::new(1.0, t.border_subtle),
        );

        painter.text(
            header.left_center() + Vec2::new(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(12.0),
            t.text_primary,
        );
        painter.text(
            header.left_center() + Vec2::new(48.0, 0.0),
            egui::Align2::LEFT_CENTER,
            match focus {
                CanvasFocus::Spectrum => "FFT amplitude spectrum",
                CanvasFocus::Waveform => "min/max envelope",
                CanvasFocus::Spectrogram => "STFT",
            },
            egui::FontId::proportional(11.0),
            t.text_disabled,
        );
        painter.text(
            header.right_center() - Vec2::new(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            if self.state.sample_rate.is_some() {
                "waiting for data"
            } else {
                "no audio loaded"
            },
            egui::FontId::proportional(10.5),
            t.text_disabled,
        );

        // 坐标画布
        let plot = egui::Rect::from_min_max(
            egui::pos2(rect.left(), header.bottom()),
            egui::pos2(rect.right(), rect.bottom()),
        );
        draw_empty_axes(&painter, plot, t, self.state);
    }
}

/// 绘制空坐标轴：网格 + 刻度 + 提示。
///
/// 刻意只画坐标轴而不画假数据。
fn draw_empty_axes(painter: &egui::Painter, rect: egui::Rect, t: &Tokens, state: &AppState) {
    let left_pad = 44.0;
    let bottom_pad = 20.0;
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + left_pad, rect.top()),
        egui::pos2(rect.right(), rect.bottom() - bottom_pad),
    );
    if plot.width() <= 1.0 || plot.height() <= 1.0 {
        return;
    }

    // 水平网格：dB 刻度
    for i in 0..=8 {
        let y = plot.bottom() - plot.height() * (i as f32 / 8.0);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, t.grid_major),
        );
        let db = t.min_db + (t.max_db - t.min_db) * (i as f32 / 8.0);
        painter.text(
            egui::pos2(rect.left() + 4.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{db:.0}"),
            egui::FontId::monospace(10.0),
            t.axis_label,
        );
    }
    // 垂直网格：频率刻度（占位，M1 拿到真实采样率后替换）
    for i in 0..=8 {
        let x = plot.left() + plot.width() * (i as f32 / 8.0);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            Stroke::new(1.0, t.grid_minor),
        );
        painter.text(
            egui::pos2(x, plot.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            format!("{}k", i * 3),
            egui::FontId::monospace(10.0),
            t.axis_label,
        );
    }

    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        Stroke::new(1.0, t.axis_line),
    );
    painter.line_segment(
        [plot.left_top(), plot.left_bottom()],
        Stroke::new(1.0, t.axis_line),
    );
    painter.text(
        egui::pos2(plot.center().x, plot.bottom() + 4.0),
        egui::Align2::CENTER_BOTTOM,
        "Hz",
        egui::FontId::monospace(10.0),
        t.axis_label,
    );

    // 中央提示
    let msg = if state.sample_rate.is_some() {
        "no decoded data yet - M1 pipeline not wired"
    } else {
        "no audio loaded - M1 will add WAV import"
    };
    painter.text(
        plot.center(),
        egui::Align2::CENTER_CENTER,
        msg,
        egui::FontId::proportional(13.0),
        t.text_disabled,
    );
}

/// 底部状态栏。
pub struct StatusBar<'a> {
    /// Token。
    pub tokens: &'a Tokens,
    /// 应用状态。
    pub state: &'a mut AppState,
}

impl<'a> StatusBar<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tokens: &'a Tokens, state: &'a mut AppState) -> Self {
        Self { tokens, state }
    }

    /// 绘制，并顺便用 egui 提供的帧间隔估算 UI 帧率。
    ///
    /// `docs/TEST_PLAN.md` §5「常规交互目标 60 FPS」——这个读数就是该目标的现场证据。
    pub fn show(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        if !self.state.layout.status_bar.visible {
            return;
        }
        let t = self.tokens;
        let dt = ctx.input(|i| i.stable_dt);
        if dt > 0.0 {
            let inst = 1.0 / dt;
            self.state.fps = if self.state.fps <= 0.0 {
                inst
            } else {
                self.state.fps * 0.9 + inst * 0.1
            };
        }

        Panel::bottom("auralis.status_bar")
            .exact_size(STATUS_BAR_HEIGHT)
            .show_separator_line(false)
            .frame(panel_frame(t.bg_elevated, t))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let sample_rate = match self.state.sample_rate {
                        Some(sr) => format!("{} Hz", thousands(sr)),
                        None => "- Hz".to_string(),
                    };
                    for (key, value) in [
                        ("采样率", sample_rate),
                        ("设备", "未连接".to_string()),
                        ("分析延迟", "-".to_string()),
                        ("任务", "0 / 0".to_string()),
                    ] {
                        ui.label(RichText::new(key).color(t.text_disabled).size(11.0));
                        ui.label(
                            RichText::new(value)
                                .color(t.text_secondary)
                                .monospace()
                                .size(11.0),
                        );
                        ui.separator();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.state.error_count > 0 {
                            ui.label(
                                RichText::new(format!("X {} 错误", self.state.error_count))
                                    .color(t.signal_critical)
                                    .monospace(),
                            );
                        }
                        if self.state.warning_count > 0 {
                            ui.label(
                                RichText::new(format!("! {} 警告", self.state.warning_count))
                                    .color(t.signal_warning)
                                    .monospace(),
                            );
                        }
                        ui.label(
                            RichText::new(format!("{:.0} FPS", self.state.fps))
                                .color(t.text_secondary)
                                .monospace(),
                        );
                    });
                });
            });
    }
}

/// 48000 -> "48 000"。等宽字体下带千分位才好对齐比较。
#[must_use]
pub fn thousands(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// 把 dB 映射到面板 y 坐标。max_db 在顶端，min_db 在底端。
#[must_use]
pub fn db_to_y(db: f32, top: f32, height: f32, min_db: f32, max_db: f32) -> f32 {
    let span = max_db - min_db;
    if span <= 0.0 {
        return top;
    }
    top + height * (1.0 - ((db - min_db) / span).clamp(0.0, 1.0))
}

/// 把归一化横坐标映射到面板 x 坐标。
#[must_use]
pub fn t_to_x(t: f32, left: f32, width: f32) -> f32 {
    left + width * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separator_is_readable() {
        assert_eq!(thousands(48_000), "48 000");
        assert_eq!(thousands(44_100), "44 100");
        assert_eq!(thousands(96_000), "96 000");
        assert_eq!(thousands(192_000), "192 000");
        assert_eq!(thousands(800), "800");
    }

    #[test]
    fn db_maps_top_of_plot_to_top() {
        let y = db_to_y(0.0, 100.0, 200.0, -140.0, 0.0);
        assert!((y - 100.0).abs() < 1e-4, "0 dBFS 应在顶端，得到 {y}");
    }

    #[test]
    fn db_maps_min_to_bottom_and_clamps_out_of_range() {
        let y = db_to_y(-140.0, 100.0, 200.0, -140.0, 0.0);
        assert!((y - 300.0).abs() < 1e-4, "得到 {y}");
        assert!(db_to_y(10.0, 100.0, 200.0, -140.0, 0.0) >= 100.0);
        assert!(db_to_y(-999.0, 100.0, 200.0, -140.0, 0.0) <= 300.0);
    }

    #[test]
    fn db_mapping_is_monotonic() {
        let mut prev = db_to_y(0.0, 0.0, 100.0, -140.0, 0.0);
        for step in 1..=140 {
            let y = db_to_y(-(step as f32), 0.0, 100.0, -140.0, 0.0);
            assert!(y > prev, "dB 降低时 y 应增大：{step} dB");
            prev = y;
        }
    }

    #[test]
    fn x_mapping_clamps_to_plot_width() {
        assert!((t_to_x(0.0, 10.0, 100.0) - 10.0).abs() < 1e-5);
        assert!((t_to_x(1.0, 10.0, 100.0) - 110.0).abs() < 1e-5);
        assert!((t_to_x(-1.0, 10.0, 100.0) - 10.0).abs() < 1e-5);
        assert!((t_to_x(2.0, 10.0, 100.0) - 110.0).abs() < 1e-5);
    }

    #[test]
    fn degenerate_db_range_does_not_divide_by_zero() {
        let y = db_to_y(-6.0, 42.0, 200.0, 0.0, 0.0);
        assert!(y.is_finite());
        assert!((y - 42.0).abs() < 1e-6);
    }
}
