//! Bottom panel: 3D rendering controls (quality, transfer function, lighting).

use crate::renderer::Quality;
use crate::state::UiState;
use crate::transfer::TransferPreset;

use super::band_slider::draw_band_range_slider;
use super::{band_color, clamp_band_thresholds, slider_committed};

/// Bottom panel: 3D rendering controls (quality, transfer function, lighting).
pub(crate) fn draw_bottom_controls(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::bottom("controls")
        .default_height(280.0)
        .resizable(true)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical().show(panel, |panel| {
                panel.columns(3, |cols| {
                    // Column 1: Rendering quality and window/level.
                    cols[0].heading("Rendering");
                    cols[0].horizontal(|h| {
                        h.label("Quality:");
                        if h.selectable_label(state.settings.quality == Quality::Fast, "Fast")
                            .clicked()
                        {
                            state.settings.quality = Quality::Fast;
                        }
                        if h.selectable_label(state.settings.quality == Quality::Medium, "Med")
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
                    cols[0].add(
                        egui::Slider::new(&mut state.settings.window_center, 0.0..=65535.0)
                            .text("WC"),
                    );
                    cols[0].add(
                        egui::Slider::new(&mut state.settings.window_width, 1.0..=65536.0)
                            .text("WW"),
                    );
                    cols[0].add(
                        egui::Slider::new(&mut state.settings.global_opacity, 0.0..=1.0)
                            .text("Opacity"),
                    );
                    cols[0].add(
                        egui::Slider::new(&mut state.settings.density_threshold, 0.0..=1.0)
                            .text("Density"),
                    );

                    // Column 2: Transfer function and material bands.
                    cols[1].heading("Transfer");
                    cols[1].horizontal(|h| {
                        if h.selectable_label(
                            state.transfer.preset == TransferPreset::Default,
                            "Default",
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
                            "Mono",
                        )
                        .clicked()
                        {
                            state.transfer.preset = TransferPreset::Monochrome;
                            state.actions.preset_changed = true;
                        }
                    });

                    if state.transfer.preset == TransferPreset::Default {
                        clamp_band_thresholds(&mut state.transfer.bands);
                        cols[1].label("Band thresholds");
                        if draw_band_range_slider(&mut cols[1], &mut state.transfer.bands) {
                            state.transfer.bands_changed = true;
                        }
                        cols[1].add_space(6.0);
                        cols[1].label("Band alpha");

                        for band in &mut state.transfer.bands {
                            cols[1].horizontal(|h| {
                                h.colored_label(band_color(band), "■");
                                h.label(band.name);
                                let ar = h.add(
                                    egui::Slider::new(&mut band.alpha, 0.0..=2.0).show_value(false),
                                );
                                h.label(format!("{:.0}%", band.alpha * 100.0));
                                if slider_committed(&ar) {
                                    state.transfer.bands_changed = true;
                                }
                            });
                        }
                    }

                    if state.transfer.preset != TransferPreset::Default {
                        // Keep spacing roughly consistent with default preset layout.
                        cols[1].add_space(12.0);
                    }

                    // Column 3: Lighting.
                    cols[2].heading("Lighting");
                    cols[2].add(
                        egui::Slider::new(&mut state.settings.ambient, 0.0..=1.0).text("Ambient"),
                    );
                    cols[2].add(
                        egui::Slider::new(&mut state.settings.diffuse, 0.0..=1.0).text("Diffuse"),
                    );
                    cols[2].add(
                        egui::Slider::new(&mut state.settings.specular, 0.0..=1.0).text("Specular"),
                    );
                });
            });
        });
}
