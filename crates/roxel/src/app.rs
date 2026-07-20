//! Application state and egui UI.
//!
//! Manages the main window, egui panels, and bridges user interaction
//! with the volume renderer. Layout matches the Go viewer:
//!
//! ```text
//! ┌──────────┬──────────────────────┬──────────────────────┐
//! │ Sidebar  │   3D Volume View     │   2D Slice View      │
//! │          │   (ray-caster)       │   (CPU rendered)     │
//! │ Metadata │                      │                      │
//! │ Layers   ├──────────────────────┼──────────────────────┤
//! │ Threats  │ Quality | Opacity    │ Volume: [dropdown]   │
//! │          │ Preset | Bands       │ View:   [dropdown]   │
//! │          │ Lighting | WC/WW     │ Composite [x]        │
//! │          │                      │ W/L: __ W/W: __      │
//! │          │                      │ Slice: [slider]      │
//! └──────────┴──────────────────────┴──────────────────────┘
//! ```

use crate::camera::Camera;
use crate::loader::{LoadOutcome, LoadedLayer, VolumeLoader};
use crate::renderer::{RenderParams, VolumeRenderer};
use crate::state::UiState;
use crate::transfer::{TransferFunction, TransferPreset};
use crate::volume::{self, ThreatBox, Volume};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::WindowEvent;

/// A named volume layer loaded from a DICOS file.
pub(crate) struct VolumeLayer {
    /// Display name (filename stem or modality-based key).
    pub(crate) name: String,
    /// The volume data.
    pub(crate) volume: Volume,
}

/// Main application state.
pub struct App {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,

    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,

    renderer: VolumeRenderer,
    ui: UiState,

    /// Background volume loader (CPU pipeline runs off the render thread).
    loader: VolumeLoader,

    // Mouse drag state.
    dragging: bool,
    last_mouse_pos: Option<(f32, f32)>,
    shift_down: bool,
}

impl App {
    pub fn new(
        window: &winit::window::Window,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let egui_ctx = egui::Context::default();

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        let renderer = VolumeRenderer::new(&device, surface_format);
        let mut camera = Camera::default();
        camera.set_coronal();

        Self {
            device,
            queue,
            egui_ctx,
            egui_state,
            egui_renderer,
            renderer,
            ui: UiState::new(camera),
            loader: VolumeLoader::new(),
            dragging: false,
            last_mouse_pos: None,
            shift_down: false,
        }
    }

    pub fn handle_event(&mut self, window: &winit::window::Window, event: &WindowEvent) -> bool {
        let response = self.egui_state.on_window_event(window, event);
        if response.consumed {
            return true;
        }

        let needs_repaint = match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left {
                    self.dragging = *state == winit::event::ElementState::Pressed;
                    if !self.dragging {
                        self.last_mouse_pos = None;
                    }
                }
                self.dragging
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift_down = modifiers.state().shift_key();
                false
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = (position.x as f32, position.y as f32);
                if self.dragging {
                    if let Some(last) = self.last_mouse_pos {
                        let dx_px = pos.0 - last.0;
                        let dy_px = pos.1 - last.1;
                        if self.shift_down {
                            self.ui.camera.pan_pixels(dx_px, dy_px);
                        } else {
                            let dx = dx_px * 0.005;
                            let dy = dy_px * 0.005;
                            self.ui.camera.rotate(dx, -dy);
                        }
                    }
                    self.last_mouse_pos = Some(pos);
                }
                self.dragging
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => *y,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.01,
                };
                let factor = 1.0 + scroll * 0.1;
                self.ui.camera.zoom(factor);
                true
            }
            _ => false,
        };

        response.repaint || needs_repaint
    }

    pub fn render(
        &mut self,
        window: &winit::window::Window,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> bool {
        // Install any completed background load before drawing this frame.
        if let Some(outcome) = self.loader.poll() {
            match outcome {
                LoadOutcome::Loaded { path, layers } => self.install_loaded(path, layers),
                LoadOutcome::Failed { path, error } => {
                    let msg = format!("Failed to load {}: {error}", path.display());
                    log::error!("{msg}");
                    self.ui.load_error = Some(msg);
                }
            }
        }

        // Reflect current loading state into the UI (spinner + disabled Open
        // buttons) so the sidebar closure can render it without touching `App`.
        self.ui.loading_file = self
            .loader
            .loading()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());

        // Clear action flags.
        self.ui.actions.file_to_load = None;
        self.ui.actions.preset_changed = false;
        self.ui.transfer.bands_changed = false;
        self.ui.actions.upload_3d_index = None;

        // Run egui UI -- pass only the UiState to avoid borrow conflicts.
        let raw_input = self.egui_state.take_egui_input(window);
        let ui = &mut self.ui;
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            crate::ui::draw_ui(ctx, ui);
        });

        // Process UI actions.
        if let Some(path) = self.ui.actions.file_to_load.take() {
            self.request_load(path);
        }

        if let Some(idx) = self.ui.actions.upload_3d_index {
            self.upload_volume_to_gpu(idx);
        }

        if self.ui.actions.preset_changed {
            // Preset switches are applied immediately; cancel any pending
            // debounced band update from previous interactions.
            self.ui.transfer.bands_debounce_deadline = None;
            let tf = TransferFunction::from_preset(self.ui.transfer.preset);
            self.renderer
                .upload_transfer_function(&self.device, &self.queue, &tf);
            self.ui.slice_view.transfer_func = Some(tf.data);
            self.ui.slice_dirty = true;
        }

        // Debounce expensive transfer function rebuild/upload while dragging
        // band handles. Apply after a short quiet period.
        if self.ui.transfer.bands_changed {
            self.ui.transfer.bands_debounce_deadline =
                Some(Instant::now() + Duration::from_millis(60));
        }

        if self.ui.transfer.preset != TransferPreset::Default {
            self.ui.transfer.bands_debounce_deadline = None;
        }

        if let Some(deadline) = self.ui.transfer.bands_debounce_deadline {
            let now = Instant::now();
            if now >= deadline {
                self.ui.transfer.bands_debounce_deadline = None;
                let tf = TransferFunction::from_bands(&self.ui.transfer.bands);
                self.renderer
                    .upload_transfer_function(&self.device, &self.queue, &tf);
                self.ui.slice_view.transfer_func = Some(tf.data);
                self.ui.slice_dirty = true;
            } else {
                self.egui_ctx
                    .request_repaint_after(deadline.saturating_duration_since(now));
            }
        }

        // Sync alpha scale to 2D composite.
        self.ui.slice_view.alpha_scale = self.ui.settings.global_opacity;

        // Update 2D slice texture if dirty.
        if self.ui.slice_dirty {
            self.update_slice_texture();
            self.ui.slice_dirty = false;
        }

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: full_output.pixels_per_point,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        let active_3d_threats: &[ThreatBox] = self
            .ui
            .library
            .active_3d_index
            .and_then(|idx| self.ui.library.volumes.get(idx))
            .map(|layer| layer.volume.threats.as_slice())
            .unwrap_or(&[]);

        // Render 3D volume.
        self.renderer.render(
            &self.queue,
            &mut encoder,
            surface_view,
            RenderParams {
                camera: &self.ui.camera,
                viewport: [width, height],
                threats: active_3d_threats,
                show_threats: self.ui.library.show_threats,
                settings: &self.ui.settings,
            },
        );

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            // egui-wgpu currently requires a `'static` render pass. Use the
            // safe lifetime erasure helper provided by wgpu.
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let mut rpass_static = rpass.forget_lifetime();
            self.egui_renderer
                .render(&mut rpass_static, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit([encoder.finish()]);

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        // Keep the frame loop pumping while a background load is in flight so
        // we continue polling (and animating the spinner) without user input.
        if self.loader.loading().is_some() {
            self.egui_ctx.request_repaint();
        }

        self.egui_ctx.has_requested_repaint()
    }

    /// Request a background load of a file or directory. Called from main for
    /// CLI args and from the sidebar Open buttons.
    ///
    /// The heavy CPU pipeline (parse, decode, threat merge, GPU packing,
    /// center-of-mass) runs on a worker thread; [`App::render`] installs the
    /// result when it completes. See [`crate::loader`].
    pub fn load_path(&mut self, path: &std::path::Path) {
        self.request_load(path.to_path_buf());
    }

    /// Kick off a background load, clearing any stale error message.
    fn request_load(&mut self, path: PathBuf) {
        self.ui.load_error = None;
        self.loader.request(path);
    }

    /// Install the layers produced by a completed background load, replacing
    /// the current volume library and re-activating the view.
    fn install_loaded(&mut self, path: PathBuf, layers: Vec<LoadedLayer>) {
        self.ui.load_error = None;
        self.ui.library.volumes.clear();

        // Keep the first layer's precomputed GPU buffer + center of mass so the
        // render thread uploads it without re-packing (Codex finding 19).
        let mut first_pack: Option<(Vec<u16>, [f32; 3])> = None;
        for (i, layer) in layers.into_iter().enumerate() {
            let LoadedLayer {
                name,
                volume,
                packed,
                center_of_mass,
            } = layer;
            if i == 0 {
                first_pack = Some((packed, center_of_mass));
            }
            self.ui.library.volumes.push(VolumeLayer { name, volume });
        }

        if self.ui.library.volumes.is_empty() {
            return;
        }

        self.ui.library.loaded_path = Some(path);
        self.activate_volumes(first_pack);
        self.log_threat_status();
    }

    /// After loading volumes, set up 3D renderer and 2D slice view.
    ///
    /// `first_pack` carries the precomputed packed buffer + center of mass for
    /// layer 0 (from the background loader). When absent, layer 0 is packed on
    /// the render thread via [`Self::upload_volume_to_gpu`].
    fn activate_volumes(&mut self, first_pack: Option<(Vec<u16>, [f32; 3])>) {
        if self.ui.library.volumes.is_empty() {
            return;
        }

        // Initialize the 3D camera view to coronal on each new dataset load.
        self.ui.camera.set_coronal();

        // Upload first volume to 3D renderer.
        match first_pack {
            Some((packed, com)) => self.upload_volume_to_gpu_precomputed(0, &packed, com),
            None => self.upload_volume_to_gpu(0),
        }

        // Point 2D slice view at first volume.
        self.ui.slice_view.volume_index = 0;
        self.ui.library.selected_threat = None;

        let vol = &self.ui.library.volumes[0].volume;

        // Default to Coronal view at mid-slice (matching Go viewer).
        self.ui.slice_view.orientation = crate::slice_view::Orientation::Coronal;
        self.ui.slice_view.update_for_volume(vol);
        self.ui.slice_view.slice_index = self.ui.slice_view.max_slices / 2;

        // Initialize 2D window/level from volume metadata.
        self.ui.slice_view.window_center = vol.window_center as f32;
        self.ui.slice_view.window_width = vol.window_width as f32;

        // Sync 3D renderer window/level from computed values.
        self.ui.settings.window_center = vol.window_center as f32;
        self.ui.settings.window_width = vol.window_width as f32;

        // Pass the current transfer function to the 2D composite renderer.
        let tf = TransferFunction::from_preset(self.ui.transfer.preset);
        self.ui.slice_view.transfer_func = Some(tf.data);
        self.ui.slice_view.alpha_scale = self.ui.settings.global_opacity;

        self.ui.slice_dirty = true;
    }

    /// Upload a volume at the given index to the 3D GPU renderer, packing on
    /// the render thread. Used for layer switches, where no precomputed buffer
    /// is available.
    fn upload_volume_to_gpu(&mut self, idx: usize) {
        if let Some(layer) = self.ui.library.volumes.get(idx) {
            let vol = &layer.volume;
            let com = vol.center_of_mass();
            self.ui.camera.target = glam::Vec3::from(com);

            self.renderer.upload_volume(&self.device, &self.queue, vol);

            let tf = TransferFunction::from_preset(self.ui.transfer.preset);
            self.renderer
                .upload_transfer_function(&self.device, &self.queue, &tf);

            self.ui.library.active_3d_index = Some(idx);
        }
    }

    /// Upload a volume at the given index using a precomputed packed buffer and
    /// center of mass produced by the background loader — no CPU packing on the
    /// render thread.
    fn upload_volume_to_gpu_precomputed(&mut self, idx: usize, packed: &[u16], com: [f32; 3]) {
        if let Some(layer) = self.ui.library.volumes.get(idx) {
            let vol = &layer.volume;
            self.ui.camera.target = glam::Vec3::from(com);

            self.renderer
                .upload_volume_packed(&self.device, &self.queue, vol, packed);

            let tf = TransferFunction::from_preset(self.ui.transfer.preset);
            self.renderer
                .upload_transfer_function(&self.device, &self.queue, &tf);

            self.ui.library.active_3d_index = Some(idx);
        }
    }

    /// Re-render the 2D slice and update the egui texture.
    fn update_slice_texture(&mut self) {
        let vol_idx = self.ui.slice_view.volume_index;
        let vol = match self.ui.library.volumes.get(vol_idx) {
            Some(layer) => &layer.volume,
            None => return,
        };

        let image = self.ui.slice_view.render(vol, self.ui.library.show_threats);
        if let Some(texture) = self.ui.slice_texture.as_mut() {
            texture.set(image, egui::TextureOptions::NEAREST);
        } else {
            let texture =
                self.egui_ctx
                    .load_texture("slice_2d", image, egui::TextureOptions::NEAREST);
            self.ui.slice_texture = Some(texture);
        }
    }

    fn log_threat_status(&self) {
        let total: usize = self
            .ui
            .library
            .volumes
            .iter()
            .map(|l| l.volume.threats.len())
            .sum();
        if total == 0 {
            log::info!("No threat boxes found in loaded data");
            return;
        }
        let with_threats = self
            .ui
            .library
            .volumes
            .iter()
            .filter(|l| !l.volume.threats.is_empty())
            .count();
        log::info!("Loaded {total} threat box(es) across {with_threats} volume layer(s)");
    }
}

pub(crate) fn merge_unique_threats(dst: &mut Vec<ThreatBox>, src: Vec<ThreatBox>) -> usize {
    let mut added = 0;
    for threat in src {
        let duplicate = dst.iter().any(|existing| {
            existing.name == threat.name
                && existing.min == threat.min
                && existing.max == threat.max
                && existing.confidence == threat.confidence
        });
        if !duplicate {
            dst.push(threat);
            added += 1;
        }
    }
    for (i, threat) in dst.iter_mut().enumerate() {
        threat.color = volume::threat_color_for_index(i);
    }
    added
}
