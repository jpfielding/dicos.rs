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
use crate::renderer::Quality;
use crate::slice_view::SliceView;
use crate::transfer::{self, ColorBand, TransferPreset};

use std::path::PathBuf;
use std::time::Instant;

/// UI state that can be passed into egui closures without borrowing the whole App.
pub(crate) struct UiState {
    pub(crate) camera: Camera,

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
}

impl UiState {
    /// Build the initial UI state around a freshly configured camera.
    pub(crate) fn new(camera: Camera) -> Self {
        Self {
            camera,
            library: VolumeLibrary::default(),
            transfer: TransferState::default(),
            settings: RenderSettings::default(),
            actions: UiActions::default(),
            slice_view: SliceView::default(),
            slice_texture: None,
            slice_dirty: false,
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
            preset: TransferPreset::Default,
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
    /// Global opacity / alpha scale applied to both the 3D and 2D composites.
    pub global_opacity: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            window_center: 32768.0,
            window_width: 65536.0,
            density_threshold: 0.0,
            quality: Quality::Medium,
            ambient: 0.3,
            diffuse: 0.6,
            specular: 0.3,
            global_opacity: 0.5,
        }
    }
}

/// Transient action flags set by UI closures and drained by `App::render`.
#[derive(Default)]
pub(crate) struct UiActions {
    pub(crate) file_to_load: Option<PathBuf>,
    /// Request to upload a different volume to the 3D renderer.
    pub(crate) upload_3d_index: Option<usize>,
    pub(crate) preset_changed: bool,
}
