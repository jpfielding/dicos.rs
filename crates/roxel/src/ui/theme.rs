//! Shared visual language for the inspection workspace.
use egui::{Color32, FontId, RichText, Stroke, TextStyle};
pub const BG: Color32 = Color32::from_rgb(16, 22, 29);
pub const PANEL: Color32 = Color32::from_rgb(23, 31, 40);
pub const MUTED: Color32 = Color32::from_rgb(184, 196, 208);
pub const ACCENT: Color32 = Color32::from_rgb(105, 211, 191);
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.window_highlight_topmost = false;
    style.visuals.widgets.noninteractive.bg_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = Color32::from_rgb(29, 40, 51);
    style.visuals.override_text_color = Some(Color32::from_rgb(222, 231, 239));
    style.visuals.selection.bg_fill = Color32::from_rgb(36, 81, 80);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(42, 55, 68));
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 46, 59);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(47, 65, 78);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(36, 81, 80);
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.slider_trailing_fill = true;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(55, 75, 88);
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.spacing.item_spacing = egui::vec2(8.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.slider_width = 110.0;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(18.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    ctx.set_style(style);
}
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(12.0).color(MUTED).strong());
    ui.add_space(2.0);
}
pub fn frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(16)
}

pub fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [58.0, 26.0],
            egui::Label::new(egui::RichText::new(label).color(MUTED)),
        );
        ui.spacing_mut().interact_size.y = 20.0;
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).clamp(48.0, 110.0);
        let slider = ui.add(egui::Slider::new(value, range.clone()).show_value(false));
        let number = ui.add_sized(
            [48.0, 26.0],
            egui::DragValue::new(value)
                .speed(if range.end() - range.start() > 10.0 {
                    1.0
                } else {
                    0.01
                })
                .range(range),
        );
        slider | number
    })
    .inner
}

pub fn mark(ui: &mut egui::Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = r.center();
    let a = size * 0.40;
    let points = [
        c + egui::vec2(0.0, -a),
        c + egui::vec2(a, -a * 0.5),
        c + egui::vec2(a, a * 0.5),
        c + egui::vec2(0.0, a),
        c + egui::vec2(-a, a * 0.5),
        c + egui::vec2(-a, -a * 0.5),
    ];
    let stroke = Stroke::new(1.5_f32, ACCENT);
    for i in 0..6 {
        ui.painter()
            .line_segment([points[i], points[(i + 1) % 6]], stroke);
    }
    for i in [1, 3, 5] {
        ui.painter().line_segment([c, points[i]], stroke);
    }
}

pub fn window_ranges(
    state: &crate::state::UiState,
    index: usize,
) -> (std::ops::RangeInclusive<f32>, std::ops::RangeInclusive<f32>) {
    if let Some(layer) = state.library.volumes.get(index) {
        let center = layer.volume.window_center as f32;
        let width = (layer.volume.window_width as f32).max(1.0);
        (
            (center - width * 0.5)..=(center + width * 0.5),
            1.0..=(width * 2.0),
        )
    } else {
        (0.0..=65535.0, 1.0..=65536.0)
    }
}

/// Drag across the study's useful range; numeric entry retains the full range.
pub fn window_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [58.0, 26.0],
            egui::Label::new(egui::RichText::new(label).color(MUTED)),
        );
        ui.spacing_mut().interact_size.y = 20.0;
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).clamp(48.0, 110.0);
        let slider = ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .clamping(egui::SliderClamping::Never),
        );
        let limit = if label == "Width" {
            1.0..=65536.0
        } else {
            -65535.0..=65535.0
        };
        let number = ui.add_sized(
            [48.0, 26.0],
            egui::DragValue::new(value).speed(1.0).range(limit),
        );
        slider | number
    })
    .inner
}

/// A quiet shared surface for mutually exclusive display choices.
pub fn segments(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(BG)
        .corner_radius(4)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 5.0);
            ui.horizontal(contents);
        });
}
