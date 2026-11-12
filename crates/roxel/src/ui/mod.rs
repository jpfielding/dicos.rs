//! egui UI drawing.
//!
//! The layout adapts to the available logical size (see [`layout`]):
//!
//! - Desktop: three areas, each a view or the study with its own controls.
//!   - [`sidebar`] — left: the study (open, details, volumes, detections).
//!   - [`controls`] — centre: the 3D view's header and its controls shelf.
//!   - [`slice_panel`] — right: the 2D slice and its controls.
//! - Phone portrait / landscape: the same content behind a tab bar ([`phone`]).
//!
//! "Focus volume" and "Focus slice" are one pattern: the view takes the whole
//! workspace in place, keeps its own controls, and "Exit focus" (or Esc)
//! returns.
//!
//! [`band_slider`] is the custom transfer-band range slider widget.
//!
//! Each draw function takes `&mut UiState` so it can mutate viewer state without
//! borrowing the whole [`App`](crate::app::App).

mod band_slider;
mod controls;
#[cfg(test)]
mod fit_tests;
pub(crate) mod layout;
mod phone;
mod sidebar;
mod slice_panel;
pub(crate) mod theme;

use crate::state::UiState;
use crate::transfer::ColorBand;
use layout::Layout;

/// Narrowest the 3D view's column may get before the 2D panel stops growing.
const CENTER_MIN_WIDTH: f32 = 532.0;

/// How to drive the view on screen, in the input the person is using. Shown
/// once, in the desktop status bar.
fn navigation_text(state: &UiState, touch: bool) -> &'static str {
    if state.slice_expanded {
        return if touch {
            "Drag to pan  ·  Pinch to zoom"
        } else {
            "Drag to pan  ·  Ctrl + scroll to zoom  ·  Esc to exit focus"
        };
    }
    match (touch, state.focus_volume) {
        (true, _) => "Drag to orbit  ·  Pinch to zoom  ·  Two fingers to pan",
        (false, false) => "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom",
        (false, true) => {
            "Drag to orbit  ·  Shift + drag to pan  ·  Scroll to zoom  ·  Esc to exit focus"
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
    let focus_before_layout = state.focus_volume;
    // Esc leaves focus, unless a popup or a text field wants the key.
    if (state.focus_volume || state.slice_expanded)
        && !egui::Popup::is_any_open(ctx)
        && ctx.memory(|m| m.focused().is_none())
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        if state.slice_expanded {
            state.slice_expanded = false;
        } else {
            state.focus_volume = false;
        }
        state.fit_camera = true;
    }
    if layout == Layout::Desktop {
        draw_desktop_panels(ctx, state);
    } else {
        phone::draw(ctx, state);
    }
    // The central area left by the panels. The 3D volume renders there
    // whenever `UiState::volume_visible`.
    let next_viewport = ctx.available_rect();
    if state.viewport.size() != next_viewport.size() {
        state.fit_camera = true;
    }
    state.viewport = next_viewport;
    if state.focus_volume != focus_before_layout {
        ctx.request_repaint();
    }
    if state.volume_visible() {
        draw_slice_plane(ctx, state);
        draw_orientation_gizmo(ctx, state);
    } else if state.library.volumes.is_empty() && !state.slice_expanded {
        draw_empty_state(ctx, state);
    }
}

/// App bar: mark, name, and the open study with both ends of its name kept.
pub(super) fn app_bar(ctx: &egui::Context, state: &mut UiState) {
    let compact = state.layout.is_compact();
    let frame = egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(16, if compact { 6 } else { 8 }));
    egui::TopBottomPanel::top("app_bar")
        .frame(frame)
        .show(ctx, |ui| {
            // Set before the row is laid out: the row takes its height from it.
            ui.spacing_mut().interact_size.y = 28.0;
            ui.horizontal(|ui| {
                theme::mark(ui, if compact { 22.0 } else { 24.0 });
                ui.label(
                    egui::RichText::new("roxel")
                        .size(if compact { 19.0 } else { 21.0 })
                        .color(theme::HEADING),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Nothing to name before a study is open: the first-use
                    // card already says so.
                    let Some(path) = state.library.loaded_path.as_ref() else {
                        return;
                    };
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let font = egui::FontId::proportional(13.0);
                    let room = (ui.available_width() - 16.0).clamp(40.0, 520.0);
                    let shown = theme::elide_middle(ui.painter(), &name, font.clone(), room);
                    let label = ui.label(egui::RichText::new(shown).font(font).color(theme::MUTED));
                    label.on_hover_text(path.display().to_string());
                });
            });
        });
}

/// Width of the 2D panel: about a third of the window, so the slice is large,
/// but never at the cost of the 3D view's column.
fn slice_panel_width(window: f32) -> f32 {
    let most = (window - sidebar::SIDEBAR_WIDTH - CENTER_MIN_WIDTH).max(336.0);
    (window * 0.36).clamp(336.0, most.min(520.0))
}

fn draw_desktop_panels(ctx: &egui::Context, state: &mut UiState) {
    app_bar(ctx, state);
    let loaded = !state.library.volumes.is_empty();
    if !loaded && !state.slice_expanded {
        // First use: the centre card is the whole interface.
        return;
    }
    let touch = ctx.input(|i| i.has_touch_screen());
    egui::TopBottomPanel::bottom("status_bar")
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin::symmetric(16, 5)),
        )
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(navigation_text(state, touch))
                    .size(12.0)
                    .color(theme::MUTED),
            );
        });
    if state.slice_expanded {
        slice_panel::draw_focus(ctx, state);
        return;
    }
    if !state.focus_volume {
        sidebar::draw_left_sidebar(ctx, state);
        let width = slice_panel_width(ctx.content_rect().width());
        slice_panel::draw_right_panel(ctx, state, width);
    }
    controls::draw_volume_header(ctx, state);
    controls::draw_shelf(ctx, state);
}

/// X / Y / Z axes in the view's lower-left corner; the "View along" buttons
/// are named for them.
fn draw_orientation_gizmo(ctx: &egui::Context, state: &UiState) {
    let rect = state.viewport;
    if rect.width() < 120.0 || rect.height() < 120.0 {
        return;
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("orientation"),
        ))
        .with_clip_rect(rect);
    let origin = rect.left_bottom() + egui::vec2(40.0, -40.0);
    for (axis, label, color) in [
        (glam::Vec3::X, "X", egui::Color32::from_rgb(231, 145, 135)),
        (glam::Vec3::Y, "Y", theme::ACCENT),
        (glam::Vec3::Z, "Z", egui::Color32::from_rgb(143, 177, 225)),
    ] {
        let direction = egui::vec2(axis.dot(state.camera.right()), axis.dot(state.camera.up()));
        let end = origin + direction * 20.0;
        painter.line_segment([origin, end], egui::Stroke::new(1.5_f32, color));
        // An axis pointing at the viewer has no direction on screen.
        let out = if direction.length() > 0.05 {
            direction.normalized()
        } else {
            egui::vec2(0.0, -1.0)
        };
        // Keep the letters clear of the origin even for a foreshortened axis.
        painter.text(
            origin + out * (direction.length() * 20.0 + 9.0).max(17.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }
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
                        ui.add_space(((ui.available_height() - 190.0) * 0.5).max(4.0));
                        ui.columns(2, |cols| {
                            cols[0].vertical_centered(|ui| empty_intro(ui, true));
                            cols[1].vertical_centered(|ui| {
                                ui.set_max_width(ui.available_width().min(320.0));
                                ui.add_space(30.0);
                                empty_actions(ui, state);
                            });
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            let compact = layout.is_compact();
                            ui.set_max_width(ui.available_width().min(if compact {
                                340.0
                            } else {
                                280.0
                            }));
                            ui.add_space(
                                (ui.available_height() * if compact { 0.2 } else { 0.26 })
                                    .max(16.0),
                            );
                            empty_intro(ui, compact);
                            ui.add_space(20.0);
                            empty_actions(ui, state);
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
            .color(theme::HEADING),
    );
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new("See a CT scan as a 3D volume and as 2D slices.")
                .color(theme::MUTED),
        )
        .wrap(),
    );
}

/// The open actions, load progress and accepted formats.
fn empty_actions(ui: &mut egui::Ui, state: &mut UiState) {
    sidebar::open_buttons(ui, state, true);
    ui.add_space(4.0);
    sidebar::load_status(ui, state);
    ui.add(
        egui::Label::new(
            egui::RichText::new(if cfg!(target_arch = "wasm32") {
                "DICOS .dcs or .dcm files. Add the threat report to see detections."
            } else if state.layout.is_compact() {
                "DICOS .dcs or .dcm files."
            } else {
                "DICOS .dcs or .dcm files, or a study folder."
            })
            .small()
            .color(theme::MUTED),
        )
        .wrap(),
    );
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

/// Outline the selected slice in the 3D view (projected with the same camera
/// model as the WGSL) and name it in the view's corner, off the object.
/// Never imply correspondence between different selected volume layers.
fn draw_slice_plane(ctx: &egui::Context, state: &UiState) {
    if !sidebar::slice_tools_available(state) || !(state.show_slice_locator || state.cutaway) {
        return;
    }
    let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) else {
        return;
    };
    let rect = state.viewport;
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("slice_plane"),
        ))
        .with_clip_rect(rect);
    let stroke = egui::Stroke::new(1.0_f32, theme::ACCENT.gamma_multiply(0.7));
    if state.show_slice_locator {
        let camera = &state.camera;
        let aspect = rect.width() / rect.height().max(1.0);
        let mut projected = Vec::with_capacity(4);
        for point in state.slice_view.plane_corners(&layer.volume) {
            let delta = glam::Vec3::from(point) * state.volume_extent - camera.position();
            let depth = delta.dot(camera.forward());
            if depth <= 0.0 {
                projected.clear();
                break;
            }
            let uv = egui::vec2(
                delta.dot(camera.right()) / (depth * camera.fov * aspect),
                delta.dot(camera.up()) / (depth * camera.fov),
            );
            projected.push(rect.center() + uv * rect.size() * 0.5);
        }
        if projected.len() == 4 {
            for i in 0..4 {
                painter.line_segment([projected[i], projected[(i + 1) % 4]], stroke);
            }
        }
    }
    // One readout in the corner says what the outline (or the cut) is.
    let what = match (state.cutaway, state.reverse_cutaway) {
        (false, _) => "Slice plane",
        (true, false) => "Cutaway",
        (true, true) => "Cutaway, reversed",
    };
    let text = format!(
        "{what}  ·  {} {} / {}",
        state.slice_view.orientation.label(),
        state.slice_view.slice_index + 1,
        state.slice_view.max_slices
    );
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.5), theme::ACCENT);
    let pos = rect.right_bottom() - galley.size() - egui::vec2(16.0, 14.0);
    if state.show_slice_locator {
        // A swatch of the outline's line ties the words to the drawing.
        let y = pos.y + galley.size().y * 0.5;
        painter.line_segment(
            [egui::pos2(pos.x - 26.0, y), egui::pos2(pos.x - 8.0, y)],
            egui::Stroke::new(1.5_f32, theme::ACCENT),
        );
    }
    painter.galley(pos, galley, theme::ACCENT);
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
