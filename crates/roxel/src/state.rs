//! Decomposed UI state.
//!
//! `UiState` is the bundle handed to the egui drawing closures (see
//! [`crate::ui`]) so they can mutate viewer state without borrowing the whole
//! [`App`](crate::app::App). It is composed of focused sub-structs rather than
//! being a single flat god-struct:
//!
//! - [`VolumeLibrary`] — loaded volume layers and selection state.
//! - [`TransferState`] — transfer-function preset, bands, and debounce timing.
//! - [`RenderSettings`] — plain-data 3D render parameters (shared with the
//!   renderer).
//! - [`UiActions`] — transient flags set by UI closures and drained by
//!   [`App::render`](crate::app::App::render).

use crate::app::VolumeLayer;
use crate::camera::Camera;
use crate::loader::LoadSource;
use crate::renderer::Quality;
use crate::slice_view::SliceView;
use crate::transfer::{self, ColorBand, TransferPreset};
use crate::ui::layout::{Layout, PhoneTab, SlicePage, VolumePage};

use std::path::PathBuf;
use web_time::Instant;

/// UI state that can be passed into egui closures without borrowing the whole App.
pub(crate) struct UiState {
    pub(crate) camera: Camera,
    pub(crate) viewport: egui::Rect,
    pub(crate) fit_camera: bool,
    pub(crate) volume_extent: glam::Vec3,
    pub(crate) show_slice_locator: bool,
    pub(crate) cutaway: bool,
    pub(crate) reverse_cutaway: bool,
    /// "Focus slice": the 2D slice fills the workspace.
    pub(crate) slice_expanded: bool,
    /// "Focus volume": the 3D view fills the workspace.
    pub(crate) focus_volume: bool,
    /// Layout chosen from the available logical size (refreshed every frame).
    pub(crate) layout: Layout,
    /// Compact layouts: the selected tab.
    pub(crate) phone_tab: PhoneTab,
    /// Compact layouts: whether the selected view's controls sheet is open.
    pub(crate) sheet_open: bool,
    /// Compact focus modes: whether the controls sheet is open.
    pub(crate) focus_adjust_open: bool,
    /// Compact layouts: the page of the 3D controls sheet.
    pub(crate) volume_page: VolumePage,
    /// Compact layouts: the page of the 2D controls sheet.
    pub(crate) slice_page: SlicePage,
    /// Compact layouts: the material-band editor replaces the Display page.
    pub(crate) bands_open: bool,
    /// Compact layouts: the band whose opacity the editor shows.
    pub(crate) selected_band: usize,
    /// Touch: a one-finger drag (and the arrow buttons) pan instead of orbit.
    pub(crate) drag_pans: bool,

    /// Loaded volume layers and threat selection state.
    pub(crate) library: VolumeLibrary,
    /// Transfer-function preset, bands, and debounce timing.
    pub(crate) transfer: TransferState,
    /// Plain-data 3D render parameters mirrored for the UI sliders.
    pub(crate) settings: RenderSettings,
    /// Transient flags set by UI closures, drained after each frame.
    pub(crate) actions: UiActions,

    // 2D slice view state.
    pub(crate) slice_view: SliceView,
    pub(crate) slice_texture: Option<egui::TextureHandle>,
    /// True when the 2D image needs re-rendering (volume/slice/orientation changed).
    pub(crate) slice_dirty: bool,

    /// Filename currently being loaded in the background, or `None` when idle.
    /// Refreshed each frame from the [`VolumeLoader`](crate::loader::VolumeLoader)
    /// so the sidebar can show a spinner and disable the Open buttons.
    pub(crate) loading_file: Option<String>,
    /// Last load error message, shown in the sidebar until the next load.
    pub(crate) load_error: Option<String>,
}

impl UiState {
    /// Keep cached composite pixels in sync with shared opacity controls.
    pub(crate) fn sync_composite_opacity(&mut self) {
        if self.slice_view.alpha_scale != self.settings.global_opacity {
            self.slice_view.alpha_scale = self.settings.global_opacity;
            self.slice_dirty |= self.slice_view.composite;
        }
    }

    /// Whether the 3D volume is on screen: the central area belongs to it
    /// unless the 2D slice or a compact page has taken that area over.
    pub(crate) fn volume_visible(&self) -> bool {
        if self.library.volumes.is_empty() || self.slice_expanded {
            return false;
        }
        !self.layout.is_compact() || self.focus_volume || self.phone_tab == PhoneTab::View3d
    }

    /// Build the initial UI state around a freshly configured camera.
    pub(crate) fn new(camera: Camera) -> Self {
        Self {
            camera,
            viewport: egui::Rect::NOTHING,
            fit_camera: false,
            volume_extent: glam::Vec3::ONE,
            show_slice_locator: true,
            cutaway: false,
            reverse_cutaway: false,
            slice_expanded: false,
            focus_volume: false,
            layout: Layout::Desktop,
            phone_tab: PhoneTab::Slice,
            sheet_open: true,
            focus_adjust_open: false,
            volume_page: VolumePage::Camera,
            slice_page: SlicePage::Slice,
            bands_open: false,
            selected_band: 1,
            drag_pans: false,
            library: VolumeLibrary::default(),
            transfer: TransferState::default(),
            settings: RenderSettings::default(),
            actions: UiActions::default(),
            slice_view: SliceView::default(),
            slice_texture: None,
            slice_dirty: false,
            loading_file: None,
            load_error: None,
        }
    }
}

/// Loaded volume layers plus threat-overlay selection state.
pub(crate) struct VolumeLibrary {
    pub(crate) volumes: Vec<VolumeLayer>,
    /// Index of the volume currently uploaded to the 3D GPU renderer.
    pub(crate) active_3d_index: Option<usize>,
    pub(crate) loaded_path: Option<PathBuf>,
    /// Global toggle for showing threat boxes in the 2D view.
    pub(crate) show_threats: bool,
    /// Optional selected threat index for threat list actions.
    pub(crate) selected_threat: Option<usize>,
}

impl Default for VolumeLibrary {
    fn default() -> Self {
        Self {
            volumes: Vec::new(),
            active_3d_index: None,
            loaded_path: None,
            show_threats: true,
            selected_threat: None,
        }
    }
}

/// Transfer-function preset, editable bands, and edit-debounce timing.
pub(crate) struct TransferState {
    pub(crate) preset: TransferPreset,
    pub(crate) bands: Vec<ColorBand>,
    pub(crate) bands_changed: bool,
    /// Debounce timer for transfer-band edits (threshold/alpha).
    pub(crate) bands_debounce_deadline: Option<Instant>,
}

impl Default for TransferState {
    fn default() -> Self {
        Self {
            preset: TransferPreset::Monochrome,
            bands: transfer::default_bands(),
            bands_changed: false,
            bands_debounce_deadline: None,
        }
    }
}

/// Plain-data 3D render parameters mirrored for the UI sliders.
///
/// Kept as a plain, public struct because it is shared with the renderer via
/// `RenderParams`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    pub window_center: f32,
    pub window_width: f32,
    pub density_threshold: f32,
    pub quality: Quality,
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    /// Attenuate distant samples to clarify overlapping surfaces.
    pub depth_cue: f32,
    /// Global opacity / alpha scale applied to both the 3D and 2D composites.
    pub global_opacity: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            window_center: 32768.0,
            window_width: 65536.0,
            density_threshold: 0.0,
            quality: Quality::High,
            ambient: 0.65,
            diffuse: 0.8,
            specular: 0.3,
            depth_cue: 0.35,
            global_opacity: 0.25,
        }
    }
}

/// Transient action flags set by UI closures and drained by `App::render`.
#[derive(Default)]
pub(crate) struct UiActions {
    pub(crate) file_to_load: Option<LoadSource>,
    /// Request to upload a different volume to the 3D renderer.
    pub(crate) upload_3d_index: Option<usize>,
    pub(crate) preset_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_changes_invalidate_composites_but_not_single_slices() {
        let mut state = UiState::new(Camera::default());
        state.slice_dirty = false;
        state.settings.global_opacity = 0.4;
        state.sync_composite_opacity();
        assert!(!state.slice_dirty);
        assert_eq!(state.slice_view.alpha_scale, 0.4);
        state.slice_view.composite = true;
        state.settings.global_opacity = 0.7;
        state.sync_composite_opacity();
        assert!(state.slice_dirty);
        assert_eq!(state.slice_view.alpha_scale, 0.7);
        state.slice_dirty = false;
        state.sync_composite_opacity();
        assert!(!state.slice_dirty);
    }
}
