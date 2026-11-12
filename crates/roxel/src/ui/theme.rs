//! Shared visual language for the inspection workspace.
use super::layout::Layout;
use egui::{Color32, FontId, RichText, Stroke, TextStyle};
pub const BG: Color32 = Color32::from_rgb(16, 22, 29);
pub const PANEL: Color32 = Color32::from_rgb(23, 31, 40);
pub const MUTED: Color32 = Color32::from_rgb(184, 196, 208);
pub const ACCENT: Color32 = Color32::from_rgb(105, 211, 191);
/// Error text; 7.9:1 on [`PANEL`].
pub const DANGER: Color32 = Color32::from_rgb(246, 151, 141);
/// Fill of the selected segment / tab.
pub const SELECTED: Color32 = Color32::from_rgb(36, 81, 80);
/// Outline for controls on [`PANEL`]; at least 3:1 (WCAG 1.4.11).
pub const OUTLINE: Color32 = Color32::from_rgb(88, 112, 132);
/// Smallest touch target edge for compact layouts.
pub const TOUCH_TARGET: f32 = 44.0;

pub fn apply(ctx: &egui::Context) {
    set_layout(ctx, Layout::Desktop);
}

/// (Re)build the style for a layout. Compact layouts get touch-sized targets,
/// larger type and roomier spacing; the palette is shared.
pub fn set_layout(ctx: &egui::Context, layout: Layout) {
    // The workspace is dark-only. Pin the theme so a light OS/browser preference
    // doesn't switch egui to its stock light style (set_style only sets the
    // style of the current theme).
    ctx.set_theme(egui::Theme::Dark);
    let compact = layout.is_compact();
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.window_highlight_topmost = false;
    style.visuals.widgets.noninteractive.bg_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = Color32::from_rgb(29, 40, 51);
    style.visuals.override_text_color = Some(Color32::from_rgb(222, 231, 239));
    style.visuals.selection.bg_fill = SELECTED;
    // Doubles as the keyboard focus ring: 2 px, 3:1 or better.
    style.visuals.selection.stroke = Stroke::new(2.0_f32, ACCENT);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(42, 55, 68));
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 46, 59);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(47, 65, 78);
    style.visuals.widgets.active.weak_bg_fill = SELECTED;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.slider_trailing_fill = true;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(70, 94, 112);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    style.visuals.widgets.hovered.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(140, 168, 186));
    style.spacing.item_spacing = if compact {
        egui::vec2(10.0, 12.0)
    } else {
        egui::vec2(8.0, 10.0)
    };
    style.spacing.button_padding = if compact {
        egui::vec2(14.0, 10.0)
    } else {
        egui::vec2(12.0, 7.0)
    };
    style.spacing.interact_size.y = if compact { TOUCH_TARGET } else { 30.0 };
    style.spacing.slider_width = 110.0;
    if compact {
        style.spacing.icon_width = 24.0;
        style.spacing.icon_width_inner = 14.0;
        style.spacing.icon_spacing = 10.0;
        style.spacing.slider_rail_height = 8.0;
        style.spacing.scroll.bar_width = 6.0;
    }
    let size =
        |regular: f32, phone: f32| FontId::proportional(if compact { phone } else { regular });
    style.text_styles.insert(TextStyle::Body, size(14.0, 16.0));
    style
        .text_styles
        .insert(TextStyle::Button, size(13.0, 15.0));
    style
        .text_styles
        .insert(TextStyle::Heading, size(18.0, 20.0));
    style.text_styles.insert(TextStyle::Small, size(12.0, 13.0));
    ctx.set_style(style);
}

/// True when controls are sized for fingers.
pub fn is_touch(ui: &egui::Ui) -> bool {
    ui.spacing().interact_size.y >= TOUCH_TARGET - 1.0
}

pub fn section(ui: &mut egui::Ui, text: &str) {
    let touch = is_touch(ui);
    ui.add_space(if touch { 4.0 } else { 8.0 });
    ui.label(
        RichText::new(text)
            .size(if touch { 14.0 } else { 12.5 })
            .color(MUTED)
            .strong(),
    );
    ui.add_space(2.0);
}

pub fn frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(16)
}

/// Panel frame for compact layouts: smaller gutters, same 16 pt reading edge.
pub fn compact_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(16, 10))
}

pub fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    slider_row(ui, label, value, range, None)
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
    let limit = if label == "Width" {
        1.0..=65536.0
    } else {
        -65535.0..=65535.0
    };
    slider_row(ui, label, value, range, Some(limit))
}

/// Label, a rail that fills the row, and an editable number. With a
/// `window_limit` the rail covers the useful range while the number accepts the
/// full range.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    window_limit: Option<std::ops::RangeInclusive<f32>>,
) -> egui::Response {
    let touch = is_touch(ui);
    ui.horizontal(|ui| {
        let row_h = if touch {
            ui.spacing().interact_size.y
        } else {
            26.0
        };
        ui.allocate_ui_with_layout(
            egui::vec2(if touch { 66.0 } else { 58.0 }, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| ui.label(RichText::new(label).color(MUTED)),
        );
        if !touch {
            ui.spacing_mut().interact_size.y = 20.0;
        }
        let number_w = if touch { 72.0 } else { 60.0 };
        let max_rail = if touch { 640.0 } else { 240.0 };
        ui.spacing_mut().slider_width =
            (ui.available_width() - number_w - ui.spacing().item_spacing.x).clamp(48.0, max_rail);
        let mut rail = egui::Slider::new(value, range.clone()).show_value(false);
        if window_limit.is_some() {
            rail = rail.clamping(egui::SliderClamping::Never);
        }
        let rail = ui.add(rail);
        let speed = if window_limit.is_some() || range.end() - range.start() > 10.0 {
            1.0
        } else {
            0.01
        };
        let limit = window_limit.unwrap_or(range);
        let number = ui.add_sized(
            [number_w, row_h.clamp(26.0, 32.0)],
            egui::DragValue::new(value).speed(speed).range(limit),
        );
        rail | number
    })
    .inner
}

/// A mutually exclusive choice among a few visible options. Returns the index
/// that was clicked this frame. Compact layouts stretch the options across the
/// row so every one is a full-size touch target.
pub fn segmented(ui: &mut egui::Ui, selected: Option<usize>, labels: &[&str]) -> Option<usize> {
    let touch = is_touch(ui);
    let mut clicked = None;
    egui::Frame::new()
        .fill(BG)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 55, 68)))
        .corner_radius(6)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.spacing_mut().button_padding = egui::vec2(if touch { 8.0 } else { 10.0 }, 5.0);
            let n = labels.len().max(1) as f32;
            let height = if touch { TOUCH_TARGET - 6.0 } else { 26.0 };
            let width = if touch {
                (ui.available_width() - 3.0 * (n - 1.0)) / n
            } else {
                0.0
            };
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let active = selected == Some(i);
                    let text = RichText::new(*label).color(if active {
                        ACCENT
                    } else {
                        Color32::from_rgb(222, 231, 239)
                    });
                    let button = egui::Button::new(text)
                        .min_size(egui::vec2(width, height))
                        .corner_radius(4)
                        .fill(if active {
                            SELECTED
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            Stroke::new(1.0_f32, ACCENT)
                        } else {
                            Stroke::NONE
                        });
                    if ui.add(button).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}

/// The one dominant action of a view.
pub fn primary_button(ui: &mut egui::Ui, text: &str, min_size: egui::Vec2) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(BG).strong())
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .min_size(min_size),
    )
}

/// A button that fills the row; the compact layouts' default for actions.
pub fn wide_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let height = if is_touch(ui) { TOUCH_TARGET } else { 34.0 };
    ui.add_sized([ui.available_width(), height], egui::Button::new(text))
}

/// A muted caption / value pair on one line: `Size   256 × 256 × 128`.
pub fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 24.0;
        ui.allocate_ui_with_layout(
            egui::vec2(76.0, 24.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| ui.label(RichText::new(key).color(MUTED)),
        );
        ui.add(egui::Label::new(value).wrap());
    });
}

/// A small translucent label over the 3D view. Returns its rectangle.
pub fn overlay_chip(
    painter: &egui::Painter,
    anchor: egui::Align2,
    pos: egui::Pos2,
    text: &str,
    color: Color32,
    size: f32,
) -> egui::Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
    let rect = anchor.anchor_size(pos, galley.size() + egui::vec2(16.0, 10.0));
    painter.rect_filled(rect, 6, BG.gamma_multiply(0.88));
    painter.galley(rect.min + egui::vec2(8.0, 5.0), galley, color);
    rect
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
