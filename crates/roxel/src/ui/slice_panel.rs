//! Right panel: 2D slice view and its controls.

use crate::slice_view::Orientation;
use crate::state::UiState;

use super::slider_committed;

/// Right panel: 2D slice view and its controls.
pub(crate) fn draw_right_panel(ctx: &egui::Context, state: &mut UiState) {
    egui::SidePanel::right("slice_panel")
        .default_width(320.0)
        .min_width(200.0)
        .show(ctx, |panel| {
            panel.heading("2D Slice");
            panel.separator();

            // Display the slice image with zoom + scroll.
            if let Some(tex) = &state.slice_texture {
                let available = panel.available_size();
                let controls_height = 220.0;
                let viewport_h = (available.y - controls_height).max(100.0);
                let tex_size = tex.size_vec2();

                // Compute base scale (fit-to-width at 100%).
                let base_scale = if tex_size.x > 0.0 {
                    available.x / tex_size.x
                } else {
                    1.0
                };
                let scale = base_scale * (state.slice_view.zoom / 100.0);
                let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);

                egui::ScrollArea::both()
                    .max_height(viewport_h)
                    .show(panel, |scroll| {
                        scroll.image(egui::load::SizedTexture::new(tex.id(), display_size));
                    });
            } else {
                let available = panel.available_size();
                let rect_h = (available.y - 220.0).max(100.0);
                let (rect, _) = panel
                    .allocate_exact_size(egui::vec2(available.x, rect_h), egui::Sense::hover());
                panel
                    .painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_gray(200));
                panel.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "No volume loaded",
                    egui::FontId::proportional(14.0),
                    egui::Color32::GRAY,
                );
            }

            panel.separator();

            // Volume selector dropdown.
            if !state.library.volumes.is_empty() {
                let current_name = state
                    .library
                    .volumes
                    .get(state.slice_view.volume_index)
                    .map(|l| l.name.as_str())
                    .unwrap_or("(none)");

                egui::ComboBox::from_label("Volume")
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
                                state.slice_view.window_center = layer.volume.window_center as f32;
                                state.slice_view.window_width = layer.volume.window_width as f32;
                                state.library.selected_threat = None;
                                state.slice_dirty = true;
                            }
                        }
                    });
            }

            // Orientation dropdown.
            let prev_orientation = state.slice_view.orientation;
            egui::ComboBox::from_label("View")
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
            if state.slice_view.orientation != prev_orientation {
                // Update max_slices for new orientation.
                if let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) {
                    state.slice_view.update_for_volume(&layer.volume);
                }
                state.slice_dirty = true;
            }

            // Composite view toggle.
            let prev_composite = state.slice_view.composite;
            panel.checkbox(&mut state.slice_view.composite, "Composite View");
            if state.slice_view.composite != prev_composite {
                state.slice_dirty = true;
            }

            // Window/Level controls (debounced -- only re-render on drag end).
            let wl_r = panel.add(
                egui::Slider::new(&mut state.slice_view.window_center, 0.0..=65535.0).text("W/L"),
            );
            if slider_committed(&wl_r) {
                state.slice_dirty = true;
            }
            let ww_r = panel.add(
                egui::Slider::new(&mut state.slice_view.window_width, 1.0..=65536.0).text("W/W"),
            );
            if slider_committed(&ww_r) {
                state.slice_dirty = true;
            }

            // Slice slider (hidden in composite mode).
            if !state.slice_view.composite && state.slice_view.max_slices > 1 {
                let max = state.slice_view.max_slices - 1;
                if panel
                    .add(
                        egui::Slider::new(&mut state.slice_view.slice_index, 0..=max).text("Slice"),
                    )
                    .changed()
                {
                    state.slice_dirty = true;
                }
            }

            // Zoom slider.
            panel.add(egui::Slider::new(&mut state.slice_view.zoom, 50.0..=500.0).text("Zoom %"));
        });
}
