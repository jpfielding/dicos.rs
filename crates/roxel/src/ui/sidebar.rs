//! Left sidebar: metadata, layers, threats, camera presets.

use crate::state::UiState;

/// Left sidebar: metadata, layers, camera presets.
pub(crate) fn draw_left_sidebar(ctx: &egui::Context, state: &mut UiState) {
    egui::SidePanel::left("sidebar")
        .default_width(232.0)
        .min_width(220.0)
        .max_width(280.0)
        .frame(super::theme::frame())
        .show(ctx, |panel| {
            panel.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            egui::ScrollArea::vertical().show(panel, |panel| {
                super::theme::section(panel, "Study library");

                // File section. Open buttons are disabled while a background
                // load is in flight (only one load may run at a time).
                let is_loading = state.loading_file.is_some();
                panel.add_enabled_ui(!is_loading, |panel| {
                    let open_file = panel
                        .add_sized(
                            [panel.available_width(), 34.0],
                            egui::Button::new("Open file…"),
                        )
                        .clicked();
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if open_file {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("DICOS", &["dcs", "dcm"])
                                .pick_file()
                            {
                                state.actions.file_to_load =
                                    Some(crate::loader::LoadSource::Path(path));
                            }
                        }
                        if panel
                            .add_sized(
                                [panel.available_width(), 34.0],
                                egui::Button::new("Open folder…"),
                            )
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                state.actions.file_to_load =
                                    Some(crate::loader::LoadSource::Path(path));
                            }
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        if open_file {
                            crate::web::pick_files();
                        }
                        if panel
                            .add_sized(
                                [panel.available_width(), 34.0],
                                egui::Button::new("Load sample"),
                            )
                            .clicked()
                        {
                            crate::web::load_sample();
                        }
                    }
                });

                if let Some(name) = &state.loading_file {
                    panel.horizontal(|h| {
                        h.add(egui::Spinner::new());
                        h.label(format!("Loading {name}…"));
                    });
                }

                if let Some(err) = &state.load_error {
                    panel.colored_label(egui::Color32::from_rgb(246, 151, 141), err);
                }

                if let Some(path) = &state.library.loaded_path {
                    panel
                        .add(
                            egui::Label::new(
                                path.file_name().unwrap_or_default().to_string_lossy(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(path.display().to_string());
                }

                panel.separator();

                // Metadata section.
                if !state.library.volumes.is_empty() {
                    super::theme::section(panel, "Study details");
                    panel.spacing_mut().item_spacing.y = 6.0;
                    if let Some(layer) = state
                        .library
                        .volumes
                        .get(state.library.active_3d_index.unwrap_or(0))
                    {
                        let vol = &layer.volume;
                        panel.label(format!(
                            "{} × {} × {}  ·  {}",
                            vol.dim_x, vol.dim_y, vol.dim_z, vol.modality
                        ));
                    }
                    if let Some(layer) = state
                        .library
                        .volumes
                        .get(state.library.active_3d_index.unwrap_or(0))
                    {
                        let vol = &layer.volume;
                        panel.label(
                            egui::RichText::new(format!(
                                "{:.0} × {:.0} × {:.0} mm",
                                vol.dim_x as f64 * vol.voxel_spacing[0],
                                vol.dim_y as f64 * vol.voxel_spacing[1],
                                vol.dim_z as f64 * vol.voxel_spacing[2]
                            ))
                            .small()
                            .color(super::theme::MUTED),
                        );
                    }
                    panel.label(
                        egui::RichText::new(format!(
                            "{} volume{}",
                            state.library.volumes.len(),
                            if state.library.volumes.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ))
                        .color(super::theme::MUTED),
                    );
                    panel.spacing_mut().item_spacing.y = 10.0;
                    panel.add_space(6.0);
                }

                // Layers section -- selecting a layer uploads it to both 3D and 2D.
                if state.library.volumes.len() > 1 {
                    super::theme::section(panel, "Volumes");
                    let active = state.library.active_3d_index.unwrap_or(0);
                    for i in 0..state.library.volumes.len() {
                        let is_active = i == active;
                        let name = state.library.volumes[i].name.clone();
                        if panel.selectable_label(is_active, &name).clicked() && !is_active {
                            state.actions.upload_3d_index = Some(i);
                            state.slice_view.volume_index = i;
                            state
                                .slice_view
                                .update_for_volume(&state.library.volumes[i].volume);
                            state.slice_view.window_center =
                                state.library.volumes[i].volume.window_center as f32;
                            state.slice_view.window_width =
                                state.library.volumes[i].volume.window_width as f32;
                            state.library.selected_threat = None;
                            state.slice_dirty = true;
                        }
                    }
                    panel.separator();
                }

                if !state.library.volumes.is_empty() {
                    let threat_vol_idx = state
                        .slice_view
                        .volume_index
                        .min(state.library.volumes.len() - 1);
                    super::theme::section(panel, "Detections");
                    if !state.library.volumes[threat_vol_idx]
                        .volume
                        .threats
                        .is_empty()
                        && panel
                            .checkbox(&mut state.library.show_threats, "Show threat boxes")
                            .changed()
                    {
                        state.slice_dirty = true;
                    }

                    if state.library.volumes[threat_vol_idx]
                        .volume
                        .threats
                        .is_empty()
                    {
                        panel.label(
                            egui::RichText::new("No detection annotations")
                                .color(super::theme::MUTED),
                        );
                        panel.add_space(6.0);
                    } else {
                        let mut any_changed = false;
                        let mut new_selected = state.library.selected_threat;
                        {
                            let threats = &mut state.library.volumes[threat_vol_idx].volume.threats;
                            if !matches!(new_selected, Some(i) if i < threats.len()) {
                                new_selected = None;
                            }

                            panel.horizontal(|h| {
                                if h.button("All On").clicked() {
                                    for threat in threats.iter_mut() {
                                        threat.enabled = true;
                                    }
                                    any_changed = true;
                                }
                                if h.button("All Off").clicked() {
                                    for threat in threats.iter_mut() {
                                        threat.enabled = false;
                                    }
                                    any_changed = true;
                                }
                            });

                            if let Some(sel) = new_selected {
                                panel.horizontal(|h| {
                                    if h.button("Only Selected").clicked() {
                                        for (i, threat) in threats.iter_mut().enumerate() {
                                            threat.enabled = i == sel;
                                        }
                                        any_changed = true;
                                    }
                                });
                            }

                            for (i, threat) in threats.iter_mut().enumerate() {
                                let mut label = threat.name.clone();
                                if let Some(conf) = threat.confidence {
                                    label.push_str(&format!(" ({conf:.2})"));
                                }
                                let is_selected = new_selected == Some(i);
                                let mut picked = false;
                                let mut next_enabled = threat.enabled;
                                panel.horizontal(|h| {
                                    let c = egui::Color32::from_rgb(
                                        threat.color[0],
                                        threat.color[1],
                                        threat.color[2],
                                    );
                                    h.colored_label(c, "■");
                                    if h.selectable_label(is_selected, &label).clicked() {
                                        picked = true;
                                    }
                                    h.checkbox(&mut next_enabled, "on");
                                });
                                if picked {
                                    new_selected = Some(i);
                                }
                                if next_enabled != threat.enabled {
                                    threat.enabled = next_enabled;
                                    any_changed = true;
                                }
                            }
                        }
                        state.library.selected_threat = new_selected;
                        if any_changed {
                            state.slice_dirty = true;
                        }
                        panel.separator();
                    }
                }

                if state.library.volumes.is_empty() {
                    return;
                }
                // Camera presets.
                super::theme::section(panel, "Camera");
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(34, 46, 59))
                    .inner_margin(3)
                    .corner_radius(4)
                    .show(panel, |panel| {
                        panel.horizontal(|h| {
                            h.spacing_mut().button_padding.x = 6.0;
                            h.spacing_mut().item_spacing.x = 4.0;
                            if h.selectable_label(
                                state.camera.azimuth.abs() < 0.001
                                    && state.camera.elevation.abs() < 0.001,
                                "Axial",
                            )
                            .clicked()
                            {
                                state.camera.set_axial();
                                state.fit_camera = true;
                            }
                            if h.selectable_label(
                                state.camera.azimuth.abs() < 0.001
                                    && (state.camera.elevation + std::f32::consts::FRAC_PI_2
                                        - 0.01)
                                        .abs()
                                        < 0.001,
                                "Coronal",
                            )
                            .clicked()
                            {
                                state.camera.set_coronal();
                                state.fit_camera = true;
                            }
                            if h.selectable_label(
                                (state.camera.azimuth - std::f32::consts::FRAC_PI_2).abs() < 0.001
                                    && state.camera.elevation.abs() < 0.001,
                                "Sagittal",
                            )
                            .clicked()
                            {
                                state.camera.set_sagittal();
                                state.fit_camera = true;
                            }
                        });
                    });
                panel.add_enabled(
                    !state.slice_view.composite
                        && state.library.active_3d_index == Some(state.slice_view.volume_index),
                    egui::Checkbox::new(&mut state.show_slice_locator, "Locate slice in 3D"),
                );
                panel
                    .add_enabled(
                        !state.slice_view.composite
                            && state.library.active_3d_index == Some(state.slice_view.volume_index),
                        egui::Checkbox::new(&mut state.cutaway, "Cutaway at slice"),
                    )
                    .on_hover_text("Hide voxels before the selected slice to reveal the interior");
                if state.cutaway {
                    panel.checkbox(&mut state.reverse_cutaway, "Reverse cutaway");
                }
                panel.separator();
                panel.collapsing("Navigation help", |panel| {
                    panel.label("LMB drag: Orbit 3D");
                    panel.label("Shift + LMB drag: Pan 3D");
                    panel.label("Mouse wheel: Zoom 3D");
                    panel.label("Axial/Coronal/Sagittal: Snap view");
                });
            });
        });
}
