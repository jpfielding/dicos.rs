//! Study library, details, detections and camera controls.
//!
//! The desktop layout stacks these sections in the left sidebar. Compact
//! layouts show the same sections as separate tabs, so each is its own
//! function.

use super::theme;
use crate::state::UiState;

/// Left sidebar: metadata, layers, camera presets.
pub(crate) fn draw_left_sidebar(ctx: &egui::Context, state: &mut UiState) {
    let touch_copy = ctx.input(|i| i.has_touch_screen());
    egui::SidePanel::left("sidebar")
        .default_width(232.0)
        .min_width(220.0)
        .max_width(280.0)
        .frame(theme::frame())
        .show(ctx, |panel| {
            panel.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            egui::ScrollArea::vertical().show(panel, |panel| {
                library_section(panel, state);
                panel.separator();
                if state.library.volumes.is_empty() {
                    return;
                }
                details_section(panel, state);
                if state.library.volumes.len() > 1 {
                    volumes_section(panel, state);
                    panel.separator();
                }
                detections_section(panel, state);
                panel.separator();
                camera_section(panel, state);
                panel.separator();
                panel.collapsing("Navigation help", |panel| {
                    navigation_help(panel, touch_copy);
                });
            });
        });
}

/// Open buttons, load progress, load errors and the current file name.
pub(crate) fn library_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Study library");

    // Open buttons are disabled while a background load is in flight (only one
    // load may run at a time).
    let is_loading = state.loading_file.is_some();
    ui.add_enabled_ui(!is_loading, |ui| {
        let open_file = theme::wide_button(ui, "Open file…").clicked();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if open_file {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("DICOS", &["dcs", "dcm"])
                    .pick_file()
                {
                    state.actions.file_to_load = Some(crate::loader::LoadSource::Path(path));
                }
            }
            if theme::wide_button(ui, "Open folder…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    state.actions.file_to_load = Some(crate::loader::LoadSource::Path(path));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if open_file {
                crate::web::pick_files();
            }
            if theme::wide_button(ui, "Load sample").clicked() {
                crate::web::load_sample();
            }
        }
    });

    load_status(ui, state);

    if let Some(path) = &state.library.loaded_path {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let label = egui::Label::new(name);
        // Compact layouts have no hover, so show the whole name.
        let label = if state.layout.is_compact() {
            label.wrap()
        } else {
            label.truncate()
        };
        ui.add(label).on_hover_text(path.display().to_string());
    }
}

/// Loading spinner and the last load error, with what to try next.
pub(crate) fn load_status(ui: &mut egui::Ui, state: &UiState) {
    if let Some(name) = &state.loading_file {
        ui.horizontal(|h| {
            h.add(egui::Spinner::new());
            h.label(format!("Loading {name}…"));
        });
    }
    if let Some(err) = &state.load_error {
        ui.add(egui::Label::new(egui::RichText::new(err).color(theme::DANGER)).wrap());
        ui.add(
            egui::Label::new(
                egui::RichText::new("Choose a DICOS .dcs or .dcm file to try again.")
                    .small()
                    .color(theme::MUTED),
            )
            .wrap(),
        );
    }
}

/// Dimensions, physical extent, modality and layer count of the 3D volume.
pub(crate) fn details_section(ui: &mut egui::Ui, state: &UiState) {
    theme::section(ui, "Study details");
    let Some(layer) = state
        .library
        .volumes
        .get(state.library.active_3d_index.unwrap_or(0))
    else {
        return;
    };
    let vol = &layer.volume;
    ui.spacing_mut().item_spacing.y = if theme::is_touch(ui) { 4.0 } else { 0.0 };
    theme::key_value(
        ui,
        "Size",
        &format!("{} × {} × {} voxels", vol.dim_x, vol.dim_y, vol.dim_z),
    );
    theme::key_value(
        ui,
        "Extent",
        &format!(
            "{:.0} × {:.0} × {:.0} mm",
            vol.dim_x as f64 * vol.voxel_spacing[0],
            vol.dim_y as f64 * vol.voxel_spacing[1],
            vol.dim_z as f64 * vol.voxel_spacing[2]
        ),
    );
    theme::key_value(ui, "Modality", &vol.modality);
    theme::key_value(ui, "Volumes", &format!("{}", state.library.volumes.len()));
    ui.add_space(4.0);
}

/// Layer picker; selecting a layer uploads it to both 3D and 2D.
pub(crate) fn volumes_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Volumes");
    let active = state.library.active_3d_index.unwrap_or(0);
    let height = ui.spacing().interact_size.y;
    for i in 0..state.library.volumes.len() {
        let is_active = i == active;
        let name = state.library.volumes[i].name.clone();
        let clicked = if state.layout.is_compact() {
            ui.add_sized(
                [ui.available_width(), height],
                egui::Button::selectable(is_active, name),
            )
            .clicked()
        } else {
            ui.selectable_label(is_active, &name).clicked()
        };
        if clicked && !is_active {
            state.actions.upload_3d_index = Some(i);
            state.slice_view.volume_index = i;
            state
                .slice_view
                .update_for_volume(&state.library.volumes[i].volume);
            state.slice_view.window_center = state.library.volumes[i].volume.window_center as f32;
            state.slice_view.window_width = state.library.volumes[i].volume.window_width as f32;
            state.library.selected_threat = None;
            state.slice_dirty = true;
        }
    }
}

/// Number of detections on the volume shown in 2D (for tab badges).
pub(crate) fn detection_count(state: &UiState) -> usize {
    state
        .library
        .volumes
        .get(state.slice_view.volume_index)
        .map_or(0, |layer| layer.volume.threats.len())
}

/// Threat boxes: overall toggle, bulk actions and one row per detection.
pub(crate) fn detections_section(ui: &mut egui::Ui, state: &mut UiState) {
    if state.library.volumes.is_empty() {
        return;
    }
    let threat_vol_idx = state
        .slice_view
        .volume_index
        .min(state.library.volumes.len() - 1);
    let count = state.library.volumes[threat_vol_idx].volume.threats.len();
    theme::section(
        ui,
        &if count > 0 {
            format!("Detections · {count}")
        } else {
            "Detections".to_owned()
        },
    );
    if count == 0 {
        ui.add(egui::Label::new("No detections were reported for this volume.").wrap());
        ui.add(
            egui::Label::new(
                egui::RichText::new("Detections appear when the study includes a threat report.")
                    .small()
                    .color(theme::MUTED),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        return;
    }
    if ui
        .checkbox(&mut state.library.show_threats, "Show detection boxes")
        .changed()
    {
        state.slice_dirty = true;
    }

    let mut any_changed = false;
    let mut new_selected = state.library.selected_threat;
    {
        let threats = &mut state.library.volumes[threat_vol_idx].volume.threats;
        if !matches!(new_selected, Some(i) if i < threats.len()) {
            new_selected = None;
        }

        ui.horizontal_wrapped(|h| {
            if h.button("Show all").clicked() {
                for threat in threats.iter_mut() {
                    threat.enabled = true;
                }
                any_changed = true;
            }
            if h.button("Hide all").clicked() {
                for threat in threats.iter_mut() {
                    threat.enabled = false;
                }
                any_changed = true;
            }
            if let Some(sel) = new_selected {
                if h.button("Only selected").clicked() {
                    for (i, threat) in threats.iter_mut().enumerate() {
                        threat.enabled = i == sel;
                    }
                    any_changed = true;
                }
            }
        });

        for (i, threat) in threats.iter_mut().enumerate() {
            let mut label = threat.name.clone();
            if let Some(conf) = threat.confidence {
                label.push_str(&format!(" ({conf:.2})"));
            }
            let is_selected = new_selected == Some(i);
            let mut picked = false;
            let mut next_enabled = threat.enabled;
            ui.horizontal(|h| {
                let c = egui::Color32::from_rgb(threat.color[0], threat.color[1], threat.color[2]);
                h.colored_label(c, "■");
                let checkbox_w = 84.0;
                let name_w =
                    (h.available_width() - checkbox_w - h.spacing().item_spacing.x).max(60.0);
                h.allocate_ui_with_layout(
                    egui::vec2(name_w, h.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |h| {
                        if h.selectable_label(is_selected, &label).clicked() {
                            picked = true;
                        }
                    },
                );
                h.checkbox(&mut next_enabled, "Show");
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
}

/// Axial / coronal / sagittal presets plus slice-plane tools.
pub(crate) fn camera_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Camera");
    let camera = &state.camera;
    let selected = if camera.azimuth.abs() < 0.001 && camera.elevation.abs() < 0.001 {
        Some(0)
    } else if camera.azimuth.abs() < 0.001
        && (camera.elevation + std::f32::consts::FRAC_PI_2 - 0.01).abs() < 0.001
    {
        Some(1)
    } else if (camera.azimuth - std::f32::consts::FRAC_PI_2).abs() < 0.001
        && camera.elevation.abs() < 0.001
    {
        Some(2)
    } else {
        None
    };
    let picked = theme::segmented(ui, selected, &["Axial", "Coronal", "Sagittal"]);
    match picked {
        Some(0) => state.camera.set_axial(),
        Some(1) => state.camera.set_coronal(),
        Some(2) => state.camera.set_sagittal(),
        _ => {}
    }
    if picked.is_some() {
        state.fit_camera = true;
    }
    ui.add_space(2.0);
    let tools_available = slice_tools_available(state);
    ui.add_enabled(
        tools_available,
        egui::Checkbox::new(&mut state.show_slice_locator, "Locate slice in 3D"),
    );
    ui.add_enabled(
        tools_available,
        egui::Checkbox::new(&mut state.cutaway, "Cutaway at slice"),
    )
    .on_hover_text("Hide voxels before the selected slice to reveal the interior");
    if state.cutaway {
        ui.checkbox(&mut state.reverse_cutaway, "Reverse cutaway");
    }
    if !tools_available {
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "Slice tools need a single slice of the 3D volume. \
                     Turn off Composite projection and show that volume in 2D.",
                )
                .small()
                .color(theme::MUTED),
            )
            .wrap(),
        );
    }
}

/// Locate and cutaway are meaningful only for one slice of the 3D volume.
pub(crate) fn slice_tools_available(state: &UiState) -> bool {
    !state.slice_view.composite
        && state.library.active_3d_index == Some(state.slice_view.volume_index)
}

/// How to drive the 3D view, in the input the person is using.
pub(crate) fn navigation_help(ui: &mut egui::Ui, touch: bool) {
    if touch {
        ui.label("One finger drag: orbit");
        ui.label("Two finger drag: pan");
        ui.label("Pinch: zoom");
    } else {
        ui.label("Drag: orbit");
        ui.label("Shift + drag: pan");
        ui.label("Scroll: zoom");
    }
    ui.label("Axial, Coronal, Sagittal: snap the view");
}
