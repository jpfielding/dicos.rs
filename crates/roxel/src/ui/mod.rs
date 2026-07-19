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

use crate::state::UiState;
use crate::transfer::ColorBand;

/// Draw the full three-panel UI.
pub(crate) fn draw_ui(ctx: &egui::Context, state: &mut UiState) {
    sidebar::draw_left_sidebar(ctx, state);
    slice_panel::draw_right_panel(ctx, state);
    // The remaining central area is where the 3D wgpu renderer draws.
    controls::draw_bottom_controls(ctx, state);
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
