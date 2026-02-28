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
use crate::renderer::{Quality, VolumeRenderer};
use crate::slice_view::{Orientation, SliceView};
use crate::transfer::{self, ColorBand, TransferFunction, TransferPreset};
use crate::volume::{self, ThreatBox, Volume};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::WindowEvent;

/// A named volume layer loaded from a DICOS file.
struct VolumeLayer {
    /// Display name (filename stem or modality-based key).
    name: String,
    /// The volume data.
    volume: Volume,
}

/// UI state that can be passed into egui closures without borrowing the whole App.
struct UiState {
    camera: Camera,
    preset: TransferPreset,
    bands: Vec<ColorBand>,
    global_opacity: f32,
    loaded_path: Option<PathBuf>,

    // Renderer params mirrored for UI sliders.
    window_center: f32,
    window_width: f32,
    density_threshold: f32,
    quality: Quality,
    ambient: f32,
    diffuse: f32,
    specular: f32,

    // Multi-volume state.
    volumes: Vec<VolumeLayer>,
    /// Index of the volume currently uploaded to the 3D GPU renderer.
    active_3d_index: Option<usize>,
    /// Global toggle for showing threat boxes in the 2D view.
    show_threats: bool,
    /// Optional selected threat index for threat list actions.
    selected_threat: Option<usize>,

    // 2D slice view state.
    slice_view: SliceView,
    slice_texture: Option<egui::TextureHandle>,
    /// True when the 2D image needs re-rendering (volume/slice/orientation changed).
    slice_dirty: bool,

    // Action flags set by UI, processed after frame.
    file_to_load: Option<PathBuf>,
    preset_changed: bool,
    bands_changed: bool,
    /// Debounce timer for transfer-band edits (threshold/alpha).
    bands_debounce_deadline: Option<Instant>,
    /// Request to upload a different volume to the 3D renderer.
    upload_3d_index: Option<usize>,
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
            ui: UiState {
                camera,
                preset: TransferPreset::Default,
                bands: transfer::default_bands(),
                global_opacity: 0.5,
                loaded_path: None,
                window_center: 32768.0,
                window_width: 65536.0,
                density_threshold: 0.0,
                quality: Quality::Medium,
                ambient: 0.3,
                diffuse: 0.6,
                specular: 0.3,
                volumes: Vec::new(),
                active_3d_index: None,
                show_threats: true,
                selected_threat: None,
                slice_view: SliceView::default(),
                slice_texture: None,
                slice_dirty: false,
                file_to_load: None,
                preset_changed: false,
                bands_changed: false,
                bands_debounce_deadline: None,
                upload_3d_index: None,
            },
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
        self.ui.file_to_load = None;
        self.ui.preset_changed = false;
        self.ui.bands_changed = false;
        self.ui.upload_3d_index = None;

        // Run egui UI -- pass only the UiState to avoid borrow conflicts.
        let raw_input = self.egui_state.take_egui_input(window);
        let ui = &mut self.ui;
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            draw_ui(ctx, ui);
        });

        // Process UI actions.
        if let Some(path) = self.ui.file_to_load.take() {
            self.load_path_inner(&path);
        }

        if let Some(idx) = self.ui.upload_3d_index {
            self.upload_volume_to_gpu(idx);
        }

        if self.ui.preset_changed {
            // Preset switches are applied immediately; cancel any pending
            // debounced band update from previous interactions.
            self.ui.bands_debounce_deadline = None;
            let tf = TransferFunction::from_preset(self.ui.preset);
            self.renderer
                .upload_transfer_function(&self.device, &self.queue, &tf);
            self.ui.slice_view.transfer_func = Some(tf.data);
            self.ui.slice_dirty = true;
        }

        // Debounce expensive transfer function rebuild/upload while dragging
        // band handles. Apply after a short quiet period.
        if self.ui.bands_changed {
            self.ui.bands_debounce_deadline = Some(Instant::now() + Duration::from_millis(60));
        }

        if self.ui.preset != TransferPreset::Default {
            self.ui.bands_debounce_deadline = None;
        }

        if let Some(deadline) = self.ui.bands_debounce_deadline {
            let now = Instant::now();
            if now >= deadline {
                self.ui.bands_debounce_deadline = None;
                let tf = TransferFunction::from_bands(&self.ui.bands);
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
        self.ui.slice_view.alpha_scale = self.ui.global_opacity;

        // Update 2D slice texture if dirty.
        if self.ui.slice_dirty {
            self.update_slice_texture();
            self.ui.slice_dirty = false;
        }

        // Sync UI state to renderer.
        self.renderer.window_center = self.ui.window_center;
        self.renderer.window_width = self.ui.window_width;
        self.renderer.alpha_scale = self.ui.global_opacity;
        self.renderer.density_threshold = self.ui.density_threshold;
        self.renderer.quality = self.ui.quality;
        self.renderer.ambient = self.ui.ambient;
        self.renderer.diffuse = self.ui.diffuse;
        self.renderer.specular = self.ui.specular;

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
            .active_3d_index
            .and_then(|idx| self.ui.volumes.get(idx))
            .map(|layer| layer.volume.threats.as_slice())
            .unwrap_or(&[]);

        // Render 3D volume.
        self.renderer.render(
            &self.queue,
            &mut encoder,
            surface_view,
            &self.ui.camera,
            width,
            height,
            active_3d_threats,
            self.ui.show_threats,
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
                self.ui.volumes.clear();
                self.ui.volumes.push(VolumeLayer { name, volume: vol });
                self.ui.loaded_path = Some(path.to_path_buf());
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

        self.ui.volumes.clear();
        for file in &files {
            match volume::load_dicos_volume(file) {
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
                    self.ui.volumes.push(VolumeLayer { name, volume: vol });
                }
                Err(e) => {
                    log::warn!("Skipping {}: {e}", file.display());
                }
            }
        }

        if self.ui.volumes.is_empty() {
            log::error!("No volumes loaded from {}", dir.display());
            return;
        }

        if let Some((dim_x, dim_y, dim_z)) = self
            .ui
            .volumes
            .iter()
            .map(|layer| {
                let vol = &layer.volume;
                (vol.dim_x, vol.dim_y, vol.dim_z)
            })
            .max_by_key(|(x, y, z)| x.saturating_mul(*y).saturating_mul(*z))
        {
            let sidecars = volume::load_threat_sidecars_from_dir(dir, [dim_x, dim_y, dim_z]);
            for layer in &mut self.ui.volumes {
                let vol = &mut layer.volume;
                if (vol.dim_x, vol.dim_y, vol.dim_z) == (dim_x, dim_y, dim_z) {
                    merge_unique_threats(&mut vol.threats, sidecars.clone());
                }
            }
        }

        self.ui.loaded_path = Some(dir.to_path_buf());
        self.activate_volumes();
        self.log_threat_status();
        log::info!(
            "Loaded {} volume layers from {}",
            self.ui.volumes.len(),
            dir.display()
        );
    }

    /// After loading volumes, set up 3D renderer and 2D slice view.
    fn activate_volumes(&mut self) {
        if self.ui.volumes.is_empty() {
            return;
        }

        // Initialize the 3D camera view to coronal on each new dataset load.
        self.ui.camera.set_coronal();

        // Upload first volume to 3D renderer.
        self.upload_volume_to_gpu(0);

        // Point 2D slice view at first volume.
        self.ui.slice_view.volume_index = 0;
        self.ui.selected_threat = None;

        let vol = &self.ui.volumes[0].volume;

        // Default to Coronal view at mid-slice (matching Go viewer).
        self.ui.slice_view.orientation = crate::slice_view::Orientation::Coronal;
        self.ui.slice_view.update_for_volume(vol);
        self.ui.slice_view.slice_index = self.ui.slice_view.max_slices / 2;

        // Initialize 2D window/level from volume metadata.
        self.ui.slice_view.window_center = vol.window_center as f32;
        self.ui.slice_view.window_width = vol.window_width as f32;

        // Sync 3D renderer window/level from computed values.
        self.ui.window_center = vol.window_center as f32;
        self.ui.window_width = vol.window_width as f32;

        // Pass the current transfer function to the 2D composite renderer.
        let tf = TransferFunction::from_preset(self.ui.preset);
        self.ui.slice_view.transfer_func = Some(tf.data);
        self.ui.slice_view.alpha_scale = self.ui.global_opacity;

        self.ui.slice_dirty = true;
    }

    /// Upload a volume at the given index to the 3D GPU renderer.
    fn upload_volume_to_gpu(&mut self, idx: usize) {
        if let Some(layer) = self.ui.volumes.get(idx) {
            let vol = &layer.volume;
            let com = vol.center_of_mass();
            self.ui.camera.target = glam::Vec3::from(com);

            self.renderer.upload_volume(&self.device, &self.queue, vol);

            let tf = TransferFunction::from_preset(self.ui.preset);
            self.renderer
                .upload_transfer_function(&self.device, &self.queue, &tf);

            self.ui.active_3d_index = Some(idx);
        }
    }

    /// Re-render the 2D slice and update the egui texture.
    fn update_slice_texture(&mut self) {
        let vol_idx = self.ui.slice_view.volume_index;
        let vol = match self.ui.volumes.get(vol_idx) {
            Some(layer) => &layer.volume,
            None => return,
        };

        let image = self.ui.slice_view.render(vol, self.ui.show_threats);
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
        let total: usize = self.ui.volumes.iter().map(|l| l.volume.threats.len()).sum();
        if total == 0 {
            log::info!("No threat boxes found in loaded data");
            return;
        }
        let with_threats = self
            .ui
            .volumes
            .iter()
            .filter(|l| !l.volume.threats.is_empty())
            .count();
        log::info!("Loaded {total} threat box(es) across {with_threats} volume layer(s)");
    }
}

fn merge_unique_threats(dst: &mut Vec<ThreatBox>, src: Vec<ThreatBox>) -> usize {
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

// ---------------------------------------------------------------------------
// egui UI drawing
// ---------------------------------------------------------------------------

/// Returns true when a slider value is "committed" -- either the drag ended,
/// the value changed without dragging (click / keyboard), or the text field
/// lost focus.  Use this to debounce expensive operations (CPU re-renders)
/// so they only fire once instead of every frame during a drag.
fn slider_committed(r: &egui::Response) -> bool {
    r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged())
}

fn band_color(band: &ColorBand) -> egui::Color32 {
    egui::Color32::from_rgb(band.color[0], band.color[1], band.color[2])
}

fn clamp_band_thresholds(bands: &mut [ColorBand]) {
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

fn draw_band_range_slider(ui: &mut egui::Ui, bands: &mut [ColorBand]) -> bool {
    if bands.len() < 2 {
        ui.label("Need at least two bands");
        return false;
    }

    let max_density = bands.last().map(|b| b.threshold as f32).unwrap_or(30000.0);
    let max_density = max_density.max(1.0);

    let desired_size = egui::vec2(ui.available_width().max(180.0), 78.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let track_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, rect.bottom() - 28.0),
        egui::pos2(rect.right() - 8.0, rect.bottom() - 16.0),
    );

    let to_x =
        |value: f32| -> f32 { track_rect.left() + (value / max_density) * track_rect.width() };
    let to_value = |x: f32| -> i32 {
        let t = ((x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
        (t * max_density).round() as i32
    };

    let handle_count = bands.len() - 1;
    let id = response.id.with("band_handles");

    if (response.drag_started() || response.clicked()) && response.interact_pointer_pos().is_some()
    {
        let pointer_x = response.interact_pointer_pos().unwrap().x;
        let mut nearest_idx = 0usize;
        let mut nearest_dist = f32::INFINITY;
        for (i, band) in bands.iter().take(handle_count).enumerate() {
            let hx = to_x(band.threshold as f32);
            let dist = (pointer_x - hx).abs();
            if dist < nearest_dist {
                nearest_dist = dist;
                nearest_idx = i;
            }
        }
        ui.memory_mut(|m| m.data.insert_temp(id, nearest_idx));
    }

    if response.drag_stopped() {
        ui.memory_mut(|m| {
            m.data.remove::<usize>(id);
        });
    }

    let mut changed = false;
    if (response.dragged() || response.clicked()) && response.interact_pointer_pos().is_some() {
        if let Some(active_idx) = ui.memory(|m| m.data.get_temp::<usize>(id)) {
            let pointer_x = response.interact_pointer_pos().unwrap().x;
            let mut value = to_value(pointer_x);

            let lower = if active_idx == 0 {
                0
            } else {
                bands[active_idx - 1].threshold as i32
            };
            let upper = if active_idx + 1 < handle_count {
                bands[active_idx + 1].threshold as i32
            } else {
                max_density as i32
            };
            value = value.clamp(lower, upper);

            if bands[active_idx].threshold != value as u16 {
                bands[active_idx].threshold = value as u16;
                changed = true;
            }
        }
    }

    if changed {
        clamp_band_thresholds(bands);
        response.mark_changed();
    }

    painter.rect_filled(track_rect, 3.0, ui.visuals().widgets.inactive.bg_fill);

    for i in 0..bands.len() {
        let start = if i == 0 { 0u16 } else { bands[i - 1].threshold };
        let end = if i == bands.len() - 1 {
            max_density as u16
        } else {
            bands[i].threshold
        };
        let sx = to_x(start as f32);
        let ex = to_x(end as f32);
        if ex <= sx {
            continue;
        }

        let mut color = band_color(&bands[i]);
        if bands[i].is_transparent {
            color = color.gamma_multiply(0.25);
        }

        let seg_rect = egui::Rect::from_min_max(
            egui::pos2(sx, track_rect.top()),
            egui::pos2(ex, track_rect.bottom()),
        );
        painter.rect_filled(seg_rect, 2.0, color);

        let seg_width = ex - sx;
        if seg_width > 28.0 {
            painter.text(
                egui::pos2((sx + ex) * 0.5, track_rect.bottom() + 6.0),
                egui::Align2::CENTER_TOP,
                bands[i].name,
                egui::FontId::proportional(10.0),
                ui.visuals().text_color(),
            );
        }
    }

    for i in 0..handle_count {
        let hx = to_x(bands[i].threshold as f32);
        let center = egui::pos2(hx, track_rect.center().y);
        let active = ui.memory(|m| m.data.get_temp::<usize>(id)) == Some(i) && response.dragged();
        let stroke_color = if active {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().widgets.inactive.fg_stroke.color
        };

        painter.circle_filled(center, 6.0, ui.visuals().extreme_bg_color);
        painter.circle_stroke(center, 6.0, egui::Stroke::new(1.5, stroke_color));
        painter.text(
            egui::pos2(hx, track_rect.top() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{}", bands[i].threshold),
            egui::FontId::monospace(10.0),
            ui.visuals().text_color(),
        );
    }

    changed
}

/// Draw the full three-panel UI.
fn draw_ui(ctx: &egui::Context, ui: &mut UiState) {
    draw_left_sidebar(ctx, ui);
    draw_right_panel(ctx, ui);
    // The remaining central area is where the 3D wgpu renderer draws.
    draw_bottom_controls(ctx, ui);
}

/// Left sidebar: metadata, layers, camera presets.
fn draw_left_sidebar(ctx: &egui::Context, ui: &mut UiState) {
    egui::SidePanel::left("sidebar")
        .default_width(220.0)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical().show(panel, |panel| {
                panel.heading("roxel");
                panel.separator();

                // File section.
                if panel.button("Open file...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("DICOS", &["dcs", "dcm"])
                        .pick_file()
                    {
                        ui.file_to_load = Some(path);
                    }
                }
                if panel.button("Open folder...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        ui.file_to_load = Some(path);
                    }
                }

                if let Some(path) = &ui.loaded_path {
                    panel.label(format!(
                        "{}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }

                panel.separator();

                // Metadata section.
                if !ui.volumes.is_empty() {
                    panel.heading("Metadata");
                    if let Some(layer) = ui.volumes.first() {
                        let vol = &layer.volume;
                        panel.label(format!(
                            "{}x{}x{} ({})",
                            vol.dim_x, vol.dim_y, vol.dim_z, vol.modality
                        ));
                    }
                    panel.label(format!("{} volume(s)", ui.volumes.len()));
                    panel.separator();
                }

                // Layers section -- selecting a layer uploads it to both 3D and 2D.
                if ui.volumes.len() > 1 {
                    panel.heading("Layers");
                    let active = ui.active_3d_index.unwrap_or(0);
                    for i in 0..ui.volumes.len() {
                        let is_active = i == active;
                        let name = ui.volumes[i].name.clone();
                        if panel.selectable_label(is_active, &name).clicked() && !is_active {
                            ui.upload_3d_index = Some(i);
                            ui.slice_view.volume_index = i;
                            ui.slice_view.update_for_volume(&ui.volumes[i].volume);
                            ui.slice_view.window_center = ui.volumes[i].volume.window_center as f32;
                            ui.slice_view.window_width = ui.volumes[i].volume.window_width as f32;
                            ui.selected_threat = None;
                            ui.slice_dirty = true;
                        }
                    }
                    panel.separator();
                }

                if !ui.volumes.is_empty() {
                    let threat_vol_idx = ui.slice_view.volume_index.min(ui.volumes.len() - 1);
                    panel.heading("Threats");
                    if panel
                        .checkbox(&mut ui.show_threats, "Show threat boxes")
                        .changed()
                    {
                        ui.slice_dirty = true;
                    }

                    if ui.volumes[threat_vol_idx].volume.threats.is_empty() {
                        panel.label("No threats in selected volume");
                        panel.separator();
                    } else {
                        let mut any_changed = false;
                        let mut new_selected = ui.selected_threat;
                        {
                            let threats = &mut ui.volumes[threat_vol_idx].volume.threats;
                            if !matches!(new_selected, Some(i) if i < threats.len()) {
                                new_selected = None;
                            }

                            panel.horizontal(|h| {
                                if h.button("All On").clicked() {
                                    for threat in threats.iter_mut() {
                                        threat.enabled = true;
                                    }
                                    any_changed = true;
                                }
                                if h.button("All Off").clicked() {
                                    for threat in threats.iter_mut() {
                                        threat.enabled = false;
                                    }
                                    any_changed = true;
                                }
                            });

                            if let Some(sel) = new_selected {
                                panel.horizontal(|h| {
                                    if h.button("Only Selected").clicked() {
                                        for (i, threat) in threats.iter_mut().enumerate() {
                                            threat.enabled = i == sel;
                                        }
                                        any_changed = true;
                                    }
                                });
                            }

                            for (i, threat) in threats.iter_mut().enumerate() {
                                let mut label = threat.name.clone();
                                if let Some(conf) = threat.confidence {
                                    label.push_str(&format!(" ({conf:.2})"));
                                }
                                let is_selected = new_selected == Some(i);
                                let mut picked = false;
                                let mut next_enabled = threat.enabled;
                                panel.horizontal(|h| {
                                    let c = egui::Color32::from_rgb(
                                        threat.color[0],
                                        threat.color[1],
                                        threat.color[2],
                                    );
                                    h.colored_label(c, "■");
                                    if h.selectable_label(is_selected, &label).clicked() {
                                        picked = true;
                                    }
                                    h.checkbox(&mut next_enabled, "on");
                                });
                                if picked {
                                    new_selected = Some(i);
                                }
                                if next_enabled != threat.enabled {
                                    threat.enabled = next_enabled;
                                    any_changed = true;
                                }
                            }
                        }
                        ui.selected_threat = new_selected;
                        if any_changed {
                            ui.slice_dirty = true;
                        }
                        panel.separator();
                    }
                }

                // Camera presets.
                panel.heading("View");
                panel.horizontal(|h| {
                    if h.button("Axial").clicked() {
                        ui.camera.set_axial();
                    }
                    if h.button("Coronal").clicked() {
                        ui.camera.set_coronal();
                    }
                    if h.button("Sagittal").clicked() {
                        ui.camera.set_sagittal();
                    }
                });

                panel.separator();
                panel.heading("Interaction Key");
                panel.label("LMB drag: Orbit 3D");
                panel.label("Shift + LMB drag: Pan 3D");
                panel.label("Mouse wheel: Zoom 3D");
                panel.label("Axial/Coronal/Sagittal: Snap view");
            });
        });
}

/// Right panel: 2D slice view and its controls.
fn draw_right_panel(ctx: &egui::Context, ui: &mut UiState) {
    egui::SidePanel::right("slice_panel")
        .default_width(320.0)
        .min_width(200.0)
        .show(ctx, |panel| {
            panel.heading("2D Slice");
            panel.separator();

            // Display the slice image with zoom + scroll.
            if let Some(tex) = &ui.slice_texture {
                let available = panel.available_size();
                let controls_height = 220.0;
                let viewport_h = (available.y - controls_height).max(100.0);
                let tex_size = tex.size_vec2();

                // Compute base scale (fit-to-width at 100%).
                let base_scale = if tex_size.x > 0.0 {
                    available.x / tex_size.x
                } else {
                    1.0
                };
                let scale = base_scale * (ui.slice_view.zoom / 100.0);
                let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);

                egui::ScrollArea::both()
                    .max_height(viewport_h)
                    .show(panel, |scroll| {
                        scroll.image(egui::load::SizedTexture::new(tex.id(), display_size));
                    });
            } else {
                let available = panel.available_size();
                let rect_h = (available.y - 220.0).max(100.0);
                let (rect, _) = panel
                    .allocate_exact_size(egui::vec2(available.x, rect_h), egui::Sense::hover());
                panel
                    .painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_gray(200));
                panel.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "No volume loaded",
                    egui::FontId::proportional(14.0),
                    egui::Color32::GRAY,
                );
            }

            panel.separator();

            // Volume selector dropdown.
            if !ui.volumes.is_empty() {
                let current_name = ui
                    .volumes
                    .get(ui.slice_view.volume_index)
                    .map(|l| l.name.as_str())
                    .unwrap_or("(none)");

                egui::ComboBox::from_label("Volume")
                    .selected_text(current_name)
                    .show_ui(panel, |combo| {
                        for (i, layer) in ui.volumes.iter().enumerate() {
                            if combo
                                .selectable_value(&mut ui.slice_view.volume_index, i, &layer.name)
                                .clicked()
                            {
                                ui.slice_view.update_for_volume(&layer.volume);
                                ui.slice_view.window_center = layer.volume.window_center as f32;
                                ui.slice_view.window_width = layer.volume.window_width as f32;
                                ui.selected_threat = None;
                                ui.slice_dirty = true;
                            }
                        }
                    });
            }

            // Orientation dropdown.
            let prev_orientation = ui.slice_view.orientation;
            egui::ComboBox::from_label("View")
                .selected_text(ui.slice_view.orientation.label())
                .show_ui(panel, |combo| {
                    for orient in Orientation::ALL {
                        combo.selectable_value(
                            &mut ui.slice_view.orientation,
                            orient,
                            orient.label(),
                        );
                    }
                });
            if ui.slice_view.orientation != prev_orientation {
                // Update max_slices for new orientation.
                if let Some(layer) = ui.volumes.get(ui.slice_view.volume_index) {
                    ui.slice_view.update_for_volume(&layer.volume);
                }
                ui.slice_dirty = true;
            }

            // Composite view toggle.
            let prev_composite = ui.slice_view.composite;
            panel.checkbox(&mut ui.slice_view.composite, "Composite View");
            if ui.slice_view.composite != prev_composite {
                ui.slice_dirty = true;
            }

            // Window/Level controls (debounced -- only re-render on drag end).
            let wl_r = panel.add(
                egui::Slider::new(&mut ui.slice_view.window_center, 0.0..=65535.0).text("W/L"),
            );
            if slider_committed(&wl_r) {
                ui.slice_dirty = true;
            }
            let ww_r = panel
                .add(egui::Slider::new(&mut ui.slice_view.window_width, 1.0..=65536.0).text("W/W"));
            if slider_committed(&ww_r) {
                ui.slice_dirty = true;
            }

            // Slice slider (hidden in composite mode).
            if !ui.slice_view.composite && ui.slice_view.max_slices > 1 {
                let max = ui.slice_view.max_slices - 1;
                if panel
                    .add(egui::Slider::new(&mut ui.slice_view.slice_index, 0..=max).text("Slice"))
                    .changed()
                {
                    ui.slice_dirty = true;
                }
            }

            // Zoom slider.
            panel.add(egui::Slider::new(&mut ui.slice_view.zoom, 50.0..=500.0).text("Zoom %"));
        });
}

/// Bottom panel: 3D rendering controls (quality, transfer function, lighting).
fn draw_bottom_controls(ctx: &egui::Context, ui: &mut UiState) {
    egui::TopBottomPanel::bottom("controls")
        .default_height(280.0)
        .resizable(true)
        .show(ctx, |panel| {
            egui::ScrollArea::vertical().show(panel, |panel| {
                panel.columns(3, |cols| {
                    // Column 1: Rendering quality and window/level.
                    cols[0].heading("Rendering");
                    cols[0].horizontal(|h| {
                        h.label("Quality:");
                        if h.selectable_label(ui.quality == Quality::Fast, "Fast")
                            .clicked()
                        {
                            ui.quality = Quality::Fast;
                        }
                        if h.selectable_label(ui.quality == Quality::Medium, "Med")
                            .clicked()
                        {
                            ui.quality = Quality::Medium;
                        }
                        if h.selectable_label(ui.quality == Quality::High, "High")
                            .clicked()
                        {
                            ui.quality = Quality::High;
                        }
                    });
                    cols[0].add(egui::Slider::new(&mut ui.window_center, 0.0..=65535.0).text("WC"));
                    cols[0].add(egui::Slider::new(&mut ui.window_width, 1.0..=65536.0).text("WW"));
                    cols[0]
                        .add(egui::Slider::new(&mut ui.global_opacity, 0.0..=1.0).text("Opacity"));
                    cols[0].add(
                        egui::Slider::new(&mut ui.density_threshold, 0.0..=1.0).text("Density"),
                    );

                    // Column 2: Transfer function and material bands.
                    cols[1].heading("Transfer");
                    cols[1].horizontal(|h| {
                        if h.selectable_label(ui.preset == TransferPreset::Default, "Default")
                            .clicked()
                        {
                            ui.preset = TransferPreset::Default;
                            ui.preset_changed = true;
                        }
                        if h.selectable_label(ui.preset == TransferPreset::Threat, "Threat")
                            .clicked()
                        {
                            ui.preset = TransferPreset::Threat;
                            ui.preset_changed = true;
                        }
                        if h.selectable_label(ui.preset == TransferPreset::Monochrome, "Mono")
                            .clicked()
                        {
                            ui.preset = TransferPreset::Monochrome;
                            ui.preset_changed = true;
                        }
                    });

                    if ui.preset == TransferPreset::Default {
                        clamp_band_thresholds(&mut ui.bands);
                        cols[1].label("Band thresholds");
                        if draw_band_range_slider(&mut cols[1], &mut ui.bands) {
                            ui.bands_changed = true;
                        }
                        cols[1].add_space(6.0);
                        cols[1].label("Band alpha");

                        for band in &mut ui.bands {
                            cols[1].horizontal(|h| {
                                h.colored_label(band_color(band), "■");
                                h.label(band.name);
                                let ar = h.add(
                                    egui::Slider::new(&mut band.alpha, 0.0..=2.0).show_value(false),
                                );
                                h.label(format!("{:.0}%", band.alpha * 100.0));
                                if slider_committed(&ar) {
                                    ui.bands_changed = true;
                                }
                            });
                        }
                    }

                    if ui.preset != TransferPreset::Default {
                        // Keep spacing roughly consistent with default preset layout.
                        cols[1].add_space(12.0);
                    }

                    // Column 3: Lighting.
                    cols[2].heading("Lighting");
                    cols[2].add(egui::Slider::new(&mut ui.ambient, 0.0..=1.0).text("Ambient"));
                    cols[2].add(egui::Slider::new(&mut ui.diffuse, 0.0..=1.0).text("Diffuse"));
                    cols[2].add(egui::Slider::new(&mut ui.specular, 0.0..=1.0).text("Specular"));
                });
            });
        });
}
