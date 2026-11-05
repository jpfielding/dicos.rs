//! Bottom panel: 3D rendering controls (quality, transfer function, lighting).

use crate::renderer::Quality;
use crate::state::UiState;
use crate::transfer::TransferPreset;

use super::band_slider::draw_band_range_slider;
use super::{band_color, clamp_band_thresholds, slider_committed};

/// Bottom panel: 3D rendering controls (quality, transfer function, lighting).
pub(crate) fn draw_bottom_controls(ctx: &egui::Context, state: &mut UiState) {
    let (center_range, width_range) =
        super::theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
    egui::TopBottomPanel::bottom("controls")
        .default_height(234.0)
        .frame(super::theme::frame())
        .resizable(true)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(panel, |panel| {
                    panel.columns(2, |cols| {
                        // Column 1: Rendering quality and window/level.
                        super::theme::section(&mut cols[0], "3D WINDOWING & APPEARANCE");
                        cols[0].horizontal(|h| {
                            h.label("Quality:");
                            if h.selectable_label(state.settings.quality == Quality::Fast, "Fast")
                                .clicked()
                            {
                                state.settings.quality = Quality::Fast;
                            }
                            if h.selectable_label(
                                state.settings.quality == Quality::Medium,
                                "Balanced",
                            )
                            .clicked()
                            {
                                state.settings.quality = Quality::Medium;
                            }
                            if h.selectable_label(state.settings.quality == Quality::High, "High")
                                .clicked()
                            {
                                state.settings.quality = Quality::High;
                            }
                        });
                        super::theme::window_slider(
                            &mut cols[0],
                            "Center",
                            &mut state.settings.window_center,
                            center_range.clone(),
                        );
                        super::theme::window_slider(
                            &mut cols[0],
                            "Width",
                            &mut state.settings.window_width,
                            width_range.clone(),
                        );
                        super::theme::slider(
                            &mut cols[0],
                            "Opacity",
                            &mut state.settings.global_opacity,
                            0.0..=1.0,
                        );

                        // Column 2: Transfer function and material bands.
                        super::theme::section(&mut cols[1], "DISPLAY PALETTE");
                        cols[1].horizontal(|h| {
                            if h.selectable_label(
                                state.transfer.preset == TransferPreset::Default,
                                "Materials",
                            )
                            .clicked()
                            {
                                state.transfer.preset = TransferPreset::Default;
                                state.actions.preset_changed = true;
                            }
                            if h.selectable_label(
                                state.transfer.preset == TransferPreset::Threat,
                                "Threat",
                            )
                            .clicked()
                            {
                                state.transfer.preset = TransferPreset::Threat;
                                state.actions.preset_changed = true;
                            }
                            if h.selectable_label(
                                state.transfer.preset == TransferPreset::Monochrome,
                                "Grayscale",
                            )
                            .clicked()
                            {
                                state.transfer.preset = TransferPreset::Monochrome;
                                state.actions.preset_changed = true;
                            }
                        });

                        cols[1].add_space(6.0);
                        match state.transfer.preset {
                            TransferPreset::Default => {
                                cols[1].horizontal_wrapped(|ui| {
                                    for band in &state.transfer.bands {
                                        ui.colored_label(band_color(band), "●");
                                        ui.label(egui::RichText::new(band.name).small())
                                            .on_hover_text(format!(
                                                "Density threshold: {}",
                                                band.threshold
                                            ));
                                    }
                                });
                            }
                            TransferPreset::Monochrome => {
                                let (rect, _) = cols[1].allocate_exact_size(
                                    egui::vec2(180.0, 28.0),
                                    egui::Sense::hover(),
                                );
                                for i in 0..90 {
                                    let x = rect.left() + i as f32 * 2.0;
                                    cols[1].painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(x, rect.top()),
                                            egui::vec2(2.0, 6.0),
                                        ),
                                        0,
                                        egui::Color32::from_gray(
                                            (0.9 * (i as f32 / 89.0).powf(0.65) * 255.0) as u8,
                                        ),
                                    );
                                }
                                let low = state.settings.window_center
                                    - state.settings.window_width * 0.5;
                                let high = state.settings.window_center
                                    + state.settings.window_width * 0.5;
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
                                    cols[1].painter().text(
                                        point,
                                        align,
                                        label,
                                        egui::FontId::proportional(12.0),
                                        super::theme::MUTED,
                                    );
                                }
                            }
                            TransferPreset::Threat => {
                                cols[1].label("Red density highlight");
                            }
                        }
                        if state.transfer.preset == TransferPreset::Default {
                            clamp_band_thresholds(&mut state.transfer.bands);
                            cols[1].collapsing("Edit material bands", |ui| {
                                ui.label("Band thresholds");
                                if draw_band_range_slider(ui, &mut state.transfer.bands) {
                                    state.transfer.bands_changed = true;
                                }
                                ui.add_space(6.0);
                                ui.label("Band opacity");

                                for band in &mut state.transfer.bands {
                                    ui.horizontal(|h| {
                                        h.colored_label(band_color(band), "■");
                                        h.label(band.name);
                                        let ar = h.add(
                                            egui::Slider::new(&mut band.alpha, 0.0..=2.0)
                                                .show_value(false),
                                        );
                                        h.label(format!("{:.0}%", band.alpha * 100.0));
                                        if slider_committed(&ar) {
                                            state.transfer.bands_changed = true;
                                        }
                                    });
                                }
                            });
                        }

                        if state.transfer.preset != TransferPreset::Default {
                            // Keep spacing roughly consistent with default preset layout.
                            cols[1].add_space(12.0);
                        }

                        // Column 3: Lighting.
                        cols[1].collapsing("Lighting & density", |ui| {
                            super::theme::slider(
                                ui,
                                "Depth cue",
                                &mut state.settings.depth_cue,
                                0.0..=1.0,
                            );
                            super::theme::slider(
                                ui,
                                "Density",
                                &mut state.settings.density_threshold,
                                0.0..=1.0,
                            );
                            ui.add(
                                egui::Slider::new(&mut state.settings.ambient, 0.0..=1.0)
                                    .text("Ambient"),
                            );
                            ui.add(
                                egui::Slider::new(&mut state.settings.diffuse, 0.0..=1.0)
                                    .text("Diffuse"),
                            );
                            ui.add(
                                egui::Slider::new(&mut state.settings.specular, 0.0..=1.0)
                                    .text("Specular"),
                            );
                        });
                    });
                });
        });
}

/// Keep image adjustments available during focused volume inspection.
pub(crate) fn draw_focus_controls(ctx: &egui::Context, state: &mut UiState) {
    let (center_range, width_range) =
        super::theme::window_ranges(state, state.library.active_3d_index.unwrap_or(0));
    egui::TopBottomPanel::bottom("focus_appearance")
        .frame(super::theme::frame())
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                super::theme::window_slider(
                    ui,
                    "Center",
                    &mut state.settings.window_center,
                    center_range,
                );
                super::theme::window_slider(
                    ui,
                    "Width",
                    &mut state.settings.window_width,
                    width_range,
                );
                super::theme::slider(ui, "Opacity", &mut state.settings.global_opacity, 0.0..=1.0);
                egui::ComboBox::from_id_salt("focus_palette")
                    .selected_text(match state.transfer.preset {
                        TransferPreset::Default => "Materials",
                        TransferPreset::Threat => "Threat",
                        TransferPreset::Monochrome => "Grayscale",
                    })
                    .show_ui(ui, |ui| {
                        for (preset, label) in [
                            (TransferPreset::Default, "Materials"),
                            (TransferPreset::Threat, "Threat"),
                            (TransferPreset::Monochrome, "Grayscale"),
                        ] {
                            if ui
                                .selectable_value(&mut state.transfer.preset, preset, label)
                                .changed()
                            {
                                state.actions.preset_changed = true;
                            }
                        }
                    });
                if state.cutaway && !state.slice_view.composite {
                    ui.horizontal(|ui| {
                        ui.label("Slice");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut state.slice_view.slice_index,
                                    0..=state.slice_view.max_slices.saturating_sub(1),
                                )
                                .custom_formatter(|v, _| format!("{}", v as usize + 1))
                                .custom_parser(|s| {
                                    s.parse::<usize>()
                                        .ok()
                                        .and_then(|v| v.checked_sub(1))
                                        .map(|v| v as f64)
                                }),
                            )
                            .changed()
                        {
                            state.slice_dirty = true;
                        }
                    });
                }
                ui.menu_button("Lighting", |ui| {
                    super::theme::slider(ui, "Depth cue", &mut state.settings.depth_cue, 0.0..=1.0);
                    super::theme::slider(ui, "Ambient", &mut state.settings.ambient, 0.0..=1.0);
                    super::theme::slider(ui, "Diffuse", &mut state.settings.diffuse, 0.0..=1.0);
                    super::theme::slider(ui, "Specular", &mut state.settings.specular, 0.0..=1.0);
                });
            });
        });
}
