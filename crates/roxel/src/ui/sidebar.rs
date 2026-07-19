//! Left sidebar: metadata, layers, threats, camera presets.

use crate::state::UiState;

/// Left sidebar: metadata, layers, camera presets.
pub(crate) fn draw_left_sidebar(ctx: &egui::Context, state: &mut UiState) {
    egui::SidePanel::left("sidebar")
        .default_width(220.0)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical().show(panel, |panel| {
                panel.heading("roxel");
                panel.separator();

                // File section. Open buttons are disabled while a background
                // load is in flight (only one load may run at a time).
                let is_loading = state.loading_file.is_some();
                panel.add_enabled_ui(!is_loading, |panel| {
                    if panel.button("Open file...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("DICOS", &["dcs", "dcm"])
                            .pick_file()
                        {
                            state.actions.file_to_load = Some(path);
                        }
                    }
                    if panel.button("Open folder...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            state.actions.file_to_load = Some(path);
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
                    panel.colored_label(egui::Color32::RED, err);
                }

                if let Some(path) = &state.library.loaded_path {
                    panel.label(format!(
                        "{}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }

                panel.separator();

                // Metadata section.
                if !state.library.volumes.is_empty() {
                    panel.heading("Metadata");
                    if let Some(layer) = state.library.volumes.first() {
                        let vol = &layer.volume;
                        panel.label(format!(
                            "{}x{}x{} ({})",
                            vol.dim_x, vol.dim_y, vol.dim_z, vol.modality
                        ));
                    }
                    panel.label(format!("{} volume(s)", state.library.volumes.len()));
                    panel.separator();
                }

                // Layers section -- selecting a layer uploads it to both 3D and 2D.
                if state.library.volumes.len() > 1 {
                    panel.heading("Layers");
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
                    panel.heading("Threats");
                    if panel
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
                        panel.label("No threats in selected volume");
                        panel.separator();
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

                // Camera presets.
                panel.heading("View");
                panel.horizontal(|h| {
                    if h.button("Axial").clicked() {
                        state.camera.set_axial();
                    }
                    if h.button("Coronal").clicked() {
                        state.camera.set_coronal();
                    }
                    if h.button("Sagittal").clicked() {
                        state.camera.set_sagittal();
                    }
                });

                panel.separator();
                panel.heading("Interaction Key");
                panel.label("LMB drag: Orbit 3D");
                panel.label("Shift + LMB drag: Pan 3D");
                panel.label("Mouse wheel: Zoom 3D");
                panel.label("Axial/Coronal/Sagittal: Snap view");
            });
        });
}
