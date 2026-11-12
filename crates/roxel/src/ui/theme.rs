//! Shared visual language for the inspection workspace: the palette, the
//! per-layout style, and the handful of controls every panel is built from.
//!
//! Contrast is measured, not estimated (WCAG 2.2, against [`PANEL`]):
//! text 13.3:1, muted text 9.4:1, control outlines 5.3:1, slider rail 3.9:1,
//! selected-segment text on its fill 7.1:1.
use super::layout::Layout;
use egui::{Color32, FontId, RichText, Stroke, TextStyle};
use std::ops::RangeInclusive;

pub const BG: Color32 = Color32::from_rgb(16, 22, 29);
pub const PANEL: Color32 = Color32::from_rgb(23, 31, 40);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(222, 231, 239);
/// Headings: the brightest text on a panel.
pub const HEADING: Color32 = Color32::from_rgb(244, 248, 251);
/// Captions and row labels.
pub const MUTED: Color32 = Color32::from_rgb(184, 196, 208);
pub const ACCENT: Color32 = Color32::from_rgb(105, 211, 191);
/// Error text; 7.7:1 on [`PANEL`].
pub const DANGER: Color32 = Color32::from_rgb(246, 151, 141);
/// Fill of the selected segment, tab or list item.
pub const SELECTED: Color32 = Color32::from_rgb(36, 81, 80);
/// Outline of buttons, number boxes and unchecked boxes.
pub const OUTLINE: Color32 = Color32::from_rgb(124, 148, 168);
/// The unfilled part of a slider rail.
pub const RAIL: Color32 = Color32::from_rgb(104, 126, 146);
/// Slider thumb.
pub const THUMB: Color32 = Color32::from_rgb(236, 242, 247);
/// Hairlines between panels and groups.
pub const HAIRLINE: Color32 = Color32::from_rgb(48, 62, 76);
/// Background of the 2D image well: scans are black-backed, so the image and
/// its surround read as one viewport.
pub const WELL: Color32 = Color32::from_rgb(0, 0, 0);
/// Smallest touch target edge for compact layouts.
pub const TOUCH_TARGET: f32 = 44.0;
/// Label column of a desktop control row; tracks start on one line.
pub const LABEL_W: f32 = 64.0;
/// Label-and-value column of a touch control row.
pub const TOUCH_LABEL_W: f32 = 78.0;
/// Number box of a desktop control row.
const NUMBER_W: f32 = 56.0;

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
    let visuals = &mut style.visuals;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.window_highlight_topmost = false;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = Color32::from_rgb(29, 40, 51);
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTED;
    // Doubles as the keyboard focus ring: 2 px, 9:1 against the panel.
    visuals.selection.stroke = Stroke::new(2.0_f32, ACCENT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 52, 66);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 52, 66);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 70, 86);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 70, 86);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, THUMB);
    visuals.widgets.active.weak_bg_fill = SELECTED;
    visuals.widgets.active.bg_fill = SELECTED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.open.weak_bg_fill = SELECTED;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(5);
        widget.expansion = 0.0;
    }
    let spacing = &mut style.spacing;
    spacing.item_spacing = if compact {
        egui::vec2(10.0, 8.0)
    } else {
        egui::vec2(8.0, 8.0)
    };
    spacing.button_padding = if compact {
        egui::vec2(12.0, 8.0)
    } else {
        egui::vec2(10.0, 5.0)
    };
    spacing.interact_size.y = if compact { TOUCH_TARGET } else { 28.0 };
    spacing.menu_margin = egui::Margin::same(12);
    if compact {
        spacing.scroll.bar_width = 6.0;
    }
    let size =
        |regular: f32, phone: f32| FontId::proportional(if compact { phone } else { regular });
    style.text_styles.insert(TextStyle::Body, size(13.5, 16.0));
    style
        .text_styles
        .insert(TextStyle::Button, size(13.0, 15.0));
    style
        .text_styles
        .insert(TextStyle::Heading, size(17.0, 18.0));
    style.text_styles.insert(TextStyle::Small, size(12.0, 13.0));
    ctx.set_style(style);
}

/// True when controls are sized for fingers.
pub fn is_touch(ui: &egui::Ui) -> bool {
    ui.spacing().interact_size.y >= TOUCH_TARGET - 1.0
}

/// The name of a panel or page: its largest text.
pub fn title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let size = if is_touch(ui) { 18.0 } else { 17.0 };
    ui.label(RichText::new(text).size(size).color(HEADING))
}

/// A group heading: larger and brighter than the rows it introduces.
pub fn section(ui: &mut egui::Ui, text: &str) {
    section_with_note(ui, text, "");
}

/// A group heading with a short muted qualifier on the same line, such as the
/// scale its numbers are in.
pub fn section_with_note(ui: &mut egui::Ui, text: &str, note: &str) {
    let touch = is_touch(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 20.0;
        ui.label(
            RichText::new(text)
                .size(if touch { 16.0 } else { 14.5 })
                .color(HEADING),
        );
        if !note.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(note)
                        .size(if touch { 13.0 } else { 12.0 })
                        .color(MUTED),
                );
            });
        }
    });
}

/// A thin rule between groups in one panel.
pub fn rule(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0_f32, HAIRLINE),
    );
}

pub fn frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(16)
}

/// A box that is unmistakably empty or ticked: dark with a light outline when
/// off, filled with the accent and a dark tick when on.
pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    checkbox_named(ui, checked, label, label)
}

/// [`checkbox`] whose spoken name differs from its visible label, for a box
/// that sits in a row whose text already says what it is.
pub fn checkbox_named(
    ui: &mut egui::Ui,
    checked: &mut bool,
    label: &str,
    name: &str,
) -> egui::Response {
    let touch = is_touch(ui);
    let side = if touch { 22.0 } else { 16.0 };
    let gap = match (label.is_empty(), touch) {
        (true, _) => 0.0,
        (false, true) => 12.0,
        (false, false) => 8.0,
    };
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
    let height = if touch {
        TOUCH_TARGET
    } else {
        galley.size().y.max(24.0)
    };
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(side + gap + galley.size().x, height),
        egui::Sense::click(),
    );
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let on = *checked;
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let square = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - side * 0.5),
            egui::vec2(side, side),
        );
        if on {
            painter.rect_filled(square, 4, ACCENT);
            let s = side * 0.26;
            let c = square.center();
            painter.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-s, 0.0),
                    c + egui::vec2(-s * 0.25, s * 0.75),
                    c + egui::vec2(s, -s * 0.7),
                ],
                Stroke::new(2.0_f32, BG),
            ));
        } else {
            painter.rect(
                square,
                4,
                BG,
                Stroke::new(1.5_f32, if response.hovered() { THUMB } else { OUTLINE }),
                egui::StrokeKind::Inside,
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                4,
                Stroke::new(2.0_f32, ACCENT),
                egui::StrokeKind::Outside,
            );
        }
        painter.galley(
            egui::pos2(
                square.right() + gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            TEXT,
        );
    }
    response
}

/// What one control row edits.
pub struct SliderSpec<'a> {
    pub label: &'a str,
    /// Range the track covers.
    pub range: RangeInclusive<f64>,
    /// Range the number box and the steppers accept.
    pub limit: RangeInclusive<f64>,
    /// One press of a stepper or an arrow key.
    pub step: f64,
    pub decimals: usize,
    /// Added for display (1 for a one-based slice number).
    pub offset: f64,
    pub unit: &'a str,
    /// Shown as "value / total" where the layout has room.
    pub total: Option<usize>,
}

impl<'a> SliderSpec<'a> {
    /// A 0–1 style fraction shown with two decimals.
    pub fn fraction(label: &'a str, range: RangeInclusive<f64>) -> Self {
        Self {
            label,
            limit: range.clone(),
            range,
            step: 0.05,
            decimals: 2,
            offset: 0.0,
            unit: "",
            total: None,
        }
    }

    /// A whole-number value.
    pub fn whole(label: &'a str, range: RangeInclusive<f64>, step: f64) -> Self {
        Self {
            label,
            limit: range.clone(),
            range,
            step,
            decimals: 0,
            offset: 0.0,
            unit: "",
            total: None,
        }
    }

    pub fn limit(mut self, limit: RangeInclusive<f64>) -> Self {
        self.limit = limit;
        self
    }

    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    fn display(&self, value: f64) -> String {
        format!("{:.*}{}", self.decimals, value + self.offset, self.unit)
    }
}

/// What a control row did this frame.
#[derive(Clone, Copy, Default)]
pub struct SliderOutcome {
    /// The value changed.
    pub changed: bool,
    /// The change is final: a drag ended, a stepper was pressed or a typed
    /// number was entered. Expensive work waits for this.
    pub committed: bool,
}

/// The one control row of the app. Desktop: label column, track, number box.
/// Touch: label over value, minus, track, plus, with full-size targets. Every
/// slider goes through here, so rows never rearrange between panels.
pub fn slider(ui: &mut egui::Ui, spec: &SliderSpec<'_>, value: &mut f64) -> SliderOutcome {
    if is_touch(ui) {
        touch_slider(ui, spec, value)
    } else {
        desktop_slider(ui, spec, value)
    }
}

/// [`slider`] for the `f32` settings.
pub fn slider_f32(ui: &mut egui::Ui, spec: &SliderSpec<'_>, value: &mut f32) -> SliderOutcome {
    let mut wide = *value as f64;
    let outcome = slider(ui, spec, &mut wide);
    if outcome.changed {
        *value = wide as f32;
    }
    outcome
}

/// The slice position row: one-based, with the slice count.
pub fn slice_slider(ui: &mut egui::Ui, index: &mut usize, max_slices: usize) -> bool {
    let last = max_slices.saturating_sub(1) as f64;
    let spec = SliderSpec {
        offset: 1.0,
        total: Some(max_slices),
        ..SliderSpec::whole("Slice", 0.0..=last, 1.0)
    };
    let mut wide = *index as f64;
    let outcome = slider(ui, &spec, &mut wide);
    if outcome.changed {
        *index = wide.round().clamp(0.0, last) as usize;
    }
    outcome.changed
}

fn desktop_slider(ui: &mut egui::Ui, spec: &SliderSpec<'_>, value: &mut f64) -> SliderOutcome {
    let mut outcome = SliderOutcome::default();
    // A fixed-height row: every desktop control row is exactly this tall.
    let row = egui::vec2(ui.available_width(), 24.0);
    let centered = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(row, centered, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.spacing_mut().interact_size.y = 24.0;
        label_cell(ui, spec.label, egui::vec2(LABEL_W, 24.0));
        let width = (ui.available_width() - NUMBER_W - 8.0).max(40.0);
        let rail = track(ui, spec, value, egui::vec2(width, 24.0));
        let (decimals, offset, unit) = (spec.decimals, spec.offset, spec.unit);
        let speed = if decimals == 0 { 1.0 } else { 0.01 };
        let number = ui.add_sized(
            [NUMBER_W, 24.0],
            egui::DragValue::new(value)
                .speed(speed)
                .range(spec.limit.clone())
                .custom_formatter(move |v, _| format!("{:.*}{}", decimals, v + offset, unit))
                .custom_parser(move |text| {
                    let text = text.trim();
                    let number = text.strip_suffix(unit).unwrap_or(text).trim();
                    number.parse::<f64>().ok().map(|v| v - offset)
                }),
        );
        outcome.changed = rail.changed() || number.changed();
        outcome.committed = rail.drag_stopped()
            || (rail.changed() && !rail.dragged())
            || number.drag_stopped()
            || number.lost_focus()
            || (number.changed() && !number.dragged());
    });
    outcome
}

fn touch_slider(ui: &mut egui::Ui, spec: &SliderSpec<'_>, value: &mut f64) -> SliderOutcome {
    let mut outcome = SliderOutcome::default();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), TOUCH_TARGET),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (cell, _) = ui.allocate_exact_size(
                egui::vec2(TOUCH_LABEL_W, TOUCH_TARGET),
                egui::Sense::hover(),
            );
            let shown = match spec.total {
                Some(total) => format!("{} / {total}", spec.display(*value)),
                None => spec.display(*value),
            };
            ui.painter().text(
                cell.left_top() + egui::vec2(0.0, 3.0),
                egui::Align2::LEFT_TOP,
                spec.label,
                FontId::proportional(13.0),
                MUTED,
            );
            ui.painter().text(
                cell.left_bottom() + egui::vec2(0.0, -3.0),
                egui::Align2::LEFT_BOTTOM,
                shown,
                FontId::proportional(15.0),
                TEXT,
            );
            let step = egui::vec2(TOUCH_TARGET, TOUCH_TARGET);
            let (low, high) = (*spec.limit.start(), *spec.limit.end());
            let less = ui
                .add_enabled(*value > low, stepper("−").min_size(step))
                .on_hover_text(format!("Decrease {}", spec.label));
            if less.clicked() {
                *value = snap(*value - spec.step, spec).clamp(low, high);
                outcome.changed = true;
                outcome.committed = true;
            }
            let width = (ui.available_width() - step.x - 10.0).max(40.0);
            let rail = track(ui, spec, value, egui::vec2(width, TOUCH_TARGET));
            outcome.changed |= rail.changed();
            outcome.committed |= rail.drag_stopped() || (rail.changed() && !rail.dragged());
            let more = ui
                .add_enabled(*value < high, stepper("+").min_size(step))
                .on_hover_text(format!("Increase {}", spec.label));
            if more.clicked() {
                *value = snap(*value + spec.step, spec).clamp(low, high);
                outcome.changed = true;
                outcome.committed = true;
            }
        },
    );
    outcome
}

/// A minus or plus button with a glyph large enough to read at arm's length.
pub fn stepper(glyph: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(glyph).size(22.0))
}

/// Round to the row's display precision so steppers land on tidy values.
fn snap(value: f64, spec: &SliderSpec<'_>) -> f64 {
    let scale = 10f64.powi(spec.decimals as i32);
    (value * scale).round() / scale
}

/// The rail and thumb of a control row. The thumb is light on a mid-tone rail
/// with the travelled part in the accent, so position reads at a glance.
fn track(
    ui: &mut egui::Ui,
    spec: &SliderSpec<'_>,
    value: &mut f64,
    size: egui::Vec2,
) -> egui::Response {
    let touch = is_touch(ui);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let radius = if touch { 12.0 } else { 7.0 };
    let (x0, x1) = (rect.left() + radius, rect.right() - radius);
    let (low, high) = (*spec.range.start(), *spec.range.end());
    let span = (high - low).max(f64::EPSILON);
    let before = *value;
    if response.is_pointer_button_down_on() || response.dragged() || response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let t = ((pointer.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0) as f64;
            *value = snap(low + t * span, spec);
        }
    }
    if response.has_focus() {
        let (left, right) = ui.input(|i| {
            (
                i.num_presses(egui::Key::ArrowLeft),
                i.num_presses(egui::Key::ArrowRight),
            )
        });
        let steps = right as f64 - left as f64;
        if steps != 0.0 {
            *value = snap(*value + steps * spec.step, spec)
                .clamp(*spec.limit.start(), *spec.limit.end());
        }
    }
    if *value != before {
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let (now, label) = (*value, spec.label);
    response.widget_info(|| egui::WidgetInfo::slider(enabled, now + spec.offset, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let half = if touch { 3.0 } else { 2.0 };
        let y = rect.center().y;
        let t = (((*value - low) / span).clamp(0.0, 1.0)) as f32;
        let x = egui::lerp(x0..=x1, t);
        let rail = egui::Rect::from_min_max(egui::pos2(x0, y - half), egui::pos2(x1, y + half));
        painter.rect_filled(rail, half, RAIL);
        painter.rect_filled(
            egui::Rect::from_min_max(rail.min, egui::pos2(x, rail.max.y)),
            half,
            ACCENT,
        );
        let grow = if response.dragged() { 1.0 } else { 0.0 };
        painter.circle(
            egui::pos2(x, y),
            radius + grow,
            THUMB,
            Stroke::new(1.5_f32, BG),
        );
        if response.has_focus() {
            painter.circle_stroke(egui::pos2(x, y), radius + 3.0, Stroke::new(2.0_f32, ACCENT));
        }
    }
    response
}

/// A mutually exclusive choice among a few visible options: a persistent
/// selection, never a one-shot action. Returns the index clicked this frame.
/// Touch layouts stretch the options across the row, sharing the spare width
/// equally so a long label gets the room it needs.
pub fn segmented(ui: &mut egui::Ui, selected: Option<usize>, labels: &[&str]) -> Option<usize> {
    let touch = is_touch(ui);
    let mut clicked = None;
    egui::Frame::new()
        .fill(BG)
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
        .corner_radius(7)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(if touch { 4.0 } else { 9.0 }, 4.0);
            let n = labels.len().max(1) as f32;
            // Frame margin and stroke bring the control to a full touch row.
            let height = if touch { TOUCH_TARGET - 6.0 } else { 24.0 };
            let font = TextStyle::Button.resolve(ui.style());
            let text_widths: Vec<f32> = labels
                .iter()
                .map(|label| {
                    ui.painter()
                        .layout_no_wrap((*label).to_owned(), font.clone(), TEXT)
                        .size()
                        .x
                })
                .collect();
            let spare = if touch {
                ((ui.available_width() - 2.0 * (n - 1.0) - text_widths.iter().sum::<f32>()) / n)
                    .max(8.0)
            } else {
                0.0
            };
            ui.spacing_mut().interact_size.y = height;
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let active = selected == Some(i);
                    let text = RichText::new(*label).color(if active { HEADING } else { MUTED });
                    let width = if touch {
                        (text_widths[i] + spare).floor()
                    } else {
                        0.0
                    };
                    let button = egui::Button::new(text)
                        .min_size(egui::vec2(width, height))
                        .corner_radius(5)
                        .fill(if active {
                            SELECTED
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            Stroke::new(1.0_f32, ACCENT)
                        } else {
                            Stroke::NONE
                        })
                        .selected(false);
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
        egui::Button::new(RichText::new(text).color(BG))
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .min_size(min_size),
    )
}

/// A row of equally wide buttons filling the available width. Returns the index
/// pressed. One-shot actions use this, never a segmented control.
pub fn button_row(ui: &mut egui::Ui, labels: &[&str]) -> Option<usize> {
    let mut pressed = None;
    let gap = ui.spacing().item_spacing.x;
    let n = labels.len().max(1) as f32;
    let width = ((ui.available_width() - gap * (n - 1.0)) / n).floor();
    let height = ui.spacing().interact_size.y;
    ui.horizontal(|ui| {
        for (i, label) in labels.iter().enumerate() {
            if ui
                .add_sized([width, height], egui::Button::new(*label))
                .clicked()
            {
                pressed = Some(i);
            }
        }
    });
    pressed
}

/// Glyphs drawn rather than typed, so they stay crisp at any scale.
#[derive(Clone, Copy)]
pub enum Icon {
    /// Corners pushed outward: make this view larger.
    Enlarge,
    /// Corners pulled inward: back to the shared workspace.
    Reduce,
}

/// A button with a drawn glyph before its label.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    let id = egui::Id::new("roxel_button_icon");
    let side = if is_touch(ui) { 14.0 } else { 12.0 };
    // The cell is wider than the glyph: the extra is the gap before the label.
    let shown = egui::Button::new((egui::Atom::custom(id, egui::vec2(side + 4.0, side)), label))
        .atom_ui(ui);
    if let Some(cell) = shown.rect(id) {
        let rect = egui::Rect::from_min_size(cell.min, egui::vec2(side, side));
        let stroke = Stroke::new(1.5_f32, TEXT);
        let arm = side * 0.36;
        let painter = ui.painter();
        for (corner, dx, dy) in [
            (rect.left_top(), 1.0, 1.0),
            (rect.right_top(), -1.0, 1.0),
            (rect.left_bottom(), 1.0, -1.0),
            (rect.right_bottom(), -1.0, -1.0),
        ] {
            // Enlarge: the elbow sits on the corner. Reduce: it sits inside.
            let elbow = match icon {
                Icon::Enlarge => corner,
                Icon::Reduce => corner + egui::vec2(dx * arm, dy * arm),
            };
            let sign = match icon {
                Icon::Enlarge => 1.0,
                Icon::Reduce => -1.0,
            };
            painter.line_segment([elbow, elbow + egui::vec2(sign * dx * arm, 0.0)], stroke);
            painter.line_segment([elbow, elbow + egui::vec2(0.0, sign * dy * arm)], stroke);
        }
    }
    shown.response
}

/// One fixed-height row of a touch controls page, vertically centred.
pub fn touch_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), TOUCH_TARGET),
        egui::Layout::left_to_right(egui::Align::Center),
        add,
    )
    .inner
}

/// A muted row label in the same column the control rows use.
pub fn row_label(ui: &mut egui::Ui, text: &str) {
    let touch = is_touch(ui);
    let size = if touch {
        egui::vec2(TOUCH_LABEL_W, TOUCH_TARGET)
    } else {
        egui::vec2(LABEL_W, 24.0)
    };
    label_cell(ui, text, size);
}

/// A fixed-size cell with muted text at its left: the label column. The cell
/// keeps its width whatever the text, so everything after it lines up.
fn label_cell(ui: &mut egui::Ui, text: &str, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(if is_touch(ui) { 14.0 } else { 13.0 }),
        MUTED,
    );
}

/// A muted caption / value pair on one line: `Size   256 × 256 × 128`.
pub fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    let touch = is_touch(ui);
    let height = if touch { 28.0 } else { 22.0 };
    ui.scope(|ui| {
        // Set before the row is laid out: the row takes its height from it.
        ui.spacing_mut().interact_size.y = height;
        ui.horizontal(|ui| {
            label_cell(ui, key, egui::vec2(if touch { 84.0 } else { 68.0 }, height));
            ui.add(egui::Label::new(value).wrap());
        });
    });
}

/// Keep both ends of a long name: the end usually holds what tells two studies
/// apart. Returns the text unchanged when it fits.
pub fn elide_middle(painter: &egui::Painter, text: &str, font: FontId, max_width: f32) -> String {
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), TEXT)
            .size()
            .x
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let build = |kept: usize| {
        let head = kept * 2 / 5;
        let tail = kept - head;
        let mut out: String = chars[..head].iter().collect();
        out.push('…');
        out.extend(&chars[chars.len() - tail..]);
        out
    };
    let (mut low, mut high) = (0usize, chars.len().saturating_sub(1));
    let mut best = String::from("…");
    while low <= high {
        let mid = (low + high) / 2;
        let candidate = build(mid);
        if width(&candidate) <= max_width {
            best = candidate;
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    best
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

/// Useful window range for a volume: the track covers it, the number box and
/// steppers accept the full stored range.
pub fn window_ranges(
    state: &crate::state::UiState,
    index: usize,
) -> (RangeInclusive<f64>, RangeInclusive<f64>) {
    if let Some(layer) = state.library.volumes.get(index) {
        let center = layer.volume.window_center;
        let width = layer.volume.window_width.max(1.0);
        (
            (center - width * 0.5)..=(center + width * 0.5),
            1.0..=(width * 2.0),
        )
    } else {
        (0.0..=65535.0, 1.0..=65536.0)
    }
}

/// The two windowing rows, shared by the 3D and the 2D controls so they read
/// and behave the same. Returns true when a change was committed.
pub fn window_rows(
    ui: &mut egui::Ui,
    center: &mut f32,
    width: &mut f32,
    ranges: (RangeInclusive<f64>, RangeInclusive<f64>),
) -> bool {
    let step = window_step(&ranges.1);
    let c = slider_f32(
        ui,
        &SliderSpec::whole("Center", ranges.0, step).limit(-65535.0..=65535.0),
        center,
    );
    let w = slider_f32(
        ui,
        &SliderSpec::whole("Width", ranges.1, step).limit(1.0..=65536.0),
        width,
    );
    c.committed || w.committed
}

/// About 1/200 of the useful range, rounded up to a tidy number.
fn window_step(width_range: &RangeInclusive<f64>) -> f64 {
    let raw = (width_range.end() - width_range.start()) / 200.0;
    if raw <= 1.0 {
        return 1.0;
    }
    let magnitude = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= raw)
        .unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_steps_are_tidy() {
        assert_eq!(window_step(&(1.0..=5048.0)), 50.0);
        assert_eq!(window_step(&(1.0..=400.0)), 2.0);
        assert_eq!(window_step(&(1.0..=50.0)), 1.0);
    }
}
