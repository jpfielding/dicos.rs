//! egui UI drawing.
//!
//! The three-panel layout is split across sibling modules, one per panel, plus
//! the custom band-range slider widget:
//!
//! - [`sidebar`] — left sidebar (metadata, layers, threats, camera presets).
//! - [`slice_panel`] — right panel (2D slice view and its controls).
//! - [`controls`] — bottom panel (3D rendering controls).
//! - [`band_slider`] — the custom transfer-band range slider widget.
//!
//! Each draw function takes `&mut UiState` so it can mutate viewer state without
//! borrowing the whole [`App`](crate::app::App).

mod band_slider;
mod controls;
mod sidebar;
mod slice_panel;
pub(crate) mod theme;

use crate::loader::LoadSource;
use crate::state::UiState;
use crate::transfer::ColorBand;

/// Draw the full three-panel UI.
pub(crate) fn draw_ui(ctx: &egui::Context, state: &mut UiState) {
    let focus_before_layout = state.focus_volume;
    if state.focus_volume
        && !state.slice_expanded
        && !egui::Popup::is_any_open(ctx)
        && ctx.memory(|m| m.focused().is_none())
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.focus_volume = false;
        state.fit_camera = true;
    }
    egui::TopBottomPanel::top("app_bar")
        .frame(theme::frame())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::mark(ui, 26.0);
                ui.label(egui::RichText::new("roxel").size(23.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let name = state
                        .library
                        .loaded_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "DICOS / INSPECT".into());
                    ui.add_sized(
                        [ui.available_width().min(480.0), 26.0],
                        egui::Label::new(egui::RichText::new(&name).size(12.0).color(theme::MUTED))
                            .truncate(),
                    )
                    .on_hover_text(name);
                });
            });
        });
    egui::TopBottomPanel::bottom("status_bar")
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(10))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(if state.loading_file.is_some() {
                        "Loading volume…"
                    } else if state.library.volumes.is_empty() {
                        "Ready to open a study"
                    } else if state.focus_volume {
                        "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom  ·  Esc to return"
                    } else {
                        "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom"
                    })
                    .size(12.0)
                    .color(theme::MUTED),
                );
            });
        });
    if !state.focus_volume {
        sidebar::draw_left_sidebar(ctx, state);
    }
    if !state.library.volumes.is_empty() && !state.focus_volume {
        slice_panel::draw_right_panel(ctx, state);
        controls::draw_bottom_controls(ctx, state);
    }
    if state.focus_volume && !state.library.volumes.is_empty() {
        controls::draw_focus_controls(ctx, state);
    }
    egui::TopBottomPanel::top("volume_header")
        .frame(theme::frame())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("3D volume").size(16.0).strong());
                if !state.library.volumes.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(if state.focus_volume {
                                "Exit focus"
                            } else {
                                "Focus volume"
                            })
                            .clicked()
                        {
                            state.focus_volume = !state.focus_volume;
                            state.fit_camera = true;
                        }
                        if state.focus_volume {
                            ui.checkbox(&mut state.show_slice_locator, "Slice plane");
                            ui.add_enabled(
                                !state.slice_view.composite
                                    && state.library.active_3d_index
                                        == Some(state.slice_view.volume_index),
                                egui::Checkbox::new(&mut state.cutaway, "Cutaway"),
                            );
                        }
                        if ui
                            .button("Fit volume")
                            .on_hover_text("Center and fit the complete volume")
                            .clicked()
                        {
                            state.fit_camera = true;
                        }
                    });
                }
            });
        });
    let next_viewport = ctx.available_rect();
    if state.viewport.size() != next_viewport.size() {
        state.fit_camera = true;
    }
    state.viewport = next_viewport;
    if state.focus_volume != focus_before_layout {
        ctx.request_repaint();
    }
    draw_slice_locator(ctx, state);
    if !state.library.volumes.is_empty() {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("orientation"),
        ));
        let origin = state.viewport.left_bottom() + egui::vec2(48.0, -48.0);
        for (axis, label, color) in [
            (glam::Vec3::X, "X", egui::Color32::from_rgb(231, 145, 135)),
            (glam::Vec3::Y, "Y", theme::ACCENT),
            (glam::Vec3::Z, "Z", egui::Color32::from_rgb(143, 177, 225)),
        ] {
            let end = origin
                + egui::vec2(axis.dot(state.camera.right()), axis.dot(state.camera.up())) * 26.0;
            painter.line_segment([origin, end], egui::Stroke::new(1.5_f32, color));
            painter.text(
                end + (end - origin).normalized() * 9.0,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }
    if state.library.volumes.is_empty() {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.32).max(20.0));
                    theme::mark(ui, 64.0);
                    ui.add_space(16.0);
                    ui.heading("A clearer view inside.");
                    ui.label(
                        egui::RichText::new("Open a DICOS study to explore its volume and slices.")
                            .color(theme::MUTED),
                    );
                    ui.add_space(16.0);
                    if ui
                        .add_enabled(
                            state.loading_file.is_none(),
                            egui::Button::new(
                                egui::RichText::new("Open DICOS file…").color(theme::BG),
                            )
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(150.0, 38.0)),
                        )
                        .clicked()
                    {
                        state.actions.file_to_load = rfd::FileDialog::new()
                            .add_filter("DICOS", &["dcs", "dcm"])
                            .pick_file()
                            .map(LoadSource::Path);
                    }
                    ui.label(
                        egui::RichText::new(".dcs and .dcm files  ·  Multi-volume folders")
                            .small()
                            .color(theme::MUTED),
                    );
                });
            });
    }
    slice_panel::draw_expanded(ctx, state);
}

/// Returns true when a slider value is "committed" -- either the drag ended,
/// the value changed without dragging (click / keyboard), or the text field
/// lost focus.  Use this to debounce expensive operations (CPU re-renders)
/// so they only fire once instead of every frame during a drag.
pub(crate) fn slider_committed(r: &egui::Response) -> bool {
    r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged())
}

pub(crate) fn band_color(band: &ColorBand) -> egui::Color32 {
    egui::Color32::from_rgb(band.color[0], band.color[1], band.color[2])
}

pub(crate) fn clamp_band_thresholds(bands: &mut [ColorBand]) {
    if bands.is_empty() {
        return;
    }

    let mut prev = 0u16;
    for band in bands.iter_mut() {
        if band.threshold < prev {
            band.threshold = prev;
        }
        prev = band.threshold;
    }

    if let Some(last) = bands.last_mut() {
        if last.threshold == 0 {
            last.threshold = 1;
        }
    }
}

/// Project the selected voxel-center plane with the same camera model as WGSL.
/// Never imply correspondence between different selected volume layers.
fn draw_slice_locator(ctx: &egui::Context, state: &UiState) {
    if !state.show_slice_locator
        || state.slice_view.composite
        || state.library.active_3d_index != Some(state.slice_view.volume_index)
    {
        return;
    }
    let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) else {
        return;
    };
    let points = state.slice_view.plane_corners(&layer.volume);
    let camera = &state.camera;
    let rect = state.viewport;
    let aspect = rect.width() / rect.height().max(1.0);
    let mut projected = Vec::with_capacity(4);
    for point in points {
        let delta = glam::Vec3::from(point) * state.volume_extent - camera.position();
        let depth = delta.dot(camera.forward());
        if depth <= 0.0 {
            return;
        }
        let uv = egui::vec2(
            delta.dot(camera.right()) / (depth * camera.fov * aspect),
            delta.dot(camera.up()) / (depth * camera.fov),
        );
        projected.push(rect.center() + uv * rect.size() * 0.5);
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("slice_locator"),
        ))
        .with_clip_rect(rect);
    let retained_axis = match state.slice_view.orientation {
        crate::slice_view::Orientation::Axial => "Z",
        crate::slice_view::Orientation::Coronal => "Y",
        crate::slice_view::Orientation::Sagittal => "X",
    };
    let plane_label = if state.cutaway {
        format!(
            "Cut plane · keeping {}{}",
            if state.reverse_cutaway { "−" } else { "+" },
            retained_axis
        )
    } else {
        "Slice plane".into()
    };
    for i in 0..4 {
        painter.line_segment(
            [projected[i], projected[(i + 1) % 4]],
            egui::Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(0.65)),
        );
    }
    let galley =
        painter.layout_no_wrap(plane_label, egui::FontId::proportional(11.0), theme::ACCENT);
    let label_size = galley.size() + egui::vec2(12.0, 8.0);
    let anchor = projected[0];
    let mut label_pos = anchor + egui::vec2(8.0, 8.0);
    label_pos.x = label_pos.x.clamp(
        rect.left() + 8.0,
        (rect.right() - label_size.x - 8.0).max(rect.left() + 8.0),
    );
    label_pos.y = label_pos.y.clamp(
        rect.top() + 8.0,
        (rect.bottom() - label_size.y - 36.0).max(rect.top() + 8.0),
    );
    let label_rect = egui::Rect::from_min_size(label_pos, label_size);
    painter.line_segment(
        [anchor, label_rect.left_center()],
        egui::Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(0.65)),
    );
    painter.rect_filled(label_rect, 4, theme::BG.gamma_multiply(0.96));
    painter.galley(label_pos + egui::vec2(6.0, 4.0), galley, theme::ACCENT);
    painter.text(
        rect.right_bottom() + egui::vec2(-16.0, -20.0),
        egui::Align2::RIGHT_BOTTOM,
        format!(
            "{} slice {} / {}",
            state.slice_view.orientation.label(),
            state.slice_view.slice_index + 1,
            state.slice_view.max_slices
        ),
        egui::FontId::proportional(12.0),
        theme::ACCENT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        state: &mut UiState,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| draw_ui(ctx, state),
        )
    }
    fn text_center(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        shapes
            .iter()
            .find_map(|s| find(&s.shape, text))
            .unwrap_or_else(|| panic!("Missing control {text}"))
    }
    #[test]
    fn expanded_slice_navigation_and_escape_use_real_egui_input() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut state = UiState::new(crate::camera::Camera::default());
        state.slice_expanded = true;
        state.slice_view.max_slices = 4;
        state.slice_view.slice_index = 1;
        for _ in 0..3 {
            frame(&ctx, &mut state, vec![]);
        }
        for (label, expected) in [("Next", 2), ("Previous", 1)] {
            let output = frame(&ctx, &mut state, vec![]);
            let pos = text_center(&output.shapes, label);
            frame(
                &ctx,
                &mut state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                &ctx,
                &mut state,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(state.slice_view.slice_index, expected);
            assert!(state.slice_dirty);
        }
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut state, vec![escape.clone()]);
        assert!(!state.slice_expanded);
        state.focus_volume = true;
        frame(&ctx, &mut state, vec![escape]);
        assert!(!state.focus_volume);
        assert!(state.fit_camera);
    }

    #[test]
    fn long_study_names_leave_a_usable_viewport_at_minimum_size() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut state = UiState::new(crate::camera::Camera::default());
        let name = "long-study-identifier-".repeat(20);
        state.library.loaded_path = Some(std::path::PathBuf::from(&name));
        state.library.volumes.push(crate::app::VolumeLayer {
            name,
            volume: crate::volume::Volume {
                data: vec![0; 8],
                dim_x: 2,
                dim_y: 2,
                dim_z: 2,
                window_center: 100.0,
                window_width: 200.0,
                rescale_intercept: 0.0,
                modality: "CT".into(),
                voxel_spacing: [1.0; 3],
                threats: vec![],
            },
        });
        state.library.active_3d_index = Some(0);
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 720.0),
                    )),
                    ..Default::default()
                },
                |ctx| draw_ui(ctx, &mut state),
            );
        }
        assert!(state.viewport.width() >= 450.0, "{:?}", state.viewport);
        assert!(state.viewport.height() >= 250.0, "{:?}", state.viewport);
        assert!(state.viewport.right() <= 1100.0);
        let workspace_size = state.viewport.size();
        let output = frame(&ctx, &mut state, vec![]);
        let pos = text_center(&output.shapes, "Focus volume");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(state.focus_volume);
        state.fit_camera = false; // Simulate app consuming this frame's fit.
        frame(&ctx, &mut state, vec![]);
        assert!(state.viewport.width() > workspace_size.x);
        assert!(
            state.fit_camera,
            "New focus viewport must request its own fit"
        );
        egui::Popup::open_id(&ctx, egui::Id::new("test_popup"));
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(state.focus_volume, "Escape must give the popup priority");
    }
}
