//! Right panel: 2D slice view and its controls.

use crate::slice_view::Orientation;
use crate::state::UiState;

use super::slider_committed;

/// Right panel: 2D slice view and its controls.
pub(crate) fn draw_right_panel(ctx: &egui::Context, state: &mut UiState) {
    let (center_range, width_range) =
        super::theme::window_ranges(state, state.slice_view.volume_index);
    egui::SidePanel::right("slice_panel")
        .default_width(336.0)
        .frame(super::theme::frame())
        .min_width(310.0)
        .max_width(440.0)
        .show(ctx, |panel| {
            panel.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(if state.slice_view.composite {
                        "2D Projection"
                    } else {
                        "2D Slice"
                    })
                    .strong(),
                );
                ui.label(
                    egui::RichText::new(if state.slice_view.composite {
                        "/  Composite"
                    } else {
                        "/  Cross-section"
                    })
                    .color(super::theme::MUTED),
                );
            });
            panel.separator();

            // Display the slice image with zoom + scroll.
            if let Some(tex) = &state.slice_texture {
                let available = panel.available_size();
                let controls_height = 430.0;
                let tex_size = tex.size_vec2();
                let viewport_h = (available.y - controls_height)
                    .max(100.0)
                    .min((available.x / tex_size.x * tex_size.y + 32.0).max(160.0));

                // Compute base scale (fit-to-width at 100%).
                let base_scale = if tex_size.x > 0.0 {
                    (available.x / tex_size.x).min(viewport_h / tex_size.y)
                } else {
                    1.0
                };
                let scale = base_scale * (state.slice_view.zoom / 100.0);
                let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);

                egui::Frame::new()
                    .fill(super::theme::BG)
                    .corner_radius(6)
                    .show(panel, |well| {
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .max_height(viewport_h)
                            .show(well, |scroll| {
                                scroll.set_min_size(egui::vec2(available.x, viewport_h));
                                scroll.add_space(((viewport_h - display_size.y) * 0.5).max(0.0));
                                scroll.horizontal(|ui| {
                                    ui.add_space(((available.x - display_size.x) * 0.5).max(0.0));
                                    ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
                                });
                            });
                    });
                panel.label(
                    egui::RichText::new(format!(
                        "{}  ·  {} × {} pixels",
                        state.slice_view.orientation.label(),
                        tex.size()[0],
                        tex.size()[1]
                    ))
                    .small()
                    .color(super::theme::MUTED),
                );
            } else {
                let available = panel.available_size();
                let rect_h = (available.y - 340.0).max(100.0);
                let (rect, _) = panel
                    .allocate_exact_size(egui::vec2(available.x, rect_h), egui::Sense::hover());
                panel.painter().rect_filled(rect, 0.0, super::theme::BG);
                panel.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Slice preview",
                    egui::FontId::proportional(14.0),
                    super::theme::MUTED,
                );
            }

            panel.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("INSPECTOR")
                        .size(11.0)
                        .color(super::theme::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Expand image").clicked() {
                        state.slice_expanded = true;
                    }
                    if ui.button("Fit image").clicked() {
                        state.slice_view.zoom = 100.0;
                    }
                });
            });
            panel.separator();

            egui::ScrollArea::vertical()
                .id_salt("slice_controls")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(panel, |panel| {
                    panel.add_enabled_ui(!state.library.volumes.is_empty(), |panel| {
                        // Volume selector dropdown.
                        if !state.library.volumes.is_empty() {
                            let current_name = state
                                .library
                                .volumes
                                .get(state.slice_view.volume_index)
                                .map(|l| l.name.as_str())
                                .unwrap_or("(none)");

                            panel.horizontal(|panel| {
                                panel.add_sized(
                                    [58.0, 26.0],
                                    egui::Label::new(
                                        egui::RichText::new("Volume").color(super::theme::MUTED),
                                    ),
                                );
                                egui::ComboBox::from_id_salt("slice_volume")
                                    .width(150.0)
                                    .truncate()
                                    .selected_text(current_name)
                                    .show_ui(panel, |combo| {
                                        for (i, layer) in state.library.volumes.iter().enumerate() {
                                            if combo
                                                .selectable_value(
                                                    &mut state.slice_view.volume_index,
                                                    i,
                                                    &layer.name,
                                                )
                                                .clicked()
                                            {
                                                state.slice_view.update_for_volume(&layer.volume);
                                                state.slice_view.window_center =
                                                    layer.volume.window_center as f32;
                                                state.slice_view.window_width =
                                                    layer.volume.window_width as f32;
                                                state.library.selected_threat = None;
                                                state.slice_dirty = true;
                                            }
                                        }
                                    });
                            });
                        }

                        // Orientation dropdown.
                        let prev_orientation = state.slice_view.orientation;
                        panel.horizontal(|panel| {
                            panel.add_sized(
                                [58.0, 26.0],
                                egui::Label::new(
                                    egui::RichText::new("Plane").color(super::theme::MUTED),
                                ),
                            );
                            egui::ComboBox::from_id_salt("slice_orientation")
                                .selected_text(state.slice_view.orientation.label())
                                .show_ui(panel, |combo| {
                                    for orient in Orientation::ALL {
                                        combo.selectable_value(
                                            &mut state.slice_view.orientation,
                                            orient,
                                            orient.label(),
                                        );
                                    }
                                });
                        });
                        if state.slice_view.orientation != prev_orientation {
                            // Update max_slices for new orientation.
                            if let Some(layer) =
                                state.library.volumes.get(state.slice_view.volume_index)
                            {
                                state.slice_view.update_for_volume(&layer.volume);
                            }
                            state.slice_dirty = true;
                        }

                        // Composite view toggle.
                        let prev_composite = state.slice_view.composite;
                        panel.checkbox(&mut state.slice_view.composite, "Composite projection");
                        if state.slice_view.composite != prev_composite {
                            state.slice_dirty = true;
                        }

                        panel.label(
                            egui::RichText::new("Windowing")
                                .small()
                                .color(super::theme::MUTED),
                        );
                        // Window/Level controls (debounced -- only re-render on drag end).
                        let wl_r = super::theme::window_slider(
                            panel,
                            "Center",
                            &mut state.slice_view.window_center,
                            center_range.clone(),
                        );
                        if slider_committed(&wl_r) {
                            state.slice_dirty = true;
                        }
                        let ww_r = super::theme::window_slider(
                            panel,
                            "Width",
                            &mut state.slice_view.window_width,
                            width_range.clone(),
                        );
                        if slider_committed(&ww_r) {
                            state.slice_dirty = true;
                        }

                        // Slice slider (hidden in composite mode).
                        if !state.slice_view.composite && state.slice_view.max_slices > 1 {
                            let max = state.slice_view.max_slices - 1;
                            if panel
                                .horizontal(|ui| {
                                    ui.add_sized(
                                        [58.0, 26.0],
                                        egui::Label::new(
                                            egui::RichText::new("Slice").color(super::theme::MUTED),
                                        ),
                                    );
                                    ui.spacing_mut().interact_size.y = 20.0;
                                    ui.add(
                                        egui::Slider::new(
                                            &mut state.slice_view.slice_index,
                                            0..=max,
                                        )
                                        .custom_formatter(|v, _| format!("{}", v as usize + 1))
                                        .custom_parser(
                                            |s| {
                                                s.parse::<usize>()
                                                    .ok()
                                                    .and_then(|v| v.checked_sub(1))
                                                    .map(|v| v as f64)
                                            },
                                        ),
                                    )
                                })
                                .inner
                                .changed()
                            {
                                state.slice_dirty = true;
                            }
                        }

                        // Zoom slider.
                        super::theme::slider(
                            panel,
                            "Zoom %",
                            &mut state.slice_view.zoom,
                            50.0..=500.0,
                        );
                    });
                });
        });
}

/// A larger inspection surface keeps small-window previews useful without
/// hiding the primary controls from the main workspace.
pub(crate) fn draw_expanded(ctx: &egui::Context, state: &mut UiState) {
    if !state.slice_expanded {
        return;
    }
    let mut close = false;
    let size = ctx.content_rect().size() * 0.86;
    let (center_range, width_range) =
        super::theme::window_ranges(state, state.slice_view.volume_index);
    let response = egui::Modal::new(egui::Id::new("expanded_slice"))
        .frame(super::theme::frame().corner_radius(8))
        .backdrop_color(egui::Color32::from_black_alpha(190))
        .show(ctx, |ui| {
            ui.set_width(size.x);
            ui.horizontal(|ui| {
                ui.heading("Slice inspection");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Back to workspace").clicked() { close = true; }
                    if ui.button("Fit image").clicked() { state.expanded_zoom=100.0; }
                });
            });
            let name = state.library.volumes.get(state.slice_view.volume_index).map(|layer| layer.name.as_str()).unwrap_or("Loading");
            ui.label(egui::RichText::new(format!("{}  /  {}  /  {}", name, state.slice_view.orientation.label(), if state.slice_view.composite { "Composite projection" } else { "Cross-section" })).color(super::theme::MUTED));
            if let Some(texture) = &state.slice_texture {
                let available = egui::vec2(size.x, (size.y-190.0).max(100.0));
                let scale = (available.x / texture.size_vec2().x).min(available.y / texture.size_vec2().y);
                let image_size = texture.size_vec2() * scale * (state.expanded_zoom / 100.0);
                egui::Frame::new().fill(super::theme::BG).corner_radius(4).show(ui, |ui| {
                    egui::ScrollArea::both().id_salt("expanded_image_pan").auto_shrink([false,false]).max_height(available.y).show(ui, |ui| {
                        ui.set_min_size(available);
                        ui.add_space(((available.y-image_size.y)*0.5).max(0.0));
                        ui.horizontal(|ui| { ui.add_space(((available.x-image_size.x)*0.5).max(0.0)); ui.image(egui::load::SizedTexture::new(texture.id(), image_size)); });
                    });
                });
            }
            if !state.slice_view.composite {
                ui.horizontal(|ui| {
                    ui.label("Slice");
                    if ui.add_enabled(state.slice_view.slice_index>0, egui::Button::new("Previous")).clicked() { state.slice_view.slice_index-=1; state.slice_dirty=true; }
                    ui.spacing_mut().interact_size.y = 20.0;
                    ui.spacing_mut().slider_width = (ui.available_width()-190.0).max(100.0);
                    if ui.add(egui::Slider::new(&mut state.slice_view.slice_index, 0..=state.slice_view.max_slices.saturating_sub(1))
                        .custom_formatter(|v,_|format!("{}",v as usize+1))
                        .custom_parser(|s|s.parse::<usize>().ok().and_then(|v|v.checked_sub(1)).map(|v|v as f64))).changed() { state.slice_dirty=true; }
                    ui.label(format!("/ {}",state.slice_view.max_slices));
                    if ui.add_enabled(state.slice_view.slice_index+1<state.slice_view.max_slices,egui::Button::new("Next")).clicked() { state.slice_view.slice_index+=1; state.slice_dirty=true; }
                });
            }
            ui.columns(3, |cols| {
                let center = super::theme::window_slider(&mut cols[0], "Center", &mut state.slice_view.window_center, center_range);
                let width = super::theme::window_slider(&mut cols[1], "Width", &mut state.slice_view.window_width, width_range);
                super::theme::slider(&mut cols[2], "Zoom %", &mut state.expanded_zoom, 50.0..=500.0);
                if slider_committed(&center) || slider_committed(&width) { state.slice_dirty=true; }
            });
            ui.label(egui::RichText::new("Drag image to pan · Zoom for detail · Select numbers to edit · Esc returns to workspace").small().color(super::theme::MUTED));
        });
    if close || response.should_close() {
        state.slice_expanded = false;
    }
}
