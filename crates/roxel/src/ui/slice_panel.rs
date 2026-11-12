//! The 2D slice: its image well and its controls.
//!
//! Desktop docks it as the right panel. "Focus slice" gives it the whole
//! workspace in place, exactly as "Focus volume" does for the 3D view. Compact
//! layouts show the same controls as the pages of the 2D controls sheet.

use super::layout::SlicePage;
use super::sidebar;
use super::theme::{self, SliderSpec};
use crate::slice_view::Orientation;
use crate::state::UiState;

/// Strip under the image, inside the well, for the scale bar and caption. The
/// scan is never drawn under it.
const FOOTER_HEIGHT: f32 = 30.0;
/// How much larger a slice must get on its side before it is turned.
const TURN_GAIN: f32 = 1.25;
/// Height of the desktop controls under the well (three headed groups).
const DESKTOP_CONTROLS_HEIGHT: f32 = 326.0;

// ---------------------------------------------------------------------------
// Shared actions
// ---------------------------------------------------------------------------

pub(crate) fn set_orientation(state: &mut UiState, orientation: Orientation) {
    if state.slice_view.orientation == orientation {
        return;
    }
    state.slice_view.orientation = orientation;
    if let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) {
        state.slice_view.update_for_volume(&layer.volume);
    }
    state.slice_dirty = true;
}

/// "Focus slice" / "Exit focus": the 2D slice takes the workspace, in place.
pub(crate) fn focus_slice_button(ui: &mut egui::Ui, state: &mut UiState) {
    let (icon, label) = if state.slice_expanded {
        (theme::Icon::Reduce, "Exit focus")
    } else {
        (theme::Icon::Enlarge, "Focus slice")
    };
    if theme::icon_button(ui, icon, label)
        .on_hover_text(if state.slice_expanded {
            "Back to the workspace (Esc)"
        } else {
            "Enlarge the 2D slice to fill the workspace"
        })
        .clicked()
    {
        state.slice_expanded = !state.slice_expanded;
        state.fit_camera = true;
    }
}

fn plane_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let selected = Orientation::ALL
        .iter()
        .position(|o| *o == state.slice_view.orientation);
    let labels = Orientation::ALL.map(|o| o.label());
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        set_orientation(state, Orientation::ALL[i]);
    }
}

/// Slice position row; inert while a composite projection is shown.
fn slice_row(ui: &mut egui::Ui, state: &mut UiState) {
    let scrubbable = !state.slice_view.composite && state.slice_view.max_slices > 1;
    ui.add_enabled_ui(scrubbable, |ui| {
        let max_slices = state.slice_view.max_slices;
        if theme::slice_slider(ui, &mut state.slice_view.slice_index, max_slices) {
            state.slice_dirty = true;
        }
    });
}

fn composite_toggle(ui: &mut egui::Ui, state: &mut UiState) {
    if theme::checkbox(ui, &mut state.slice_view.composite, "Composite projection")
        .on_hover_text("Blend every slice along this plane into one image")
        .changed()
    {
        state.slice_dirty = true;
    }
}

fn zoom_row(ui: &mut egui::Ui, state: &mut UiState) {
    theme::slider_f32(
        ui,
        &SliderSpec::whole("Zoom", 50.0..=500.0, 25.0).unit("%"),
        &mut state.slice_view.zoom,
    );
}

fn windowing_group(ui: &mut egui::Ui, state: &mut UiState) {
    let ranges = theme::window_ranges(state, state.slice_view.volume_index);
    theme::section_with_note(ui, "2D windowing", "stored values");
    // Re-rendering the slice is CPU work: wait for the change to be committed.
    if theme::window_rows(
        ui,
        &mut state.slice_view.window_center,
        &mut state.slice_view.window_width,
        ranges,
    ) {
        state.slice_dirty = true;
    }
}

/// Which volume the 2D slice shows, when there is more than one to pick.
fn volume_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let mut picked = None;
    egui::ComboBox::from_id_salt("slice_volume")
        .width((ui.available_width() - 8.0).clamp(120.0, 280.0))
        .truncate()
        .selected_text(
            state
                .library
                .volumes
                .get(state.slice_view.volume_index)
                .map(|l| l.name.as_str())
                .unwrap_or("None"),
        )
        .show_ui(ui, |combo| {
            for (i, layer) in state.library.volumes.iter().enumerate() {
                if combo
                    .selectable_label(state.slice_view.volume_index == i, &layer.name)
                    .clicked()
                {
                    picked = Some(i);
                }
            }
        });
    if let Some(i) = picked {
        sidebar::show_volume_in_slice(state, i);
    }
}

// ---------------------------------------------------------------------------
// The image well
// ---------------------------------------------------------------------------

/// The slice in a black well of the given size. The image is fitted at 100 %
/// and pans inside the well when zoomed; a footer strip below it carries the
/// scale bar and the caption so neither is ever drawn over the scan.
///
/// With `turn_to_fit`, a slice that would be clearly larger on its side is
/// drawn turned a quarter turn, as it would look with the phone held sideways:
/// the way a wide slice gets bigger on a tall screen without cropping it.
pub(crate) fn image_well(
    ui: &mut egui::Ui,
    state: &mut UiState,
    size: egui::Vec2,
    turn_to_fit: bool,
) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, 6, theme::WELL);
    let area = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.bottom() - FOOTER_HEIGHT),
    )
    .shrink(1.0);
    let footer = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - FOOTER_HEIGHT),
        rect.max,
    );
    let layer = state.library.volumes.get(state.slice_view.volume_index);
    let Some(texture) = &state.slice_texture else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if layer.is_some() {
                "Preparing slice…"
            } else {
                "No study open"
            },
            egui::FontId::proportional(14.0),
            theme::MUTED,
        );
        return;
    };
    let physical = layer
        .map(|layer| state.slice_view.physical_slice_size(&layer.volume))
        .unwrap_or(texture.size_vec2());
    let fit_within =
        |image: egui::Vec2| (area.width() / image.x.max(1.0)).min(area.height() / image.y.max(1.0));
    let upright = fit_within(physical);
    let on_side = fit_within(egui::vec2(physical.y, physical.x));
    let turned = turn_to_fit && on_side >= upright * TURN_GAIN;
    let fit = if turned { on_side } else { upright };

    // Pinch (or Ctrl + scroll) zooms the image under the pointer.
    if ui.rect_contains_pointer(area) {
        let pinch = ui.input(|i| i.zoom_delta());
        if pinch != 1.0 {
            state.slice_view.zoom = (state.slice_view.zoom * pinch).clamp(50.0, 500.0);
        }
    }
    let scale = fit * state.slice_view.zoom / 100.0;
    let shown = if turned {
        egui::vec2(physical.y, physical.x) * scale
    } else {
        physical * scale
    };

    let mut well = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    well.set_clip_rect(area.intersect(ui.clip_rect()));
    egui::ScrollArea::both()
        .id_salt("slice_pan")
        .auto_shrink([false, false])
        .show(&mut well, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_min_size(area.size());
            ui.add_space(((area.height() - shown.y) * 0.5).max(0.0));
            ui.horizontal(|ui| {
                ui.add_space(((area.width() - shown.x) * 0.5).max(0.0));
                if turned {
                    quarter_turned_image(ui, texture.id(), shown);
                } else {
                    ui.image(egui::load::SizedTexture::new(texture.id(), shown));
                }
            });
        });

    // Footer: scale bar on the left, caption on the right.
    let painter = ui.painter().with_clip_rect(footer);
    painter.hline(
        footer.x_range(),
        footer.top(),
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(48)),
    );
    let font = egui::FontId::proportional(12.0);
    let mut left = footer.left() + 12.0;
    if layer.is_some() && scale.is_finite() && scale > 0.0 {
        let (mm, width) = scale_bar_length(scale, 72.0);
        let y = footer.center().y;
        let stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);
        painter.line_segment([egui::pos2(left, y), egui::pos2(left + width, y)], stroke);
        for x in [left, left + width] {
            painter.line_segment([egui::pos2(x, y - 4.0), egui::pos2(x, y + 4.0)], stroke);
        }
        let label = painter.text(
            egui::pos2(left + width + 8.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{mm} mm"),
            font.clone(),
            egui::Color32::WHITE,
        );
        left = label.right() + 16.0;
    }
    let plane = state.slice_view.orientation.label();
    let position = if state.slice_view.composite {
        format!("{plane} projection")
    } else {
        format!(
            "{plane} {} / {}",
            state.slice_view.slice_index + 1,
            state.slice_view.max_slices
        )
    };
    let extent = if turned {
        "turned 90°".to_owned()
    } else {
        format!("{:.0} × {:.0} mm", physical.x, physical.y)
    };
    // The longest caption that fits beside the scale bar.
    let room = footer.right() - 12.0 - left;
    for caption in [format!("{position}  ·  {extent}"), position] {
        let galley = painter.layout_no_wrap(caption, font.clone(), theme::MUTED);
        if galley.size().x <= room {
            painter.galley(
                egui::pos2(
                    footer.right() - 12.0 - galley.size().x,
                    footer.center().y - galley.size().y * 0.5,
                ),
                galley,
                theme::MUTED,
            );
            break;
        }
    }
}

/// Paint the texture turned a quarter turn clockwise in a rect of `size`.
fn quarter_turned_image(ui: &mut egui::Ui, texture: egui::TextureId, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut mesh = egui::Mesh::with_texture(texture);
    // Screen corner <- image corner: the image's left edge runs along the top.
    for (pos, uv) in [
        (rect.left_top(), egui::pos2(0.0, 1.0)),
        (rect.right_top(), egui::pos2(0.0, 0.0)),
        (rect.right_bottom(), egui::pos2(1.0, 0.0)),
        (rect.left_bottom(), egui::pos2(1.0, 1.0)),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: egui::Color32::WHITE,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A round length (1, 2 or 5 × 10ⁿ mm) whose bar is at most `max_px` wide.
fn scale_bar_length(pixels_per_mm: f32, max_px: f32) -> (f32, f32) {
    let target = max_px / pixels_per_mm;
    let base = 10.0_f32.powf(target.log10().floor());
    let mm = [5.0, 2.0, 1.0]
        .into_iter()
        .map(|n| n * base)
        .find(|mm| *mm <= target)
        .unwrap_or(base);
    (mm, mm * pixels_per_mm)
}

/// Tallest a fitted slice can be at this width, over the three planes: the
/// well is sized for it so switching planes never moves the controls.
fn tallest_fit(state: &UiState, width: f32) -> f32 {
    let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) else {
        return width * 0.6;
    };
    let mut probe = crate::slice_view::SliceView {
        orientation: Orientation::Axial,
        ..Default::default()
    };
    Orientation::ALL
        .into_iter()
        .map(|orientation| {
            probe.orientation = orientation;
            let size = probe.physical_slice_size(&layer.volume);
            width / size.x.max(1.0) * size.y
        })
        .fold(0.0, f32::max)
}

// ---------------------------------------------------------------------------
// Desktop
// ---------------------------------------------------------------------------

/// Title and the two image actions, mirroring the 3D view's header.
fn slice_header(ui: &mut egui::Ui, state: &mut UiState) {
    ui.horizontal(|ui| {
        theme::title(ui, "2D slice");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            focus_slice_button(ui, state);
            if ui
                .button("Fit image")
                .on_hover_text("Show the whole slice")
                .clicked()
            {
                state.slice_view.zoom = 100.0;
            }
        });
    });
}

/// "Previous" / "Next" under the slice track, for exact single-slice steps.
fn pager(ui: &mut egui::Ui, state: &mut UiState) {
    let scrubbable = !state.slice_view.composite && state.slice_view.max_slices > 1;
    ui.add_enabled_ui(scrubbable, |ui| {
        ui.horizontal(|ui| {
            // Start where the slice rail starts (the track is inset by its thumb).
            ui.add_space(theme::LABEL_W + 7.0);
            let index = state.slice_view.slice_index;
            if ui
                .add_enabled(index > 0, egui::Button::new("Previous"))
                .on_hover_text("Previous slice")
                .clicked()
            {
                state.slice_view.slice_index -= 1;
                state.slice_dirty = true;
            }
            if ui
                .add_enabled(
                    index + 1 < state.slice_view.max_slices,
                    egui::Button::new("Next"),
                )
                .on_hover_text("Next slice")
                .clicked()
            {
                state.slice_view.slice_index += 1;
                state.slice_dirty = true;
            }
        });
    });
}

fn slice_group(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Slice");
    plane_picker(ui, state);
    slice_row(ui, state);
    pager(ui, state);
}

fn image_group(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Image");
    zoom_row(ui, state);
    if state.library.volumes.len() > 1 {
        ui.horizontal(|ui| {
            theme::row_label(ui, "Volume");
            volume_picker(ui, state);
        });
    }
    composite_toggle(ui, state);
}

/// Right panel: the slice in its well, then its three control groups.
pub(crate) fn draw_right_panel(ctx: &egui::Context, state: &mut UiState, width: f32) {
    egui::SidePanel::right("slice_panel")
        .exact_width(width)
        .resizable(false)
        .frame(theme::frame())
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            slice_header(ui, state);
            ui.add_space(2.0);
            let width = ui.available_width();
            let extra = if state.library.volumes.len() > 1 {
                30.0
            } else {
                0.0
            };
            let room = ui.available_height() - DESKTOP_CONTROLS_HEIGHT - extra;
            let tallest = tallest_fit(state, width) + FOOTER_HEIGHT + 2.0;
            let height = room.min(tallest).max(140.0);
            image_well(ui, state, egui::vec2(width, height), false);
            ui.add_space(4.0);
            slice_group(ui, state);
            ui.add_space(4.0);
            windowing_group(ui, state);
            ui.add_space(4.0);
            image_group(ui, state);
        });
}

/// Focus slice on desktop: the well takes the workspace, the same three
/// groups dock below it side by side.
pub(crate) fn draw_focus(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::top("slice_header")
        .frame(theme::frame().inner_margin(egui::Margin::symmetric(16, 10)))
        .show(ctx, |ui| slice_header(ui, state));
    egui::TopBottomPanel::bottom("slice_dock")
        .frame(theme::frame().inner_margin(egui::Margin::symmetric(16, 12)))
        .show(ctx, |ui| {
            let width = ui.available_width().min(1440.0);
            let pad = (ui.available_width() - width) * 0.5;
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 24.0;
                        ui.columns(3, |cols| {
                            for col in cols.iter_mut() {
                                col.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                            }
                            slice_group(&mut cols[0], state);
                            windowing_group(&mut cols[1], state);
                            image_group(&mut cols[2], state);
                        });
                    },
                );
            });
        });
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 0,
                    bottom: 0,
                }),
        )
        .show(ctx, |ui| {
            let size = ui.available_size();
            image_well(ui, state, size, false);
        });
}

// ---------------------------------------------------------------------------
// Compact layouts: the pages of the 2D controls sheet
// ---------------------------------------------------------------------------

/// The 2D controls sheet: a page selector and the selected page.
pub(crate) fn slice_sheet(ui: &mut egui::Ui, state: &mut UiState) {
    let mut pages = vec![SlicePage::Slice, SlicePage::Image];
    if state.library.volumes.len() > 1 {
        pages.push(SlicePage::Volume);
    }
    if !pages.contains(&state.slice_page) {
        state.slice_page = SlicePage::Slice;
    }
    let labels: Vec<&str> = pages.iter().map(|p| p.label()).collect();
    let selected = pages.iter().position(|p| *p == state.slice_page);
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        state.slice_page = pages[i];
    }
    match state.slice_page {
        SlicePage::Slice => {
            plane_picker(ui, state);
            slice_row(ui, state);
            theme::touch_row(ui, |ui| composite_toggle(ui, state));
            if !state.slice_expanded {
                // In focus these two live in the bottom action bar.
                match theme::button_row(ui, &["Fit image", "Focus slice"]) {
                    Some(0) => state.slice_view.zoom = 100.0,
                    Some(1) => state.slice_expanded = true,
                    _ => {}
                }
            }
        }
        SlicePage::Image => {
            windowing_group(ui, state);
            zoom_row(ui, state);
        }
        SlicePage::Volume => {
            theme::touch_row(ui, |ui| {
                theme::row_label(ui, "Volume");
                volume_picker(ui, state);
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Changes the 2D slice only. Pick the 3D volume under Study.",
                    )
                    .size(13.0)
                    .color(theme::MUTED),
                )
                .wrap(),
            );
        }
    }
}

/// The slice position row on its own, for the compact focus view.
pub(crate) fn slice_row_only(ui: &mut egui::Ui, state: &mut UiState) {
    slice_row(ui, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_bar_is_a_round_length_that_fits() {
        // 0.45 px per mm, 72 px available: 100 mm (45 px) fits, 200 mm does not.
        assert_eq!(scale_bar_length(0.45, 72.0), (100.0, 45.0));
        // 2 px per mm: 20 mm is 40 px; 50 mm would be 100 px.
        assert_eq!(scale_bar_length(2.0, 72.0), (20.0, 40.0));
    }
}
