//! Compact (phone-sized) layouts.
//!
//! - Portrait: the volume on top; a tab bar at the bottom opens a sheet above
//!   it (primary controls sit in thumb reach, tapping the active tab collapses
//!   the sheet to give the volume the whole screen).
//! - Landscape: the volume on the left, a tabbed panel on the right.
//!
//! Both show the same four areas as the desktop's three panels, so every
//! feature stays reachable: Study (library, details, volumes), Detections,
//! 2D slice and 3D view.

use super::layout::{Layout, PhoneTab};
use super::{controls, sidebar, slice_panel, theme};
use crate::state::UiState;

const TAB_BAR_HEIGHT: f32 = 56.0;

pub(crate) fn draw(ctx: &egui::Context, state: &mut UiState) {
    let loaded = !state.library.volumes.is_empty();
    super::app_bar(ctx, state);
    if !loaded {
        return;
    }
    let touch_copy = true;
    let screen = ctx.content_rect().size();
    match (state.layout, state.focus_volume) {
        (Layout::PhonePortrait, false) => {
            tab_bar_panel(ctx, state);
            if state.sheet_open {
                let height = (screen.y * 0.44).clamp(220.0, 420.0);
                egui::TopBottomPanel::bottom("phone_sheet")
                    .exact_height(height)
                    .frame(theme::compact_frame())
                    .show(ctx, |ui| {
                        grabber(ui, state);
                        egui::ScrollArea::vertical()
                            .id_salt(("phone_sheet", state.phone_tab as u8))
                            .auto_shrink([false, false])
                            .show(ui, |ui| tab_content(ui, state, touch_copy));
                    });
            }
            volume_toolbar(ctx, state);
        }
        (Layout::PhoneLandscape, false) => {
            let width = (screen.x * 0.44).clamp(320.0, 400.0);
            egui::SidePanel::right("phone_side")
                .exact_width(width)
                .frame(theme::compact_frame())
                .show(ctx, |ui| {
                    tab_strip(ui, state);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("phone_side", state.phone_tab as u8))
                        .auto_shrink([false, false])
                        .show(ui, |ui| tab_content(ui, state, touch_copy));
                });
        }
        (_, true) => focus_panels(ctx, state),
        // Desktop never reaches here.
        (Layout::Desktop, false) => {}
    }
}

fn tab_content(ui: &mut egui::Ui, state: &mut UiState, touch_copy: bool) {
    match state.phone_tab {
        PhoneTab::Study => {
            sidebar::library_section(ui, state);
            if !state.library.volumes.is_empty() {
                ui.separator();
                sidebar::details_section(ui, state);
                if state.library.volumes.len() > 1 {
                    sidebar::volumes_section(ui, state);
                }
            }
        }
        PhoneTab::Detections => sidebar::detections_section(ui, state),
        PhoneTab::Slice => slice_panel::slice_tab(ui, state),
        PhoneTab::View3d => controls::view_tab(ui, state, touch_copy),
    }
}

/// "3D volume" heading with Fit / Focus, above the volume in portrait.
fn volume_toolbar(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::top("volume_header")
        .frame(theme::compact_frame())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("3D volume").size(16.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    super::view_actions(ui, state);
                });
            });
        });
}

/// Bottom tab bar (portrait). Tapping the open tab collapses the sheet.
fn tab_bar_panel(ctx: &egui::Context, state: &mut UiState) {
    egui::TopBottomPanel::bottom("phone_tabs")
        .exact_height(TAB_BAR_HEIGHT + 12.0)
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 0,
                    // Clear of the home indicator.
                    bottom: 12,
                }),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let open = state.sheet_open;
            if let Some(tab) = tab_row(ui, state, TabEdge::Top, open, TAB_BAR_HEIGHT) {
                if open && tab == state.phone_tab {
                    state.sheet_open = false;
                } else {
                    state.phone_tab = tab;
                    state.sheet_open = true;
                }
            }
        });
}

/// Tabs across the top of the landscape side panel.
fn tab_strip(ui: &mut egui::Ui, state: &mut UiState) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if let Some(tab) = tab_row(ui, state, TabEdge::Bottom, true, 44.0) {
            state.phone_tab = tab;
        }
    });
}

#[derive(Clone, Copy)]
enum TabEdge {
    /// Indicator on top (bottom bar).
    Top,
    /// Indicator underneath (top strip).
    Bottom,
}

/// Draw the four tabs as equal full-height touch targets; returns the tab
/// tapped this frame.
fn tab_row(
    ui: &mut egui::Ui,
    state: &UiState,
    edge: TabEdge,
    active_visible: bool,
    height: f32,
) -> Option<PhoneTab> {
    let detections = sidebar::detection_count(state);
    let mut tapped = None;
    let width = ui.available_width() / PhoneTab::ALL.len() as f32;
    ui.horizontal(|ui| {
        for tab in PhoneTab::ALL {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
            let active = active_visible && tab == state.phone_tab;
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    active,
                    tab.label(),
                )
            });
            if response.clicked() {
                tapped = Some(tab);
            }
            let painter = ui.painter();
            if active {
                painter.rect_filled(rect.shrink2(egui::vec2(2.0, 4.0)), 8, theme::PANEL);
                let bar = match edge {
                    TabEdge::Top => egui::Rect::from_min_size(
                        rect.min + egui::vec2(14.0, 2.0),
                        egui::vec2(rect.width() - 28.0, 3.0),
                    ),
                    TabEdge::Bottom => egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 8.0, rect.bottom() - 4.0),
                        egui::vec2(rect.width() - 16.0, 3.0),
                    ),
                };
                painter.rect_filled(bar, 2, theme::ACCENT);
            } else if response.is_pointer_button_down_on() {
                painter.rect_filled(rect.shrink2(egui::vec2(2.0, 4.0)), 8, theme::PANEL);
            }
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tab.label(),
                egui::FontId::proportional(14.5),
                if active {
                    theme::ACCENT
                } else {
                    egui::Color32::from_rgb(222, 231, 239)
                },
            );
            if tab == PhoneTab::Detections && detections > 0 {
                let center = egui::pos2(rect.right() - 10.0, rect.top() + 14.0);
                painter.circle_filled(center, 9.0, theme::DANGER);
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    detections.to_string(),
                    egui::FontId::proportional(12.0),
                    theme::BG,
                );
            }
        }
    });
    tapped
}

/// A small pill at the top of the sheet; tapping it collapses the sheet.
fn grabber(ui: &mut egui::Ui, state: &mut UiState) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Collapse panel")
    });
    ui.painter().rect_filled(
        egui::Rect::from_center_size(rect.center(), egui::vec2(40.0, 4.0)),
        2,
        theme::OUTLINE,
    );
    if response.clicked() {
        state.sheet_open = false;
    }
}

/// Focus mode on a compact layout: a bar with the slice-plane tools and an
/// "Adjust" toggle for the display sheet above it.
fn focus_panels(ctx: &egui::Context, state: &mut UiState) {
    let screen = ctx.content_rect().size();
    egui::TopBottomPanel::bottom("focus_bar")
        .frame(theme::compact_frame())
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut state.show_slice_locator, "Slice plane");
                ui.add_enabled(
                    sidebar::slice_tools_available(state),
                    egui::Checkbox::new(&mut state.cutaway, "Cutaway"),
                );
                let label = if state.focus_adjust_open {
                    "Hide adjustments"
                } else {
                    "Adjust display"
                };
                if ui
                    .add(egui::Button::new(label).selected(state.focus_adjust_open))
                    .clicked()
                {
                    state.focus_adjust_open = !state.focus_adjust_open;
                }
            });
        });
    if state.focus_adjust_open {
        let height = (screen.y * 0.42).clamp(190.0, 380.0);
        egui::TopBottomPanel::bottom("focus_adjust")
            .exact_height(height)
            .frame(theme::compact_frame())
            .show(ctx, |ui| {
                grabber_focus(ui, state);
                egui::ScrollArea::vertical()
                    .id_salt("focus_adjust_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| controls::focus_adjust_tab(ui, state));
            });
    }
    if state.layout == Layout::PhonePortrait {
        volume_toolbar(ctx, state);
    }
}

fn grabber_focus(ui: &mut egui::Ui, state: &mut UiState) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Hide adjustments")
    });
    ui.painter().rect_filled(
        egui::Rect::from_center_size(rect.center(), egui::vec2(40.0, 4.0)),
        2,
        theme::OUTLINE,
    );
    if response.clicked() {
        state.focus_adjust_open = false;
    }
}
