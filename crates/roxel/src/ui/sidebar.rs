//! The study panel: the open study, opening another, its details, its volumes
//! and its detections.
//!
//! Desktop stacks these sections in the left sidebar. Compact layouts show
//! them as the Study and Detections pages, so each is its own function.

use super::theme;
use crate::state::UiState;

pub(crate) const SIDEBAR_WIDTH: f32 = 232.0;

/// Left sidebar: everything about the study, nothing about a view.
pub(crate) fn draw_left_sidebar(ctx: &egui::Context, state: &mut UiState) {
    egui::SidePanel::left("sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .frame(theme::frame())
        .show(ctx, |panel| {
            panel.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            egui::ScrollArea::vertical().show(panel, |panel| {
                study_section(panel, state);
                theme::rule(panel);
                details_section(panel, state);
                if state.library.volumes.len() > 1 {
                    theme::rule(panel);
                    volumes_section(panel, state);
                }
                theme::rule(panel);
                detections_section(panel, state);
            });
        });
}

/// The open study as the selected item, then the ways to open another.
pub(crate) fn study_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Study");
    study_card(ui, state);
    open_buttons(ui, state, false);
    load_status(ui, state);
}

/// The open study, styled as the selected item of the library and showing its
/// whole name: nothing is cut off, on any screen.
fn study_card(ui: &mut egui::Ui, state: &UiState) {
    let Some(path) = &state.library.loaded_path else {
        return;
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let card = egui::Frame::new()
        .fill(egui::Color32::from_rgb(27, 47, 52))
        .corner_radius(6)
        .inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 8,
            bottom: 9,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Left-aligned whatever the parent does: a justified parent would
            // stretch the name's letters apart.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(egui::RichText::new("Open now").small().color(theme::ACCENT));
                ui.add(
                    egui::Label::new(egui::RichText::new(name.as_ref()).color(theme::HEADING))
                        .wrap(),
                );
            });
        })
        .response;
    // The accent edge marks it as the current selection without relying on
    // the tint alone.
    let edge = egui::Rect::from_min_max(
        card.rect.left_top(),
        egui::pos2(card.rect.left() + 4.0, card.rect.bottom()),
    );
    ui.painter().rect_filled(
        edge,
        egui::CornerRadius {
            nw: 6,
            sw: 6,
            ne: 0,
            se: 0,
        },
        theme::ACCENT,
    );
    card.on_hover_text(path.display().to_string());
}

/// "Open file…" and, where the platform can pick one, "Open folder…". The
/// first use screen makes the file button primary.
pub(crate) fn open_buttons(ui: &mut egui::Ui, state: &mut UiState, first_use: bool) {
    // Only one load may run at a time.
    let idle = state.loading_file.is_none();
    let compact = state.layout.is_compact();
    ui.add_enabled_ui(idle, |ui| {
        let height = ui.spacing().interact_size.y.max(30.0);
        let size = egui::vec2(ui.available_width(), if first_use { 44.0 } else { height });
        let open_file = if first_use {
            theme::primary_button(ui, "Open file…", size).clicked()
        } else {
            ui.add_sized(size, egui::Button::new("Open file…"))
                .clicked()
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if open_file {
                state.actions.file_to_load = rfd::FileDialog::new()
                    .add_filter("DICOS", &["dcs", "dcm"])
                    .pick_file()
                    .map(crate::loader::LoadSource::Path);
            }
            // Phone browsers cannot pick folders, so phone layouts leave it out.
            if !compact
                && ui
                    .add_sized(size, egui::Button::new("Open folder…"))
                    .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    state.actions.file_to_load = Some(crate::loader::LoadSource::Path(path));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = compact;
            if open_file {
                crate::web::pick_files();
            }
            if ui
                .add_sized(size, egui::Button::new("Load sample"))
                .clicked()
            {
                crate::web::load_sample();
            }
        }
    });
}

/// Loading spinner and the last load error, with what to try next.
pub(crate) fn load_status(ui: &mut egui::Ui, state: &UiState) {
    if let Some(name) = &state.loading_file {
        ui.horizontal(|h| {
            h.add(egui::Spinner::new());
            h.add(egui::Label::new(format!("Loading {name}…")).wrap());
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
    theme::section(ui, "Details");
    let Some(layer) = state
        .library
        .volumes
        .get(state.library.active_3d_index.unwrap_or(0))
    else {
        return;
    };
    let vol = &layer.volume;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        theme::key_value(
            ui,
            "Voxels",
            &format!("{} × {} × {}", vol.dim_x, vol.dim_y, vol.dim_z),
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
    });
}

/// Show layer `index` in both the 3D view and the 2D slice.
pub(crate) fn select_volume(state: &mut UiState, index: usize) {
    state.actions.upload_3d_index = Some(index);
    show_volume_in_slice(state, index);
}

/// Show layer `index` in the 2D slice only.
pub(crate) fn show_volume_in_slice(state: &mut UiState, index: usize) {
    let Some(layer) = state.library.volumes.get(index) else {
        return;
    };
    state.slice_view.volume_index = index;
    state.slice_view.update_for_volume(&layer.volume);
    state.slice_view.window_center = layer.volume.window_center as f32;
    state.slice_view.window_width = layer.volume.window_width as f32;
    state.library.selected_threat = None;
    state.slice_dirty = true;
}

/// Layer picker; selecting a layer shows it in both 3D and 2D.
pub(crate) fn volumes_section(ui: &mut egui::Ui, state: &mut UiState) {
    theme::section(ui, "Volumes");
    let active = state.library.active_3d_index.unwrap_or(0);
    let height = ui.spacing().interact_size.y;
    for i in 0..state.library.volumes.len() {
        let is_active = i == active;
        let name = state.library.volumes[i].name.clone();
        let clicked = ui
            .add_sized(
                [ui.available_width(), height],
                egui::Button::selectable(is_active, name),
            )
            .clicked();
        if clicked && !is_active {
            select_volume(state, i);
        }
    }
}

/// Number of detections on the volume shown in 2D (for the tab badge).
pub(crate) fn detection_count(state: &UiState) -> usize {
    state
        .library
        .volumes
        .get(state.slice_view.volume_index)
        .map_or(0, |layer| layer.volume.threats.len())
}

/// Index of the volume whose detections are listed (the one shown in 2D).
fn detections_volume(state: &UiState) -> Option<usize> {
    let last = state.library.volumes.len().checked_sub(1)?;
    Some(state.slice_view.volume_index.min(last))
}

/// Detections: heading, the overall toggle, bulk actions, then the list.
pub(crate) fn detections_section(ui: &mut egui::Ui, state: &mut UiState) {
    if detections_summary(ui, state) {
        detections_list(ui, state);
    }
}

/// Heading with the count, then either the one sentence saying no report was
/// loaded or the controls that act on every detection. Returns true when
/// there are detections to list.
pub(crate) fn detections_summary(ui: &mut egui::Ui, state: &mut UiState) -> bool {
    let Some(index) = detections_volume(state) else {
        return false;
    };
    let count = state.library.volumes[index].volume.threats.len();
    theme::section_with_note(
        ui,
        "Detections",
        &if count > 0 {
            format!("{count} reported")
        } else {
            String::new()
        },
    );
    if count == 0 {
        // Says what is missing; never that the scan is clear.
        ui.add(egui::Label::new("No threat report was loaded with this study.").wrap());
        return false;
    }
    if theme::checkbox(ui, &mut state.library.show_threats, "Show detection boxes").changed() {
        state.slice_dirty = true;
    }
    let selected = state
        .library
        .selected_threat
        .filter(|selected| *selected < count);
    let threats = &mut state.library.volumes[index].volume.threats;
    let mut show = |keep: &dyn Fn(usize) -> bool| {
        for (i, threat) in threats.iter_mut().enumerate() {
            threat.enabled = keep(i);
        }
        state.slice_dirty = true;
    };
    match theme::button_row(ui, &["Show all", "Hide all"]) {
        Some(0) => show(&|_| true),
        Some(1) => show(&|_| false),
        _ => {}
    }
    // Always present so the list never jumps; it needs a selection.
    let only = ui
        .add_enabled_ui(selected.is_some(), |ui| {
            let height = ui.spacing().interact_size.y;
            ui.add_sized(
                [ui.available_width(), height],
                egui::Button::new("Show only the selected one"),
            )
        })
        .inner;
    if let (true, Some(selected)) = (only.clicked(), selected) {
        show(&|i| i == selected);
    }
    true
}

/// One row per detection: a box to show or hide it, and the row itself to
/// select it.
pub(crate) fn detections_list(ui: &mut egui::Ui, state: &mut UiState) {
    let Some(index) = detections_volume(state) else {
        return;
    };
    let threats = &mut state.library.volumes[index].volume.threats;
    let mut selected = state
        .library
        .selected_threat
        .filter(|selected| *selected < threats.len());
    let mut changed = false;
    for (i, threat) in threats.iter_mut().enumerate() {
        let color = egui::Color32::from_rgb(threat.color[0], threat.color[1], threat.color[2]);
        ui.horizontal(|ui| {
            let name = format!("Show {}", threat.name);
            changed |= theme::checkbox_named(ui, &mut threat.enabled, "", &name)
                .on_hover_text("Show this detection")
                .changed();
            if detection_row(ui, threat, color, selected == Some(i)) {
                selected = Some(i);
            }
        });
    }
    state.library.selected_threat = selected;
    if changed {
        state.slice_dirty = true;
    }
}

/// One detection as a selectable row: colour swatch, name, and the report's
/// confidence as a percentage.
/// Returns true when it was clicked.
fn detection_row(
    ui: &mut egui::Ui,
    threat: &crate::volume::ThreatBox,
    color: egui::Color32,
    selected: bool,
) -> bool {
    let height = ui.spacing().interact_size.y.max(28.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        let name = match threat.confidence {
            Some(confidence) => {
                format!("{}, confidence {:.0}%", threat.name, confidence * 100.0)
            }
            None => threat.name.clone(),
        };
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name)
    });
    let painter = ui.painter();
    if selected {
        painter.rect(
            rect,
            5,
            theme::SELECTED,
            egui::Stroke::new(1.0_f32, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 5, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            egui::vec2(10.0, 10.0),
        ),
        2,
        color,
    );
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut right = rect.right() - 8.0;
    if let Some(confidence) = threat.confidence {
        let shown = painter.text(
            egui::pos2(right, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.0}%", confidence * 100.0),
            font.clone(),
            theme::MUTED,
        );
        right = shown.left() - 8.0;
    }
    let left = rect.left() + 28.0;
    let name = theme::elide_middle(painter, &threat.name, font.clone(), right - left);
    painter.text(
        egui::pos2(left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        if selected {
            theme::HEADING
        } else {
            theme::TEXT
        },
    );
    response.on_hover_text(&threat.name).clicked()
}

/// The slice plane and cutaway are meaningful only for one slice of the volume
/// shown in 3D.
pub(crate) fn slice_tools_available(state: &UiState) -> bool {
    !state.slice_view.composite
        && state.library.active_3d_index == Some(state.slice_view.volume_index)
}

/// Why the slice-plane tools are disabled, in one short line.
pub(crate) const SLICE_TOOLS_NOTE: &str = "Needs a single 2D slice of this volume";
