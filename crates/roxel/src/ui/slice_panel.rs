//! The 2D slice inspector: the docked panel, its shared content blocks and the
//! expanded modal.

use super::layout::Layout;
use super::slider_committed;
use super::theme;
use crate::slice_view::Orientation;
use crate::state::UiState;

/// Right panel: 2D slice view and its controls.
pub(crate) fn draw_right_panel(ctx: &egui::Context, state: &mut UiState) {
    egui::SidePanel::right("slice_panel")
        .default_width(336.0)
        .frame(theme::frame())
        .min_width(310.0)
        .max_width((ctx.available_rect().width() - 500.0).clamp(310.0, 440.0))
        .show(ctx, |panel| {
            panel.label(egui::RichText::new(slice_title(state)).size(16.0).strong());
            panel.separator();
            // The image stays pinned; only the controls scroll.
            let image_max = (panel.available_height() - 300.0).max(140.0);
            slice_view_block(panel, state, image_max);
            panel.separator();
            egui::ScrollArea::vertical()
                .id_salt("slice_controls")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(panel, |panel| slice_controls(panel, state));
        });
}

fn slice_title(state: &UiState) -> &'static str {
    if state.slice_view.composite {
        "2D projection"
    } else {
        "2D slice"
    }
}

/// The slice image (pan and zoom inside its well), its caption and the two
/// image actions. `max_height` caps the well so controls stay in view.
pub(crate) fn slice_view_block(ui: &mut egui::Ui, state: &mut UiState, max_height: f32) {
    let touch = theme::is_touch(ui);
    let available = ui.available_size();
    if let Some(tex) = &state.slice_texture {
        let tex_size = state
            .library
            .volumes
            .get(state.slice_view.volume_index)
            .map(|layer| state.slice_view.physical_slice_size(&layer.volume))
            .unwrap_or(tex.size_vec2());
        // Hug the image at 100% so the well has no dead bands; zooming pans
        // inside it.
        let fit_h = available.x / tex_size.x.max(1.0) * tex_size.y;
        let viewport_h = (fit_h + 16.0).clamp(120.0, max_height.max(120.0));

        // Base scale: fit-to-width at 100%.
        let base_scale = if tex_size.x > 0.0 {
            (available.x / tex_size.x).min(viewport_h / tex_size.y)
        } else {
            1.0
        };
        let scale = base_scale * (state.slice_view.zoom / 100.0);
        let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);

        egui::Frame::new()
            .fill(theme::BG)
            .corner_radius(6)
            .show(ui, |well| {
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
        let caption = egui::RichText::new(format!(
            "{}  ·  {:.0} × {:.0} mm",
            state.slice_view.orientation.label(),
            tex_size.x,
            tex_size.y
        ))
        .small()
        .color(theme::MUTED);
        if touch {
            // Caption and the one image action share a row to save height.
            ui.horizontal(|ui| {
                ui.label(caption);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Expand image").clicked() {
                        state.slice_expanded = true;
                    }
                });
            });
        } else {
            ui.label(caption);
        }
    } else {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available.x, if touch { 120.0 } else { 140.0 }),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 6, theme::BG);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Preparing slice…",
            egui::FontId::proportional(14.0),
            theme::MUTED,
        );
    }
    if !touch {
        ui.horizontal(|ui| {
            if ui.button("Fit image").clicked() {
                state.slice_view.zoom = 100.0;
            }
            if ui.button("Expand image").clicked() {
                state.slice_expanded = true;
            }
        });
    }
}

/// Plane, slice, volume, windowing and zoom controls for the 2D view.
pub(crate) fn slice_controls(ui: &mut egui::Ui, state: &mut UiState) {
    let (center_range, width_range) = theme::window_ranges(state, state.slice_view.volume_index);
    ui.add_enabled_ui(!state.library.volumes.is_empty(), |ui| {
        // Touch layouts scrub first: the image above answers each change.
        let touch = theme::is_touch(ui);
        if touch {
            slice_scrubber(ui, state);
            plane_picker(ui, state);
        } else {
            plane_picker(ui, state);
            slice_scrubber(ui, state);
        }

        // Volume: a picker only when there is something to pick.
        if state.library.volumes.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Volume").color(theme::MUTED));
                egui::ComboBox::from_id_salt("slice_volume")
                    .width((ui.available_width() - 8.0).clamp(120.0, 260.0))
                    .truncate()
                    .selected_text(
                        state
                            .library
                            .volumes
                            .get(state.slice_view.volume_index)
                            .map(|l| l.name.as_str())
                            .unwrap_or("(none)"),
                    )
                    .show_ui(ui, |combo| {
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
            });
        } else if let Some(layer) = state.library.volumes.first() {
            theme::key_value(ui, "Volume", &layer.name);
        }

        // Composite view toggle.
        let prev_composite = state.slice_view.composite;
        ui.checkbox(&mut state.slice_view.composite, "Composite projection");
        if state.slice_view.composite != prev_composite {
            state.slice_dirty = true;
        }

        theme::section(ui, "Windowing");
        // Window/Level controls (debounced -- only re-render on drag end).
        let center = theme::window_slider(
            ui,
            "Center",
            &mut state.slice_view.window_center,
            center_range,
        );
        let width =
            theme::window_slider(ui, "Width", &mut state.slice_view.window_width, width_range);
        if slider_committed(&center) || slider_committed(&width) {
            state.slice_dirty = true;
        }
        theme::slider(ui, "Zoom %", &mut state.slice_view.zoom, 50.0..=500.0);
        if theme::is_touch(ui) && ui.button("Fit image").clicked() {
            state.slice_view.zoom = 100.0;
        }
    });
}

fn plane_picker(ui: &mut egui::Ui, state: &mut UiState) {
    let prev_orientation = state.slice_view.orientation;
    let selected = Orientation::ALL
        .iter()
        .position(|o| *o == state.slice_view.orientation);
    let labels = Orientation::ALL.map(|o| o.label());
    if let Some(i) = theme::segmented(ui, selected, &labels) {
        state.slice_view.orientation = Orientation::ALL[i];
    }
    if state.slice_view.orientation != prev_orientation {
        // Update max_slices for new orientation.
        if let Some(layer) = state.library.volumes.get(state.slice_view.volume_index) {
            state.slice_view.update_for_volume(&layer.volume);
        }
        state.slice_dirty = true;
    }
}

/// Slice scrubber, hidden in composite mode.
fn slice_scrubber(ui: &mut egui::Ui, state: &mut UiState) {
    if !state.slice_view.composite && state.slice_view.max_slices > 1 {
        let max_slices = state.slice_view.max_slices;
        if slice_row(ui, &mut state.slice_view.slice_index, max_slices) {
            state.slice_dirty = true;
        }
    }
}

/// 1-based editable slice number.
fn slice_number(index: &mut usize, last: usize) -> egui::DragValue<'_> {
    egui::DragValue::new(index)
        .range(0..=last)
        .custom_formatter(|v, _| format!("{}", v as usize + 1))
        .custom_parser(|s| {
            s.parse::<usize>()
                .ok()
                .and_then(|v| v.checked_sub(1))
                .map(|v| v as f64)
        })
}

/// Slice scrubber: label, rail and a 1-based number. Touch layouts add
/// previous/next steppers (a finger cannot land on one slice of hundreds).
/// Returns true when the index changed.
pub(crate) fn slice_row(ui: &mut egui::Ui, index: &mut usize, max_slices: usize) -> bool {
    let last = max_slices.saturating_sub(1);
    let touch = theme::is_touch(ui);
    let mut changed = false;
    if touch {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(48.0, theme::TOUCH_TARGET),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| ui.label(egui::RichText::new("Slice").color(theme::MUTED)),
            );
            let step = egui::vec2(theme::TOUCH_TARGET, theme::TOUCH_TARGET);
            if ui
                .add_enabled(*index > 0, egui::Button::new("−").min_size(step))
                .on_hover_text("Previous slice")
                .clicked()
            {
                *index -= 1;
                changed = true;
            }
            let number_w = 64.0;
            ui.spacing_mut().slider_width =
                (ui.available_width() - step.x - number_w - ui.spacing().item_spacing.x * 2.0)
                    .max(48.0);
            changed |= ui
                .add(egui::Slider::new(index, 0..=last).show_value(false))
                .changed();
            if ui
                .add_enabled(*index < last, egui::Button::new("+").min_size(step))
                .on_hover_text("Next slice")
                .clicked()
            {
                *index += 1;
                changed = true;
            }
            changed |= ui
                .add_sized([number_w, theme::TOUCH_TARGET], slice_number(index, last))
                .changed();
        });
    } else {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(58.0, 26.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| ui.label(egui::RichText::new("Slice").color(theme::MUTED)),
            );
            ui.spacing_mut().interact_size.y = 20.0;
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).clamp(48.0, 240.0);
            changed |= ui
                .add(egui::Slider::new(index, 0..=last).show_value(false))
                .changed();
            changed |= ui
                .add_sized([52.0, 26.0], slice_number(index, last))
                .changed();
        });
    }
    changed
}

/// Compact layouts: the whole inspector as one scrolling column.
pub(crate) fn slice_tab(ui: &mut egui::Ui, state: &mut UiState) {
    let max = if state.layout == Layout::PhoneLandscape {
        170.0
    } else {
        220.0
    };
    slice_view_block(ui, state, max);
    ui.add_space(4.0);
    slice_controls(ui, state);
}

/// A larger inspection surface keeps small-window previews useful without
/// hiding the primary controls from the main workspace. Compact layouts use
/// the whole screen.
pub(crate) fn draw_expanded(ctx: &egui::Context, state: &mut UiState) {
    if !state.slice_expanded {
        return;
    }
    let layout = state.layout;
    let compact = layout.is_compact();
    let screen = ctx.content_rect().size();
    let size = if compact {
        screen - egui::vec2(24.0, 24.0)
    } else {
        screen * 0.86
    };
    let mut close = false;
    let (center_range, width_range) = theme::window_ranges(state, state.slice_view.volume_index);
    let frame = theme::frame()
        .inner_margin(if compact { 12 } else { 16 })
        .corner_radius(if compact { 0 } else { 8 });
    let response = egui::Modal::new(egui::Id::new("expanded_slice"))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(190))
        .show(ctx, |ui| {
            ui.set_width(size.x);
            if compact {
                ui.set_min_height(size.y);
                // Phones may need to scroll: landscape is only ~370 pt tall.
                egui::ScrollArea::vertical()
                    .id_salt("expanded_scroll")
                    .max_height(size.y)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        expanded_content(
                            ui,
                            state,
                            layout,
                            size,
                            center_range,
                            width_range,
                            &mut close,
                        )
                    });
            } else {
                expanded_content(
                    ui,
                    state,
                    layout,
                    size,
                    center_range,
                    width_range,
                    &mut close,
                );
            }
        });
    if close || response.should_close() {
        state.slice_expanded = false;
    }
}

fn expanded_content(
    ui: &mut egui::Ui,
    state: &mut UiState,
    layout: Layout,
    size: egui::Vec2,
    center_range: std::ops::RangeInclusive<f32>,
    width_range: std::ops::RangeInclusive<f32>,
    close: &mut bool,
) {
    let study = state
        .library
        .loaded_path
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = state
        .library
        .volumes
        .get(state.slice_view.volume_index)
        .map(|layer| layer.name.clone())
        .unwrap_or_else(|| "Loading".into());
    let fov = state
        .library
        .volumes
        .get(state.slice_view.volume_index)
        .map(|layer| state.slice_view.physical_slice_size(&layer.volume))
        .map(|mm| format!(" · {:.0} × {:.0} mm", mm.x, mm.y))
        .unwrap_or_default();
    let details = format!(
        "{} / {} / {}{}",
        name,
        state.slice_view.orientation.label(),
        if state.slice_view.composite {
            "Composite projection"
        } else {
            "Cross-section"
        },
        fov
    );

    // Header.
    ui.horizontal(|ui| {
        ui.heading("Slice inspection");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Back to workspace").clicked() {
                *close = true;
            }
            if layout != Layout::PhonePortrait && ui.button("Fit image").clicked() {
                state.expanded_zoom = 100.0;
            }
            if layout == Layout::PhoneLandscape {
                ui.add(
                    egui::Label::new(egui::RichText::new(&details).color(theme::MUTED)).truncate(),
                )
                .on_hover_text(&name);
            }
        });
    });
    match layout {
        Layout::PhoneLandscape => {}
        Layout::PhonePortrait => {
            ui.add(
                egui::Label::new(egui::RichText::new(&study).size(13.0).color(theme::MUTED))
                    .truncate(),
            );
            ui.add(egui::Label::new(egui::RichText::new(&details).color(theme::MUTED)).wrap());
        }
        Layout::Desktop => {
            ui.add(
                egui::Label::new(egui::RichText::new(&study).size(12.0).color(theme::MUTED))
                    .truncate(),
            )
            .on_hover_text(&study);
            ui.add(egui::Label::new(egui::RichText::new(&details).color(theme::MUTED)).truncate())
                .on_hover_text(&name);
        }
    }

    // Image.
    let chrome = match layout {
        Layout::Desktop => 218.0,
        Layout::PhonePortrait => 470.0,
        Layout::PhoneLandscape => 210.0,
    };
    if let Some(texture) = &state.slice_texture {
        let available = egui::vec2(size.x, (size.y - chrome).max(100.0));
        let physical_size = state
            .library
            .volumes
            .get(state.slice_view.volume_index)
            .map(|layer| state.slice_view.physical_slice_size(&layer.volume))
            .unwrap_or(texture.size_vec2());
        let scale = (available.x / physical_size.x).min(available.y / physical_size.y);
        let image_size = physical_size * scale * (state.expanded_zoom / 100.0);
        let fit_h = physical_size.y * scale;
        let well_h = if layout == Layout::Desktop {
            available.y
        } else {
            (fit_h * 2.5).clamp(160.0, available.y.max(160.0))
        };
        let well = egui::vec2(available.x, well_h);
        egui::Frame::new()
            .fill(theme::BG)
            .corner_radius(4)
            .show(ui, |ui| {
                let scroll = egui::ScrollArea::both()
                    .id_salt("expanded_image_pan")
                    .auto_shrink([false, false])
                    .max_height(well.y)
                    .show(ui, |ui| {
                        ui.set_min_size(well);
                        ui.add_space(((well.y - image_size.y) * 0.5).max(0.0));
                        ui.horizontal(|ui| {
                            ui.add_space(((well.x - image_size.x) * 0.5).max(0.0));
                            ui.image(egui::load::SizedTexture::new(texture.id(), image_size))
                                .rect
                        })
                        .inner
                    });
                draw_scale_bar(
                    ui,
                    scroll.inner.intersect(scroll.inner_rect),
                    scroll.inner_rect,
                    scale * state.expanded_zoom / 100.0,
                );
            });
    }

    // Controls.
    if !state.slice_view.composite {
        let max_slices = state.slice_view.max_slices;
        if layout.is_compact() {
            if slice_row(ui, &mut state.slice_view.slice_index, max_slices) {
                state.slice_dirty = true;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label("Slice");
                if ui
                    .add_enabled(
                        state.slice_view.slice_index > 0,
                        egui::Button::new("Previous"),
                    )
                    .clicked()
                {
                    state.slice_view.slice_index -= 1;
                    state.slice_dirty = true;
                }
                ui.spacing_mut().interact_size.y = 20.0;
                ui.spacing_mut().slider_width = (ui.available_width() - 190.0).max(100.0);
                if ui
                    .add(
                        egui::Slider::new(
                            &mut state.slice_view.slice_index,
                            0..=state.slice_view.max_slices.saturating_sub(1),
                        )
                        .custom_formatter(|v, _| format!("{}", v as usize + 1))
                        .custom_parser(|s| {
                            s.parse::<usize>()
                                .ok()
                                .and_then(|v| v.checked_sub(1))
                                .map(|v| v as f64)
                        }),
                    )
                    .changed()
                {
                    state.slice_dirty = true;
                }
                ui.label(format!("/ {}", state.slice_view.max_slices));
                if ui
                    .add_enabled(
                        state.slice_view.slice_index + 1 < state.slice_view.max_slices,
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    state.slice_view.slice_index += 1;
                    state.slice_dirty = true;
                }
            });
        }
    }
    let mut committed = false;
    if layout == Layout::PhonePortrait {
        let c = theme::window_slider(
            ui,
            "Center",
            &mut state.slice_view.window_center,
            center_range,
        );
        let w = theme::window_slider(ui, "Width", &mut state.slice_view.window_width, width_range);
        theme::slider(ui, "Zoom %", &mut state.expanded_zoom, 50.0..=500.0);
        if ui.button("Fit image").clicked() {
            state.expanded_zoom = 100.0;
        }
        committed = slider_committed(&c) || slider_committed(&w);
    } else {
        ui.columns(3, |cols| {
            let center = theme::window_slider(
                &mut cols[0],
                "Center",
                &mut state.slice_view.window_center,
                center_range,
            );
            let width = theme::window_slider(
                &mut cols[1],
                "Width",
                &mut state.slice_view.window_width,
                width_range,
            );
            theme::slider(
                &mut cols[2],
                "Zoom %",
                &mut state.expanded_zoom,
                50.0..=500.0,
            );
            committed = slider_committed(&center) || slider_committed(&width);
        });
    }
    if committed {
        state.slice_dirty = true;
    }
    if layout != Layout::PhoneLandscape {
        ui.add(
            egui::Label::new(
                egui::RichText::new(if layout.is_compact() {
                    "Drag the image to pan · Use Zoom for detail · Tap a number to edit it"
                } else {
                    "Drag image to pan · Zoom for detail · Select numbers to edit · Esc returns to workspace"
                })
                .small()
                .color(theme::MUTED),
            )
            .wrap(),
        );
    }
}

/// A fixed viewport overlay whose physical length follows image zoom.
fn draw_scale_bar(ui: &egui::Ui, image_rect: egui::Rect, rect: egui::Rect, pixels_per_mm: f32) {
    if !pixels_per_mm.is_finite() || pixels_per_mm <= 0.0 {
        return;
    }
    let target_mm = 100.0 / pixels_per_mm;
    let base = 10.0_f32.powf(target_mm.log10().floor());
    let mm = [5.0, 2.0, 1.0]
        .into_iter()
        .find(|n| base * n <= target_mm)
        .unwrap_or(1.0)
        * base;
    let bar_width = mm * pixels_per_mm;
    let outside = image_rect.left() - rect.left() >= bar_width + 32.0;
    let start = egui::pos2(
        if outside {
            image_rect.left() - bar_width - 16.0
        } else {
            image_rect.left() + 20.0
        },
        image_rect.bottom() - 20.0,
    );
    let end = start + egui::vec2(mm * pixels_per_mm, 0.0);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(
        egui::Rect::from_min_max(start - egui::vec2(8.0, 27.0), end + egui::vec2(8.0, 8.0)),
        4.0,
        egui::Color32::from_black_alpha(210),
    );
    let stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);
    painter.line_segment([start, end], stroke);
    for pos in [start, end] {
        painter.line_segment(
            [pos - egui::vec2(0.0, 3.0), pos + egui::vec2(0.0, 3.0)],
            stroke,
        );
    }
    painter.text(
        start - egui::vec2(0.0, 7.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{mm:.1} mm"),
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
}
