//! Compact (phone-sized) layouts.
//!
//! A bottom tab bar holds the four destinations in both orientations, so
//! navigation never moves on rotation. The two views (3D view, 2D slice) each
//! take the main area when selected, with their controls in a sheet: under the
//! view in portrait, beside it in landscape. Study and Detections are pages.
//!
//! Tapping the selected view's tab hides or shows its sheet (the chevron on
//! the tab says which). Sheets are paged rather than scrolled, so a slider
//! drag is never taken for a scroll.
//!
//! "Focus volume" / "Focus slice" drop the app bar and tab bar and leave one
//! action bar at the bottom, in thumb reach.

use super::layout::{Layout, PhoneTab};
use super::{controls, sidebar, slice_panel, theme};
use crate::state::UiState;

/// Tab targets: 49 pt tall, the height phone tab bars use.
const TAB_ROW: f32 = 49.0;
/// Four touch rows and the gaps between them.
const SHEET_BODY: f32 = 4.0 * theme::TOUCH_TARGET + 3.0 * 8.0;
/// Page selector, body and the sheet's own margins.
const SHEET_HEIGHT: f32 = 8.0 + theme::TOUCH_TARGET + 8.0 + SHEET_BODY + 8.0;
/// Width of the controls panel beside the view in landscape.
const LANDSCAPE_PANEL: f32 = 400.0;

pub(crate) fn draw(ctx: &egui::Context, state: &mut UiState) {
    let loaded = !state.library.volumes.is_empty();
    if state.slice_expanded || (state.focus_volume && loaded) {
        focus_view(ctx, state);
    } else if loaded {
        tab_view(ctx, state);
    } else {
        super::app_bar(ctx, state);
    }
}

fn tab_view(ctx: &egui::Context, state: &mut UiState) {
    // Drawn first so the bar stays put whatever the page above it does.
    tab_bar(ctx, state);
    super::app_bar(ctx, state);
    match state.phone_tab {
        PhoneTab::Study => page(ctx, "study_page", |ui| study_page(ui, state)),
        PhoneTab::Detections => page(ctx, "detections_page", |ui| detections_page(ui, state)),
        PhoneTab::Slice => {
            if state.sheet_open {
                sheet(ctx, state, slice_panel::slice_sheet);
            }
            slice_area(ctx, state);
        }
        PhoneTab::View3d => {
            if state.sheet_open {
                sheet(ctx, state, controls::volume_sheet);
            }
            // The 3D view renders in the area left over.
        }
    }
}

/// A full page of reading content. Pages hold no sliders, so they may scroll.
fn page(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(16))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, add);
        });
}

fn study_page(ui: &mut egui::Ui, state: &mut UiState) {
    let many = state.library.volumes.len() > 1;
    if state.layout == Layout::PhoneLandscape {
        // Wide and short: the study on the left, its facts on the right.
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.columns(2, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().item_spacing.x = 10.0;
            }
            sidebar::study_section(&mut cols[0], state);
            sidebar::details_section(&mut cols[1], state);
            if many {
                theme::rule(&mut cols[1]);
                sidebar::volumes_section(&mut cols[1], state);
            }
        });
    } else {
        sidebar::study_section(ui, state);
        theme::rule(ui);
        sidebar::details_section(ui, state);
        if many {
            theme::rule(ui);
            sidebar::volumes_section(ui, state);
        }
    }
}

fn detections_page(ui: &mut egui::Ui, state: &mut UiState) {
    if state.layout == Layout::PhoneLandscape {
        // Wide and short: the controls stay put on the left while the list
        // scrolls on the right.
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.columns(2, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().item_spacing.x = 10.0;
            }
            if sidebar::detections_summary(&mut cols[0], state) {
                sidebar::detections_list(&mut cols[1], state);
            }
        });
    } else {
        sidebar::detections_section(ui, state);
    }
}

/// The 2D slice in the main area, edge to edge.
fn slice_area(ctx: &egui::Context, state: &mut UiState) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::WELL))
        .show(ctx, |ui| {
            let size = ui.available_size();
            // Focus makes the slice as large as the screen allows.
            let turn_to_fit = state.slice_expanded;
            slice_panel::image_well(ui, state, size, turn_to_fit);
        });
}

/// The controls of the selected view: below it in portrait, beside it in
/// landscape. Its size is fixed, so changing page never resizes the view.
fn sheet(ctx: &egui::Context, state: &mut UiState, add: impl FnOnce(&mut egui::Ui, &mut UiState)) {
    let frame = egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(16, 8));
    if state.layout == Layout::PhoneLandscape {
        egui::SidePanel::right("phone_side")
            .exact_width(LANDSCAPE_PANEL)
            .resizable(false)
            .frame(frame)
            .show(ctx, |ui| add(ui, state));
    } else {
        egui::TopBottomPanel::bottom("phone_sheet")
            .exact_height(SHEET_HEIGHT)
            .frame(frame)
            .show(ctx, |ui| add(ui, state));
    }
}

/// Bottom tab bar, clear of the home indicator. Tapping another tab opens it
/// with its controls; tapping the selected view's tab hides or shows them.
fn tab_bar(ctx: &egui::Context, state: &mut UiState) {
    let inset = state.layout.home_indicator_inset();
    egui::TopBottomPanel::bottom("phone_tabs")
        .exact_height(TAB_ROW + inset)
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 0,
                    bottom: inset as i8,
                }),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if let Some(tab) = tab_row(ui, state) {
                if tab != state.phone_tab {
                    state.phone_tab = tab;
                    state.sheet_open = true;
                } else if tab.is_view() {
                    state.sheet_open = !state.sheet_open;
                }
            }
        });
}

/// Draw the four tabs as equal full-height touch targets; returns the tab
/// tapped this frame.
fn tab_row(ui: &mut egui::Ui, state: &UiState) -> Option<PhoneTab> {
    let detections = sidebar::detection_count(state);
    let mut tapped = None;
    let width = (ui.available_width() / PhoneTab::ALL.len() as f32).floor();
    ui.horizontal(|ui| {
        for tab in PhoneTab::ALL {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, TAB_ROW), egui::Sense::click());
            let active = tab == state.phone_tab;
            let toggles = active && tab.is_view();
            response.widget_info(|| {
                let name = if !toggles {
                    tab.label().to_owned()
                } else if state.sheet_open {
                    format!("{}, hide controls", tab.label())
                } else {
                    format!("{}, show controls", tab.label())
                };
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, name)
            });
            if response.clicked() {
                tapped = Some(tab);
            }
            let painter = ui.painter();
            if active || response.is_pointer_button_down_on() {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        rect.min + egui::vec2(3.0, 2.0),
                        rect.max - egui::vec2(3.0, 4.0),
                    ),
                    8,
                    theme::PANEL,
                );
            }
            if active {
                // Selection is a bar and a tint as well as a colour.
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 14.0, rect.top() + 2.0),
                        egui::vec2(rect.width() - 28.0, 3.0),
                    ),
                    2,
                    theme::ACCENT,
                );
            }
            let color = if active { theme::ACCENT } else { theme::TEXT };
            let galley = painter.layout_no_wrap(
                tab.label().to_owned(),
                egui::FontId::proportional(14.0),
                color,
            );
            // A view's tab carries a chevron: down hides its controls, up
            // brings them back.
            let extra = if toggles { 13.0 } else { 0.0 };
            let left = rect.center().x - (galley.size().x + extra) * 0.5;
            let text_right = left + galley.size().x;
            painter.galley(
                egui::pos2(left, rect.center().y - galley.size().y * 0.5),
                galley,
                color,
            );
            if toggles {
                let c = egui::pos2(text_right + 9.0, rect.center().y + 1.0);
                let dy = if state.sheet_open { 2.5 } else { -2.5 };
                painter.add(egui::Shape::line(
                    vec![
                        c + egui::vec2(-4.0, -dy),
                        c + egui::vec2(0.0, dy),
                        c + egui::vec2(4.0, -dy),
                    ],
                    egui::Stroke::new(1.6_f32, color),
                ));
            }
            if tab == PhoneTab::Detections && detections > 0 {
                let center = egui::pos2(rect.right() - 9.0, rect.top() + 13.0);
                painter.circle_filled(center, 8.0, theme::DANGER);
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    detections.to_string(),
                    egui::FontId::proportional(11.0),
                    theme::BG,
                );
            }
        }
    });
    tapped
}

/// Focus on a compact layout: the view fills the screen above one action bar.
/// "Adjust display" opens the same paged controls the tab view uses.
fn focus_view(ctx: &egui::Context, state: &mut UiState) {
    let slice = state.slice_expanded;
    let inset = state.layout.home_indicator_inset();
    egui::TopBottomPanel::bottom("focus_bar")
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 8,
                    bottom: 8 + inset as i8,
                }),
        )
        .show(ctx, |ui| {
            let gap = ui.spacing().item_spacing.x;
            let width = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
            let size = [width, theme::TOUCH_TARGET];
            ui.horizontal(|ui| {
                if ui
                    .add_sized(size, egui::Button::new("Exit focus"))
                    .clicked()
                {
                    state.slice_expanded = false;
                    state.focus_volume = false;
                    state.focus_adjust_open = false;
                    state.fit_camera = true;
                }
                let fit = if slice { "Fit image" } else { "Fit view" };
                if ui.add_sized(size, egui::Button::new(fit)).clicked() {
                    if slice {
                        state.slice_view.zoom = 100.0;
                    } else {
                        state.fit_camera = true;
                    }
                }
                if ui
                    .add_sized(
                        size,
                        egui::Button::new("Adjust display").selected(state.focus_adjust_open),
                    )
                    .clicked()
                {
                    state.focus_adjust_open = !state.focus_adjust_open;
                }
            });
        });
    if state.focus_adjust_open {
        if slice {
            sheet(ctx, state, slice_panel::slice_sheet);
        } else {
            sheet(ctx, state, controls::volume_sheet);
        }
    } else if slice && !state.slice_view.composite && state.slice_view.max_slices > 1 {
        // Stepping through slices is the main thing to do here: keep it out.
        egui::TopBottomPanel::bottom("focus_slice_row")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| slice_panel::slice_row_only(ui, state));
    }
    if slice {
        slice_area(ctx, state);
    }
}
