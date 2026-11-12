//! Controls of the 3D view: camera, slice plane, windowing, palette, lighting.
//!
//! Desktop puts the camera actions and slice-plane options in the header above
//! the view and the appearance groups in a shelf below it. Compact layouts
//! show the same controls as the pages of the 3D controls sheet.

use crate::renderer::Quality;
use crate::slice_view::Orientation;
use crate::state::UiState;
use crate::transfer::TransferPreset;

use super::band_slider::draw_band_range_slider;
use super::layout::VolumePage;
use super::sidebar;
use super::theme::{self, SliderSpec};
use super::{band_color, clamp_band_thresholds};

const PRESETS: [(TransferPreset, &str); 3] = [
    (TransferPreset::Default, "Materials"),
    (TransferPreset::Threat, "Threat"),
    (TransferPreset::Monochrome, "Grayscale"),
];

/// Height of the palette legend: fixed, so switching palettes moves nothing.
const LEGEND_HEIGHT: f32 = 34.0;
/// One press of an orbit arrow, in radians.
const ORBIT_STEP: f32 = 0.2618;
/// One press of a pan arrow, in drag pixels.
const PAN_STEP: f32 = 80.0;
/// One press of a zoom button.
const ZOOM_STEP: f32 = 1.25;

// ---------------------------------------------------------------------------
// Shared actions
// ---------------------------------------------------------------------------

/// "Focus volume" / "Exit focus": the 3D view takes the workspace, in place.
pub(crate) fn focus_volume_button(ui: &mut egui::Ui, state: &mut UiState) {
    let (icon, label) = if state.focus_volume {
        (theme::Icon::Reduce, "Exit focus")
    } else {
        (theme::Icon::Enlarge, "Focus volume")
    };
    if theme::icon_button(ui, icon, label)
        .on_hover_text(if state.focus_volume {
            "Back to the workspace (Esc)"
        } else {
            "Enlarge the 3D view to fill the workspace"
        })
        .clicked()
    {
        state.focus_volume = !state.focus_volume;
        state.fit_camera = true;
    }
}

/// One-shot camera moves, named for the axes the corner gizmo draws (not for
/// the slice planes, which are a selection).
fn look_along(state: &mut UiState, axis: usize) {
    match axis {
        0 => state.camera.set_sagittal(),
        1 => state.camera.set_coronal(),
        _ => state.camera.set_axial(),
    }
    state.fit_camera = true;
}

fn plane_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let selected = Orientation::ALL
        .iter()
        .position(|o| *o == state.slice_view.orientation);
    let labels = Orientation::ALL.map(|o| o.label());
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        super::slice_panel::set_orientation(state, Orientation::ALL[i]);
    }
}

// ---------------------------------------------------------------------------
// Desktop
// ---------------------------------------------------------------------------

/// Header above the 3D view. Row one: the view's name and its one-shot camera
/// actions. Row two: the options that change what the view draws.
pub(crate) fn draw_volume_header(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::top("volume_header")
        .frame(theme::frame().inner_margin(egui::Margin::symmetric(16, 10)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::title(ui, "3D view");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    focus_volume_button(ui, state);
                    if ui
                        .button("Fit view")
                        .on_hover_text("Center and fit the whole volume")
                        .clicked()
                    {
                        state.fit_camera = true;
                    }
                    ui.add_space(8.0);
                    // Right-to-left, so Z is added first.
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (axis, label) in [(2, "Z"), (1, "Y"), (0, "X")] {
                            if ui
                                .add(egui::Button::new(label).min_size(egui::vec2(30.0, 0.0)))
                                .on_hover_text(format!("Look along the {label} axis"))
                                .clicked()
                            {
                                look_along(state, axis);
                            }
                        }
                    });
                    ui.label(
                        egui::RichText::new("View along")
                            .size(13.0)
                            .color(theme::MUTED),
                    );
                });
            });
            // A fixed-height row, so every option centres on one line.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    slice_plane_options(ui, state);
                },
            );
        });
}

/// "Slice plane", then "Cutaway" with "Reverse cutaway" enclosed with it as
/// its dependent. In focus the plane and slice position join them, because the
/// 2D panel that normally holds those is hidden.
fn slice_plane_options(ui: &mut egui::Ui, state: &mut UiState) {
    let available = sidebar::slice_tools_available(state);
    ui.add_enabled_ui(available, |ui| {
        theme::checkbox(ui, &mut state.show_slice_locator, "Slice plane")
            .on_hover_text("Outline the 2D slice in the 3D view");
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0_f32, theme::HAIRLINE))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 2))
            .show(ui, |ui| {
                theme::checkbox(ui, &mut state.cutaway, "Cutaway")
                    .on_hover_text("Hide the volume on one side of the slice plane");
                ui.add_enabled_ui(state.cutaway, |ui| {
                    theme::checkbox(ui, &mut state.reverse_cutaway, "Reverse cutaway")
                        .on_hover_text("Hide the other side instead");
                });
            });
    });
    if !available {
        ui.label(
            egui::RichText::new(sidebar::SLICE_TOOLS_NOTE)
                .small()
                .color(theme::MUTED),
        );
    } else if state.focus_volume {
        plane_picker(ui, state);
        if state.slice_view.max_slices > 1 {
            let width = ui.available_width().min(340.0);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 24.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let max_slices = state.slice_view.max_slices;
                    if theme::slice_slider(ui, &mut state.slice_view.slice_index, max_slices) {
                        state.slice_dirty = true;
                    }
                },
            );
        }
    }
}

/// Shelf below the 3D view: windowing, palette and lighting, in as many equal
/// columns as fit so no part of it is left empty.
pub(crate) fn draw_shelf(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::bottom("controls")
        .frame(theme::frame().inner_margin(egui::Margin::symmetric(16, 12)))
        .show(ctx, |ui| {
            // On very wide windows keep rows a usable length.
            let width = ui.available_width().min(1440.0);
            let pad = (ui.available_width() - width) * 0.5;
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let three = width >= 720.0;
                        ui.spacing_mut().item_spacing.x = 24.0;
                        ui.columns(if three { 3 } else { 2 }, |cols| {
                            for col in cols.iter_mut() {
                                col.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                            }
                            windowing_group(&mut cols[0], state);
                            palette_group(&mut cols[1], state, !three);
                            if three {
                                theme::section(&mut cols[2], "Lighting");
                                lighting_rows(&mut cols[2], state);
                            }
                        });
                    },
                );
            });
        });
}

/// 3D window/level, opacity and the density cut-off.
fn windowing_group(ui: &mut egui::Ui, state: &mut UiState) {
    let ranges = theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
    theme::section_with_note(ui, "3D windowing", "stored values");
    theme::window_rows(
        ui,
        &mut state.settings.window_center,
        &mut state.settings.window_width,
        ranges,
    );
    opacity_row(ui, state);
    density_row(ui, state);
}

fn opacity_row(ui: &mut egui::Ui, state: &mut UiState) {
    theme::slider_f32(
        ui,
        &SliderSpec::fraction("Opacity", 0.0..=1.0),
        &mut state.settings.global_opacity,
    );
}

fn density_row(ui: &mut egui::Ui, state: &mut UiState) {
    theme::slider_f32(
        ui,
        &SliderSpec::fraction("Density", 0.0..=1.0),
        &mut state.settings.density_threshold,
    );
}

fn lighting_rows(ui: &mut egui::Ui, state: &mut UiState) {
    for (label, value) in [
        ("Depth cue", &mut state.settings.depth_cue),
        ("Ambient", &mut state.settings.ambient),
        ("Diffuse", &mut state.settings.diffuse),
        ("Specular", &mut state.settings.specular),
    ] {
        theme::slider_f32(ui, &SliderSpec::fraction(label, 0.0..=1.0), value);
    }
}

fn palette_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let labels = PRESETS.map(|(_, label)| label);
    let selected = PRESETS
        .iter()
        .position(|(preset, _)| *preset == state.transfer.preset);
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        state.transfer.preset = PRESETS[i].0;
        state.actions.preset_changed = true;
    }
}

fn quality_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let selected = match state.settings.quality {
        Quality::Fast => 0,
        Quality::Medium => 1,
        Quality::High => 2,
    };
    match theme::segmented(ui, Some(selected), &["Fast", "Balanced", "High"]) {
        Some(0) => state.settings.quality = Quality::Fast,
        Some(1) => state.settings.quality = Quality::Medium,
        Some(2) => state.settings.quality = Quality::High,
        _ => {}
    }
}

/// Palette choice with its legend, render quality, and the editors that open
/// on request. `lighting_menu` adds the lighting sliders as a menu when the
/// shelf has no column for them.
fn palette_group(ui: &mut egui::Ui, state: &mut UiState, lighting_menu: bool) {
    theme::section(ui, "Palette");
    palette_picker(ui, state);
    let width = ui.available_width();
    palette_legend(ui, state, width);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Quality")
                .size(13.0)
                .color(theme::MUTED),
        );
        quality_picker(ui, state);
    });
    let materials = state.transfer.preset == TransferPreset::Default;
    if lighting_menu || materials {
        ui.horizontal(|ui| {
            if lighting_menu {
                ui.menu_button("Lighting", |ui| {
                    ui.set_width(300.0);
                    theme::section(ui, "Lighting");
                    lighting_rows(ui, state);
                });
            }
            if materials {
                ui.menu_button("Material bands", |ui| {
                    ui.set_width(340.0);
                    material_bands_editor(ui, state);
                });
            }
        });
    }
}

/// What the palette's colours mean, in a fixed-height strip.
fn palette_legend(ui: &mut egui::Ui, state: &UiState, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, LEGEND_HEIGHT), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let font = egui::FontId::proportional(if theme::is_touch(ui) { 13.0 } else { 12.0 });
    match state.transfer.preset {
        TransferPreset::Default => {
            // Dot-and-name pairs on one line, or two balanced lines: laid out
            // by hand so the strip keeps its height and no name is orphaned.
            let gap = 12.0;
            let items: Vec<_> = state
                .transfer
                .bands
                .iter()
                .map(|band| {
                    let galley =
                        painter.layout_no_wrap(band.name.to_owned(), font.clone(), theme::TEXT);
                    (band_color(band), galley)
                })
                .collect();
            let widths: Vec<f32> = items.iter().map(|(_, g)| 14.0 + g.size().x).collect();
            let run = |w: &[f32]| w.iter().sum::<f32>() + gap * w.len().saturating_sub(1) as f32;
            let first_line = if run(&widths) <= width {
                widths.len()
            } else {
                // The split that leaves the two lines closest in length.
                (1..widths.len())
                    .min_by(|a, b| {
                        let longest = |k: usize| run(&widths[..k]).max(run(&widths[k..]));
                        longest(*a).total_cmp(&longest(*b))
                    })
                    .unwrap_or(widths.len())
            };
            let line = font.size + 4.0;
            let lines = if first_line == widths.len() { 1.0 } else { 2.0 };
            let top = rect.top() + (LEGEND_HEIGHT - line * lines) * 0.5;
            let mut pos = egui::pos2(rect.left(), top);
            for (i, (color, galley)) in items.into_iter().enumerate() {
                if i == first_line {
                    pos = egui::pos2(rect.left(), top + line);
                }
                painter.circle_filled(pos + egui::vec2(5.0, line * 0.5), 4.5, color);
                painter.galley(
                    pos + egui::vec2(14.0, (line - galley.size().y) * 0.5),
                    galley,
                    theme::TEXT,
                );
                pos.x += widths[i] + gap;
            }
        }
        TransferPreset::Monochrome => {
            let bar = egui::Rect::from_min_size(
                rect.left_top() + egui::vec2(0.0, 3.0),
                egui::vec2(width.min(360.0), 9.0),
            );
            let steps = (bar.width() / 2.0).max(2.0) as usize;
            let step_w = bar.width() / steps as f32;
            for i in 0..steps {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(bar.left() + i as f32 * step_w, bar.top()),
                        egui::vec2(step_w + 0.5, bar.height()),
                    ),
                    0,
                    egui::Color32::from_gray(
                        (0.9 * (i as f32 / (steps - 1) as f32).powf(0.65) * 255.0) as u8,
                    ),
                );
            }
            painter.rect_stroke(
                bar,
                0,
                egui::Stroke::new(1.0_f32, theme::HAIRLINE),
                egui::StrokeKind::Outside,
            );
            let low = state.settings.window_center - state.settings.window_width * 0.5;
            let high = state.settings.window_center + state.settings.window_width * 0.5;
            for (x, align, label) in [
                (bar.left(), egui::Align2::LEFT_BOTTOM, format!("{low:.0}")),
                (
                    bar.center().x,
                    egui::Align2::CENTER_BOTTOM,
                    "Stored value".to_owned(),
                ),
                (
                    bar.right(),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("{high:.0}"),
                ),
            ] {
                painter.text(
                    egui::pos2(x, rect.bottom()),
                    align,
                    label,
                    font.clone(),
                    theme::MUTED,
                );
            }
        }
        TransferPreset::Threat => {
            painter.circle_filled(
                rect.left_center() + egui::vec2(5.0, 0.0),
                4.5,
                egui::Color32::from_rgb(235, 80, 70),
            );
            painter.text(
                rect.left_center() + egui::vec2(14.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Dense material in red",
                font,
                theme::TEXT,
            );
        }
    }
}

/// Desktop band editor: thresholds strip plus one opacity row per band.
fn material_bands_editor(ui: &mut egui::Ui, state: &mut UiState) {
    clamp_band_thresholds(&mut state.transfer.bands);
    theme::section_with_note(ui, "Band thresholds", "stored values");
    if draw_band_range_slider(ui, &mut state.transfer.bands) {
        state.transfer.bands_changed = true;
    }
    theme::section(ui, "Band opacity");
    for band in &mut state.transfer.bands {
        if band_opacity_row(ui, band.name, &mut band.alpha) {
            state.transfer.bands_changed = true;
        }
    }
}

/// One band's opacity multiplier. Returns true when a change was committed.
fn band_opacity_row(ui: &mut egui::Ui, label: &str, alpha: &mut f32) -> bool {
    theme::slider_f32(ui, &SliderSpec::fraction(label, 0.0..=2.0).unit("×"), alpha).committed
}

// ---------------------------------------------------------------------------
// Compact layouts: the pages of the 3D controls sheet
// ---------------------------------------------------------------------------

/// The 3D controls sheet: a page selector and the selected page. Every page
/// fits the sheet, so nothing scrolls under a slider.
pub(crate) fn volume_sheet(ui: &mut egui::Ui, state: &mut UiState) {
    if state.bands_open && state.transfer.preset == TransferPreset::Default {
        bands_page(ui, state);
        return;
    }
    state.bands_open = false;
    let labels = VolumePage::ALL.map(|p| p.label());
    let selected = VolumePage::ALL.iter().position(|p| *p == state.volume_page);
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        state.volume_page = VolumePage::ALL[i];
    }
    match state.volume_page {
        VolumePage::Camera => camera_page(ui, state),
        VolumePage::Slice => slice_plane_page(ui, state),
        VolumePage::Windowing => {
            let ranges = theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
            theme::section_with_note(ui, "3D windowing", "stored values");
            theme::window_rows(
                ui,
                &mut state.settings.window_center,
                &mut state.settings.window_width,
                ranges,
            );
            opacity_row(ui, state);
        }
        VolumePage::Display => {
            palette_picker(ui, state);
            theme::touch_row(ui, |ui| {
                let materials = state.transfer.preset == TransferPreset::Default;
                let button = if materials { 132.0 } else { 0.0 };
                let width = ui.available_width() - button - if materials { 10.0 } else { 0.0 };
                palette_legend(ui, state, width);
                if materials
                    && ui
                        .add_sized(
                            [button, theme::TOUCH_TARGET],
                            egui::Button::new("Material bands"),
                        )
                        .clicked()
                {
                    state.bands_open = true;
                }
            });
            theme::touch_row(ui, |ui| {
                theme::row_label(ui, "Quality");
                quality_picker(ui, state);
            });
            density_row(ui, state);
        }
        VolumePage::Lighting => lighting_rows(ui, state),
    }
}

/// Camera page: the view actions, then a visible single-tap way to do
/// everything the gestures do (WCAG 2.5.1, 2.5.7).
fn camera_page(ui: &mut egui::Ui, state: &mut UiState) {
    if !state.focus_volume {
        // In focus these two live in the bottom action bar.
        match theme::button_row(ui, &["Fit view", "Focus volume"]) {
            Some(0) => state.fit_camera = true,
            Some(1) => {
                state.focus_volume = true;
                state.fit_camera = true;
            }
            _ => {}
        }
    }
    theme::touch_row(ui, |ui| {
        theme::row_label(ui, "View along");
        if let Some(axis) = theme::button_row(ui, &["X", "Y", "Z"]) {
            look_along(state, axis);
        }
    });
    theme::touch_row(ui, |ui| {
        let arrows = 4.0 * theme::TOUCH_TARGET + 4.0 * ui.spacing().item_spacing.x;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width() - arrows, theme::TOUCH_TARGET),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let selected = usize::from(state.drag_pans);
                if let Some(i) = theme::segmented(ui, Some(selected), &["Orbit", "Pan"]) {
                    state.drag_pans = i == 1;
                }
            },
        );
        for (direction, dx, dy) in [
            (Arrow::Left, -1.0, 0.0),
            (Arrow::Right, 1.0, 0.0),
            (Arrow::Up, 0.0, -1.0),
            (Arrow::Down, 0.0, 1.0),
        ] {
            if arrow_button(ui, direction, state.drag_pans).clicked() {
                if state.drag_pans {
                    state.camera.pan_pixels(dx * PAN_STEP, dy * PAN_STEP);
                } else {
                    state.camera.rotate(dx * ORBIT_STEP, -dy * ORBIT_STEP);
                }
            }
        }
    });
    theme::touch_row(ui, |ui| {
        theme::row_label(ui, "Zoom");
        let size = egui::vec2(theme::TOUCH_TARGET, theme::TOUCH_TARGET);
        if ui
            .add(theme::stepper("−").min_size(size))
            .on_hover_text("Zoom out")
            .clicked()
        {
            state.camera.zoom(1.0 / ZOOM_STEP);
        }
        if ui
            .add(theme::stepper("+").min_size(size))
            .on_hover_text("Zoom in")
            .clicked()
        {
            state.camera.zoom(ZOOM_STEP);
        }
        // The gesture hint lives here, once, instead of over the view.
        ui.add(
            egui::Label::new(
                egui::RichText::new(if state.drag_pans {
                    "Or drag to pan and pinch to zoom."
                } else {
                    "Or drag to orbit, pinch to zoom, two fingers to pan."
                })
                .size(13.0)
                .color(theme::MUTED),
            )
            .wrap(),
        );
    });
}

#[derive(Clone, Copy)]
enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// A full-size button with a drawn chevron.
fn arrow_button(ui: &mut egui::Ui, arrow: Arrow, pans: bool) -> egui::Response {
    let size = egui::vec2(theme::TOUCH_TARGET, theme::TOUCH_TARGET);
    let name = match arrow {
        Arrow::Left => "left",
        Arrow::Right => "right",
        Arrow::Up => "up",
        Arrow::Down => "down",
    };
    let verb = if pans { "Pan" } else { "Orbit" };
    let response = ui
        .add(egui::Button::new("").min_size(size))
        .on_hover_text(format!("{verb} {name}"));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{verb} {name}"))
    });
    let c = response.rect.center();
    let (tip, side) = match arrow {
        Arrow::Left => (egui::vec2(-5.0, 0.0), egui::vec2(0.0, 7.0)),
        Arrow::Right => (egui::vec2(5.0, 0.0), egui::vec2(0.0, 7.0)),
        Arrow::Up => (egui::vec2(0.0, -5.0), egui::vec2(7.0, 0.0)),
        Arrow::Down => (egui::vec2(0.0, 5.0), egui::vec2(7.0, 0.0)),
    };
    ui.painter().add(egui::Shape::line(
        vec![c - tip + side, c + tip, c - tip - side],
        egui::Stroke::new(2.0_f32, theme::TEXT),
    ));
    response
}

/// Slice page: the slice plane, the cutaway with its dependent option indented
/// beneath it, and the slice position that moves both.
fn slice_plane_page(ui: &mut egui::Ui, state: &mut UiState) {
    let available = sidebar::slice_tools_available(state);
    ui.add_enabled_ui(available, |ui| {
        theme::touch_row(ui, |ui| {
            theme::checkbox(ui, &mut state.show_slice_locator, "Slice plane");
        });
        theme::touch_row(ui, |ui| {
            theme::checkbox(ui, &mut state.cutaway, "Cutaway");
        });
        ui.add_enabled_ui(state.cutaway, |ui| {
            theme::touch_row(ui, |ui| {
                ui.add_space(34.0);
                theme::checkbox(ui, &mut state.reverse_cutaway, "Reverse cutaway");
            });
        });
    });
    if !available {
        theme::touch_row(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(sidebar::SLICE_TOOLS_NOTE)
                        .size(13.0)
                        .color(theme::MUTED),
                )
                .wrap(),
            );
        });
    } else if state.slice_view.max_slices > 1 {
        let max_slices = state.slice_view.max_slices;
        if theme::slice_slider(ui, &mut state.slice_view.slice_index, max_slices) {
            state.slice_dirty = true;
        }
    }
}

/// Compact band editor: the thresholds strip, then one band at a time so its
/// opacity row fits without scrolling.
fn bands_page(ui: &mut egui::Ui, state: &mut UiState) {
    clamp_band_thresholds(&mut state.transfer.bands);
    theme::touch_row(ui, |ui| {
        if ui
            .add(egui::Button::new("Done").min_size(egui::vec2(72.0, theme::TOUCH_TARGET)))
            .clicked()
        {
            state.bands_open = false;
        }
        theme::title(ui, "Material bands");
    });
    if draw_band_range_slider(ui, &mut state.transfer.bands) {
        state.transfer.bands_changed = true;
    }
    let names: Vec<&str> = state.transfer.bands.iter().map(|b| b.name).collect();
    let last = names.len().saturating_sub(1);
    state.selected_band = state.selected_band.min(last);
    if let Some(i) = theme::segmented(ui, Some(state.selected_band), &names) {
        state.selected_band = i;
    }
    if let Some(band) = state.transfer.bands.get_mut(state.selected_band) {
        if band_opacity_row(ui, "Opacity", &mut band.alpha) {
            state.transfer.bands_changed = true;
        }
    }
}
