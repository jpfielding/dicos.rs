//! egui UI drawing.
//!
//! The layout adapts to the available logical size (see [`layout`]):
//!
//! - Desktop: three panels, one per sibling module, plus a controls shelf.
//!   - [`sidebar`] — left sidebar (library, details, detections, camera).
//!   - [`slice_panel`] — right panel (2D slice view and its controls).
//!   - [`controls`] — bottom shelf (3D rendering controls).
//! - Phone portrait / landscape: the same content as tabs ([`phone`]).
//!
//! [`band_slider`] is the custom transfer-band range slider widget.
//!
//! Each draw function takes `&mut UiState` so it can mutate viewer state without
//! borrowing the whole [`App`](crate::app::App).

mod band_slider;
mod controls;
pub(crate) mod layout;
mod phone;
mod sidebar;
mod slice_panel;
pub(crate) mod theme;

use crate::state::UiState;
use crate::transfer::ColorBand;
use layout::Layout;

/// How to drive the 3D view, in the input the person is using.
fn navigation_text(touch: bool, focus: bool) -> &'static str {
    match (touch, focus) {
        (true, _) => "Drag to orbit  ·  Pinch to zoom  ·  Two fingers to pan",
        (false, false) => "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom",
        (false, true) => {
            "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom  ·  Esc to return"
        }
    }
}

/// Draw the full UI for the current window size.
pub(crate) fn draw_ui(ctx: &egui::Context, state: &mut UiState) {
    let layout = Layout::from_size(ctx.content_rect().size());
    if layout != state.layout {
        theme::set_layout(ctx, layout);
        state.layout = layout;
    }
    let touch_copy = layout.is_compact() || ctx.input(|i| i.has_touch_screen());
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
    if layout == Layout::Desktop {
        draw_desktop_panels(ctx, state, touch_copy);
    } else {
        phone::draw(ctx, state);
    }
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
        draw_orientation_gizmo(ctx, state);
        if layout.is_compact() {
            draw_viewport_status(ctx, state, touch_copy);
        }
    } else {
        draw_empty_state(ctx, state);
    }
    slice_panel::draw_expanded(ctx, state);
}

/// App bar: mark, name, the open study. Landscape phones also host the view
/// actions here to keep the volume tall.
pub(super) fn app_bar(ctx: &egui::Context, state: &mut UiState) {
    let layout = state.layout;
    let compact = layout.is_compact();
    let loaded = !state.library.volumes.is_empty();
    let frame = if layout == Layout::Desktop {
        theme::frame()
    } else {
        theme::compact_frame().inner_margin(egui::Margin::symmetric(
            16,
            if layout == Layout::PhoneLandscape {
                4
            } else {
                8
            },
        ))
    };
    egui::TopBottomPanel::top("app_bar")
        .frame(frame)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::mark(ui, if compact { 24.0 } else { 26.0 });
                ui.label(
                    egui::RichText::new("roxel")
                        .size(if compact { 20.0 } else { 23.0 })
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if layout == Layout::PhoneLandscape && loaded {
                        view_actions(ui, state);
                    }
                    let name = state
                        .library
                        .loaded_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "No study open".into());
                    if compact {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&name).size(13.0).color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    } else {
                        ui.add_sized(
                            [ui.available_width().min(480.0), 26.0],
                            egui::Label::new(
                                egui::RichText::new(&name).size(12.0).color(theme::MUTED),
                            )
                            .truncate(),
                        )
                        .on_hover_text(name);
                    }
                });
            });
        });
}

/// Fit and focus actions for compact layouts, laid out right to left.
pub(super) fn view_actions(ui: &mut egui::Ui, state: &mut UiState) {
    let label = if state.focus_volume {
        "Exit focus"
    } else {
        "Focus volume"
    };
    if ui.button(label).clicked() {
        state.focus_volume = !state.focus_volume;
        state.fit_camera = true;
    }
    if ui.button("Fit volume").clicked() {
        state.fit_camera = true;
    }
}

fn draw_desktop_panels(ctx: &egui::Context, state: &mut UiState, touch_copy: bool) {
    app_bar(ctx, state);
    egui::TopBottomPanel::bottom("status_bar")
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(10))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(if state.loading_file.is_some() {
                        "Loading volume…"
                    } else if state.library.volumes.is_empty() {
                        "Ready to open a study"
                    } else {
                        navigation_text(touch_copy, state.focus_volume)
                    })
                    .size(12.0)
                    .color(theme::MUTED),
                );
            });
        });
    if !state.focus_volume {
        sidebar::draw_left_sidebar(ctx, state);
    }
    if state.library.volumes.is_empty() {
        return;
    }
    if state.focus_volume {
        controls::draw_focus_controls(ctx, state);
    } else {
        slice_panel::draw_right_panel(ctx, state);
        controls::draw_bottom_controls(ctx, state);
    }
    egui::TopBottomPanel::top("volume_header")
        .frame(theme::frame())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("3D volume").size(16.0).strong());
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
                            sidebar::slice_tools_available(state),
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
            });
        });
}

/// X / Y / Z axes in the viewport's lower-left corner.
fn draw_orientation_gizmo(ctx: &egui::Context, state: &UiState) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("orientation"),
    ));
    let origin = state.viewport.left_bottom() + egui::vec2(44.0, -44.0);
    for (axis, label, color) in [
        (glam::Vec3::X, "X", egui::Color32::from_rgb(231, 145, 135)),
        (glam::Vec3::Y, "Y", theme::ACCENT),
        (glam::Vec3::Z, "Z", egui::Color32::from_rgb(143, 177, 225)),
    ] {
        let end =
            origin + egui::vec2(axis.dot(state.camera.right()), axis.dot(state.camera.up())) * 26.0;
        painter.line_segment([origin, end], egui::Stroke::new(1.5_f32, color));
        painter.text(
            end + (end - origin).normalized() * 10.0,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }
}

/// Compact layouts have no status bar: one chip at the top of the volume
/// carries the gesture hint, load progress or the last load error.
fn draw_viewport_status(ctx: &egui::Context, state: &UiState, touch_copy: bool) {
    let (text, color) = if state.load_error.is_some() {
        (
            "Could not open that file. Details are under Study.",
            theme::DANGER,
        )
    } else if state.loading_file.is_some() {
        ("Loading volume…", theme::MUTED)
    } else {
        (
            navigation_text(touch_copy, state.focus_volume),
            theme::MUTED,
        )
    };
    let rect = state.viewport;
    if rect.width() < 120.0 {
        return;
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("viewport_status"),
        ))
        .with_clip_rect(rect);
    theme::overlay_chip(
        &painter,
        egui::Align2::CENTER_TOP,
        rect.center_top() + egui::vec2(0.0, 10.0),
        text,
        color,
        12.0,
    );
}

/// First-use state: say what this is and offer the one action.
fn draw_empty_state(ctx: &egui::Context, state: &mut UiState) {
    let layout = state.layout;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(24))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if layout == Layout::PhoneLandscape {
                        // Too short to stack: message left, actions right.
                        ui.add_space(((ui.available_height() - 200.0) * 0.5).max(4.0));
                        ui.columns(2, |cols| {
                            cols[0].vertical_centered(|ui| empty_intro(ui, true));
                            cols[1].vertical_centered(|ui| empty_actions(ui, state, true));
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            let compact = layout.is_compact();
                            ui.set_max_width(ui.available_width().min(360.0));
                            ui.add_space(
                                (ui.available_height() * if compact { 0.2 } else { 0.28 })
                                    .max(16.0),
                            );
                            empty_intro(ui, compact);
                            ui.add_space(20.0);
                            empty_actions(ui, state, compact);
                        });
                    }
                });
        });
}

fn empty_intro(ui: &mut egui::Ui, compact: bool) {
    theme::mark(ui, if compact { 56.0 } else { 64.0 });
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new("Open a DICOS study")
            .size(22.0)
            .strong(),
    );
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(
                "Explore a scan as a 3D volume and as 2D slices, with detections marked.",
            )
            .color(theme::MUTED),
        )
        .wrap(),
    );
}

/// The open actions, load progress and accepted formats.
fn empty_actions(ui: &mut egui::Ui, state: &mut UiState, compact: bool) {
    let size = egui::vec2(
        if compact {
            ui.available_width().min(360.0)
        } else {
            180.0
        },
        if compact { 52.0 } else { 40.0 },
    );
    let idle = state.loading_file.is_none();
    let open_file = ui
        .add_enabled_ui(idle, |ui| {
            theme::primary_button(ui, "Open DICOS file…", size).clicked()
        })
        .inner;
    #[cfg(not(target_arch = "wasm32"))]
    {
        if open_file {
            state.actions.file_to_load = rfd::FileDialog::new()
                .add_filter("DICOS", &["dcs", "dcm"])
                .pick_file()
                .map(crate::loader::LoadSource::Path);
        }
        if compact {
            let open_folder = ui
                .add_enabled_ui(idle, |ui| {
                    ui.add_sized(size, egui::Button::new("Open folder…"))
                        .clicked()
                })
                .inner;
            if open_folder {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    state.actions.file_to_load = Some(crate::loader::LoadSource::Path(path));
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if open_file {
            crate::web::pick_files();
        }
        let sample = ui
            .add_enabled_ui(idle, |ui| {
                ui.add_sized(size, egui::Button::new("Load sample"))
                    .clicked()
            })
            .inner;
        if sample {
            crate::web::load_sample();
        }
    }
    ui.add_space(8.0);
    sidebar::load_status(ui, state);
    ui.add(
        egui::Label::new(
            egui::RichText::new(if cfg!(target_arch = "wasm32") {
                ".dcs and .dcm files  ·  Select a CT with its threat report"
            } else {
                ".dcs and .dcm files  ·  Multi-volume folders"
            })
            .small()
            .color(theme::MUTED),
        )
        .wrap(),
    );
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
        painter.layout_no_wrap(plane_label, egui::FontId::proportional(12.0), theme::ACCENT);
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
        egui::FontId::proportional(12.5),
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
    fn phone_frame(
        ctx: &egui::Context,
        state: &mut UiState,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| draw_ui(ctx, state),
        )
    }

    fn loaded_state() -> UiState {
        let mut state = UiState::new(crate::camera::Camera::default());
        state.library.loaded_path = Some(std::path::PathBuf::from("study"));
        state.library.volumes.push(crate::app::VolumeLayer {
            name: "ct".into(),
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
        state
    }

    fn tap(ctx: &egui::Context, state: &mut UiState, size: egui::Vec2, label: &str) {
        let output = phone_frame(ctx, state, size, vec![]);
        let pos = text_center(&output.shapes, label);
        for pressed in [true, false] {
            phone_frame(
                ctx,
                state,
                size,
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
    }

    #[test]
    fn phone_layouts_leave_the_volume_room_and_tabs_switch() {
        for (size, layout) in [
            (egui::vec2(393.0, 852.0), layout::Layout::PhonePortrait),
            (egui::vec2(852.0, 393.0), layout::Layout::PhoneLandscape),
        ] {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let mut state = loaded_state();
            for _ in 0..3 {
                phone_frame(&ctx, &mut state, size, vec![]);
            }
            assert_eq!(state.layout, layout);
            assert!(
                state.viewport.right() <= size.x + 0.5,
                "{:?}",
                state.viewport
            );
            assert!(
                state.viewport.bottom() <= size.y + 0.5,
                "{:?}",
                state.viewport
            );
            assert!(
                state.viewport.width() >= 300.0,
                "{layout:?} {:?}",
                state.viewport
            );
            assert!(
                state.viewport.height() >= 240.0,
                "{layout:?} {:?}",
                state.viewport
            );

            // Every tab draws, and tapping a tab selects it.
            for (label, tab) in [
                ("Study", layout::PhoneTab::Study),
                ("Detections", layout::PhoneTab::Detections),
                ("3D view", layout::PhoneTab::View3d),
                ("2D slice", layout::PhoneTab::Slice),
            ] {
                tap(&ctx, &mut state, size, label);
                assert_eq!(state.phone_tab, tab, "{layout:?}");
                for _ in 0..2 {
                    phone_frame(&ctx, &mut state, size, vec![]);
                }
            }
        }
    }

    #[test]
    fn portrait_tab_bar_collapses_and_reopens_the_sheet() {
        let size = egui::vec2(393.0, 852.0);
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut state = loaded_state();
        for _ in 0..3 {
            phone_frame(&ctx, &mut state, size, vec![]);
        }
        let with_sheet = state.viewport.height();
        tap(&ctx, &mut state, size, "2D slice");
        for _ in 0..2 {
            phone_frame(&ctx, &mut state, size, vec![]);
        }
        assert!(
            !state.sheet_open,
            "tapping the open tab collapses the sheet"
        );
        assert!(state.viewport.height() > with_sheet + 150.0);
        tap(&ctx, &mut state, size, "3D view");
        assert!(state.sheet_open);
        assert_eq!(state.phone_tab, layout::PhoneTab::View3d);
    }

    #[test]
    fn phone_focus_and_expanded_views_draw() {
        for size in [egui::vec2(393.0, 852.0), egui::vec2(852.0, 393.0)] {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let mut state = loaded_state();
            state.focus_volume = true;
            state.cutaway = true;
            for _ in 0..3 {
                phone_frame(&ctx, &mut state, size, vec![]);
            }
            assert!(state.viewport.height() >= 240.0, "{:?}", state.viewport);
            tap(&ctx, &mut state, size, "Adjust display");
            assert!(state.focus_adjust_open);
            state.focus_volume = false;
            state.slice_expanded = true;
            for _ in 0..3 {
                phone_frame(&ctx, &mut state, size, vec![]);
            }
        }
    }
}
