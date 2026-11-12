//! 3D rendering controls (quality, windowing, palette, lighting).
//!
//! Desktop docks them in a bottom shelf; compact layouts show the same sections
//! in the "3D view" tab and, in focus mode, in an adjustments sheet.

use crate::renderer::Quality;
use crate::state::UiState;
use crate::transfer::TransferPreset;

use super::band_slider::draw_band_range_slider;
use super::sidebar;
use super::theme;
use super::{band_color, clamp_band_thresholds, slider_committed};

const PRESETS: [(TransferPreset, &str); 3] = [
    (TransferPreset::Default, "Materials"),
    (TransferPreset::Threat, "Threat"),
    (TransferPreset::Monochrome, "Grayscale"),
];

/// Bottom panel: 3D rendering controls (quality, transfer function, lighting).
pub(crate) fn draw_bottom_controls(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::bottom("controls")
        .default_height(262.0)
        .frame(theme::frame())
        .resizable(true)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(panel, |panel| {
                    panel.columns(2, |cols| {
                        appearance_section(&mut cols[0], state);
                        palette_section(&mut cols[1], state);
                        lighting_section(&mut cols[1], state);
                    });
                });
        });
}

/// The "3D view" tab of compact layouts: one scrolling column.
pub(crate) fn view_tab(ui: &mut egui::Ui, state: &mut UiState, touch_copy: bool) {
    sidebar::camera_section(ui, state);
    ui.separator();
    appearance_section(ui, state);
    ui.separator();
    palette_section(ui, state);
    lighting_section(ui, state);
    ui.separator();
    ui.collapsing("Navigation help", |ui| {
        sidebar::navigation_help(ui, touch_copy);
    });
}

/// Render quality plus 3D window/level and opacity.
pub(crate) fn appearance_section(ui: &mut egui::Ui, state: &mut UiState) {
    let (center_range, width_range) =
        theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
    theme::section(ui, "3D windowing & appearance");
    ui.label(
        egui::RichText::new("Render quality")
            .small()
            .color(theme::MUTED),
    );
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
    theme::window_slider(
        ui,
        "Center",
        &mut state.settings.window_center,
        center_range,
    );
    theme::window_slider(ui, "Width", &mut state.settings.window_width, width_range);
    theme::slider(ui, "Opacity", &mut state.settings.global_opacity, 0.0..=1.0);
}

/// Palette choice, its legend and the material-band editor.
pub(crate) fn palette_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Display palette");
    let labels = PRESETS.map(|(_, label)| label);
    let selected = PRESETS
        .iter()
        .position(|(preset, _)| *preset == state.transfer.preset);
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        state.transfer.preset = PRESETS[i].0;
        state.actions.preset_changed = true;
    }

    ui.add_space(6.0);
    match state.transfer.preset {
        TransferPreset::Default => {
            ui.horizontal_wrapped(|ui| {
                for band in &state.transfer.bands {
                    ui.colored_label(band_color(band), "●");
                    ui.label(egui::RichText::new(band.name).small())
                        .on_hover_text(format!("Density threshold: {}", band.threshold));
                }
            });
        }
        TransferPreset::Monochrome => {
            let width = ui
                .available_width()
                .min(if theme::is_touch(ui) { 320.0 } else { 220.0 });
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
            let steps = (width / 2.0).max(2.0) as usize;
            let step_w = width / steps as f32;
            for i in 0..steps {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + i as f32 * step_w, rect.top()),
                        egui::vec2(step_w + 0.5, 8.0),
                    ),
                    0,
                    egui::Color32::from_gray(
                        (0.9 * (i as f32 / (steps - 1) as f32).powf(0.65) * 255.0) as u8,
                    ),
                );
            }
            let low = state.settings.window_center - state.settings.window_width * 0.5;
            let high = state.settings.window_center + state.settings.window_width * 0.5;
            for (point, align, label) in [
                (
                    rect.left_bottom(),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{low:.0}"),
                ),
                (
                    rect.center_bottom(),
                    egui::Align2::CENTER_BOTTOM,
                    "Intensity".into(),
                ),
                (
                    rect.right_bottom(),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("{high:.0}"),
                ),
            ] {
                ui.painter().text(
                    point,
                    align,
                    label,
                    egui::FontId::proportional(12.0),
                    theme::MUTED,
                );
            }
        }
        TransferPreset::Threat => {
            ui.label("Dense material is highlighted in red.");
        }
    }
    if state.transfer.preset == TransferPreset::Default {
        clamp_band_thresholds(&mut state.transfer.bands);
        ui.collapsing("Edit material bands", |ui| {
            ui.label(egui::RichText::new("Band thresholds").color(theme::MUTED));
            if draw_band_range_slider(ui, &mut state.transfer.bands) {
                state.transfer.bands_changed = true;
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Band opacity").color(theme::MUTED));
            for band in &mut state.transfer.bands {
                ui.horizontal(|h| {
                    h.colored_label(band_color(band), "■");
                    h.allocate_ui_with_layout(
                        egui::vec2(72.0, h.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| ui.label(band.name),
                    );
                    h.spacing_mut().slider_width =
                        (h.available_width() - 56.0 - h.spacing().item_spacing.x)
                            .clamp(60.0, 240.0);
                    let ar = h.add(egui::Slider::new(&mut band.alpha, 0.0..=2.0).show_value(false));
                    h.label(format!("{:.0}%", band.alpha * 100.0));
                    if slider_committed(&ar) {
                        state.transfer.bands_changed = true;
                    }
                });
            }
        });
    } else {
        // Keep spacing roughly consistent with the materials layout.
        ui.add_space(12.0);
    }
}

/// Lighting and density sliders in a collapsible group.
pub(crate) fn lighting_section(ui: &mut egui::Ui, state: &mut UiState) {
    ui.collapsing("Lighting & density", |ui| {
        theme::slider(ui, "Depth cue", &mut state.settings.depth_cue, 0.0..=1.0);
        theme::slider(
            ui,
            "Density",
            &mut state.settings.density_threshold,
            0.0..=1.0,
        );
        theme::slider(ui, "Ambient", &mut state.settings.ambient, 0.0..=1.0);
        theme::slider(ui, "Diffuse", &mut state.settings.diffuse, 0.0..=1.0);
        theme::slider(ui, "Specular", &mut state.settings.specular, 0.0..=1.0);
    });
}

/// Keep image adjustments available during focused volume inspection.
pub(crate) fn draw_focus_controls(ctx: &egui::Context, state: &mut UiState) {
    let (center_range, width_range) =
        theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
    egui::TopBottomPanel::bottom("focus_appearance")
        .frame(theme::frame())
        .show(ctx, |outer| {
            let dock_width = outer.available_width().min(1040.0);
            outer.horizontal(|outer| {
                outer.add_space(((outer.available_width() - dock_width) * 0.5).max(0.0));
                outer.allocate_ui_with_layout(
                    egui::vec2(dock_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        // Three equal columns keep the sliders aligned.
                        ui.columns(3, |cols| {
                            theme::window_slider(
                                &mut cols[0],
                                "Center",
                                &mut state.settings.window_center,
                                center_range,
                            );
                            theme::window_slider(
                                &mut cols[1],
                                "Width",
                                &mut state.settings.window_width,
                                width_range,
                            );
                            theme::slider(
                                &mut cols[2],
                                "Opacity",
                                &mut state.settings.global_opacity,
                                0.0..=1.0,
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            let labels = PRESETS.map(|(_, label)| label);
                            let selected = PRESETS
                                .iter()
                                .position(|(preset, _)| *preset == state.transfer.preset);
                            if let Some(i) = theme::segmented(ui, selected, &labels) {
                                state.transfer.preset = PRESETS[i].0;
                                state.actions.preset_changed = true;
                            }
                            ui.menu_button("Lighting", |ui| {
                                theme::slider(
                                    ui,
                                    "Depth cue",
                                    &mut state.settings.depth_cue,
                                    0.0..=1.0,
                                );
                                theme::slider(
                                    ui,
                                    "Ambient",
                                    &mut state.settings.ambient,
                                    0.0..=1.0,
                                );
                                theme::slider(
                                    ui,
                                    "Diffuse",
                                    &mut state.settings.diffuse,
                                    0.0..=1.0,
                                );
                                theme::slider(
                                    ui,
                                    "Specular",
                                    &mut state.settings.specular,
                                    0.0..=1.0,
                                );
                            });
                            if state.cutaway && !state.slice_view.composite {
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);
                                ui.checkbox(&mut state.reverse_cutaway, "Reverse cutaway");
                                let max_slices = state.slice_view.max_slices;
                                if super::slice_panel::slice_row(
                                    ui,
                                    &mut state.slice_view.slice_index,
                                    max_slices,
                                ) {
                                    state.slice_dirty = true;
                                }
                            }
                        });
                    },
                );
            });
        });
}

/// Compact focus mode: the adjustments sheet (windowing, palette, lighting and
/// the cutaway slice scrubber).
pub(crate) fn focus_adjust_tab(ui: &mut egui::Ui, state: &mut UiState) {
    if state.cutaway && !state.slice_view.composite {
        theme::section(ui, "Cutaway slice");
        ui.checkbox(&mut state.reverse_cutaway, "Reverse cutaway");
        let max_slices = state.slice_view.max_slices;
        if super::slice_panel::slice_row(ui, &mut state.slice_view.slice_index, max_slices) {
            state.slice_dirty = true;
        }
        ui.separator();
    }
    appearance_section(ui, state);
    ui.separator();
    palette_section(ui, state);
    lighting_section(ui, state);
}
