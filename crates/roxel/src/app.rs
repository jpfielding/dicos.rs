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
            self.load_path_inner(&path);
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

        self.egui_ctx.has_requested_repaint()
    }

    /// Load a file or directory from a path. Called from main for CLI args.
    pub fn load_path(&mut self, path: &std::path::Path) {
        self.load_path_inner(path);
    }

    fn load_path_inner(&mut self, path: &std::path::Path) {
        if path.is_dir() {
            self.load_directory(path);
        } else {
            self.load_single_file(path);
        }
    }

    /// Load a single DICOS file as one volume layer.
    fn load_single_file(&mut self, path: &std::path::Path) {
        match volume::load_dicos_path(path) {
            Ok(mut vol) => {
                if let Some(dir) = path.parent() {
                    let sidecars = volume::load_threat_sidecars_from_dir(
                        dir,
                        [vol.dim_x, vol.dim_y, vol.dim_z],
                    );
                    let added = merge_unique_threats(&mut vol.threats, sidecars);
                    if added > 0 {
                        log::info!("Loaded {added} threat box(es) from sidecar files");
                    }
                }

                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                self.ui.library.volumes.clear();
                self.ui
                    .library
                    .volumes
                    .push(VolumeLayer { name, volume: vol });
                self.ui.library.loaded_path = Some(path.to_path_buf());
                self.activate_volumes();
                self.log_threat_status();
                log::info!("Loaded 1 volume from {}", path.display());
            }
            Err(e) => {
                log::error!("Failed to load {}: {e}", path.display());
            }
        }
    }

    /// Load a directory of DICOS files as separate volume layers.
    ///
    /// Each .dcs/.dcm file becomes its own named layer, matching the Go
    /// viewer's approach where volumes are kept separate rather than stacked.
    fn load_directory(&mut self, dir: &std::path::Path) {
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    if path.is_file() {
                        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                        if ext == "dcs" || ext == "dcm" {
                            return Some(path);
                        }
                    }
                    None
                })
                .collect(),
            Err(e) => {
                log::error!("Failed to read directory {}: {e}", dir.display());
                return;
            }
        };

        files.sort();

        if files.is_empty() {
            log::error!("No .dcs or .dcm files found in {}", dir.display());
            return;
        }

        log::info!("Loading {} DICOS files from {}", files.len(), dir.display());

        self.ui.library.volumes.clear();
        for file in &files {
            match volume::load_dicos_path(file) {
                Ok(vol) => {
                    let name = file
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    log::info!(
                        "  {} -> {}x{}x{} ({})",
                        name,
                        vol.dim_x,
                        vol.dim_y,
                        vol.dim_z,
                        vol.modality
                    );
                    self.ui
                        .library
                        .volumes
                        .push(VolumeLayer { name, volume: vol });
                }
                Err(e) => {
                    log::warn!("Skipping {}: {e}", file.display());
                }
            }
        }

        if self.ui.library.volumes.is_empty() {
            log::error!("No volumes loaded from {}", dir.display());
            return;
        }

        if let Some((dim_x, dim_y, dim_z)) = self
            .ui
            .library
            .volumes
            .iter()
            .map(|layer| {
                let vol = &layer.volume;
                (vol.dim_x, vol.dim_y, vol.dim_z)
            })
            .max_by_key(|(x, y, z)| x.saturating_mul(*y).saturating_mul(*z))
        {
            let sidecars = volume::load_threat_sidecars_from_dir(dir, [dim_x, dim_y, dim_z]);
            for layer in &mut self.ui.library.volumes {
                let vol = &mut layer.volume;
                if (vol.dim_x, vol.dim_y, vol.dim_z) == (dim_x, dim_y, dim_z) {
                    merge_unique_threats(&mut vol.threats, sidecars.clone());
                }
            }
        }

        self.ui.library.loaded_path = Some(dir.to_path_buf());
        self.activate_volumes();
        self.log_threat_status();
        log::info!(
            "Loaded {} volume layers from {}",
            self.ui.library.volumes.len(),
            dir.display()
        );
    }

    /// After loading volumes, set up 3D renderer and 2D slice view.
    fn activate_volumes(&mut self) {
        if self.ui.library.volumes.is_empty() {
            return;
        }

        // Initialize the 3D camera view to coronal on each new dataset load.
        self.ui.camera.set_coronal();

        // Upload first volume to 3D renderer.
        self.upload_volume_to_gpu(0);

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

    /// Upload a volume at the given index to the 3D GPU renderer.
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
