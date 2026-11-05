//! roxel -- GPU-accelerated DICOS volume viewer.
//!
//! Usage:
//!   roxel                      # Launch with empty viewport
//!   roxel scan.dcs             # Load a single DICOS file
//!   roxel /path/to/slices/     # Load a directory of .dcs files as a volume

use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

struct RoxelApp {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    app: Option<roxel::app::App>,
    /// Path to load on startup (from CLI args).
    initial_path: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    settled_frames: u32,
}

impl RoxelApp {
    fn new(initial_path: Option<PathBuf>) -> Self {
        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            app: None,
            initial_path,
            screenshot: std::env::var_os("ROXEL_SCREENSHOT").map(PathBuf::from),
            settled_frames: 0,
        }
    }

    fn init_gpu(&mut self, window: Arc<Window>) {
        match Self::try_init_gpu(&window) {
            Ok(gpu) => {
                let device = Arc::new(gpu.device);
                let queue = Arc::new(gpu.queue);

                let mut app =
                    roxel::app::App::new(&window, device.clone(), queue.clone(), gpu.config.format);

                if self.screenshot.is_some()
                    && std::env::var_os("ROXEL_SCREENSHOT_EXPANDED").is_some()
                {
                    app.expand_slice_view();
                }

                if self.screenshot.is_some() && std::env::var_os("ROXEL_SCREENSHOT_FOCUS").is_some()
                {
                    app.focus_volume();
                }
                if self.screenshot.is_some()
                    && std::env::var_os("ROXEL_SCREENSHOT_CUTAWAY").is_some()
                {
                    app.enable_cutaway();
                }
                // Load the initial path if provided via CLI.
                if let Some(path) = self.initial_path.take() {
                    app.load_path(&path);
                }

                self.window = Some(window);
                self.surface = Some(gpu.surface);
                self.device = Some(device);
                self.queue = Some(queue);
                self.surface_config = Some(gpu.config);
                self.app = Some(app);
            }
            Err(message) => {
                show_gpu_error(&message);
                std::process::exit(1);
            }
        }
    }

    /// Attempt to initialize the GPU surface, adapter, and device.
    ///
    /// Returns a descriptive error message (rather than panicking) on
    /// failure so the caller can present it to the user before exiting.
    fn try_init_gpu(window: &Arc<Window>) -> Result<GpuInit, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| {
            format!(
                "Failed to create a GPU rendering surface for the window:\n{e}\n\n\
                 This usually means your graphics drivers do not support any of \
                 roxel's supported backends (Vulkan, Metal, or DX12). Try updating \
                 your graphics drivers."
            )
        })?;

        let adapter = Self::request_adapter(&instance, &surface)?;

        let required_features =
            wgpu::Features::TEXTURE_FORMAT_16BIT_NORM | wgpu::Features::FLOAT32_FILTERABLE;
        let required_limits = wgpu::Limits::default();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("roxel_device"),
            required_features,
            required_limits: required_limits.clone(),
            memory_hints: Default::default(),
            trace: Default::default(),
            experimental_features: Default::default(),
        }))
        .map_err(|e| {
            format!(
                "Failed to create a GPU device:\n{e}\n\n\
                 roxel requires a GPU/driver supporting the features \
                 {required_features:?} and at least the default wgpu resource \
                 limits ({required_limits:?}). Please update your graphics \
                 drivers, or try running on a different GPU if more than one \
                 is available."
            )
        })?;

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer non-sRGB format (egui prefers Rgba8Unorm/Bgra8Unorm).
        // Fall back to sRGB if no non-sRGB format is available.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .or_else(|| surface_caps.formats.first())
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(GpuInit {
            surface,
            device,
            queue,
            config,
        })
    }

    /// Request a GPU adapter, retrying once against a software fallback
    /// adapter if the preferred high-performance adapter is unavailable.
    fn request_adapter(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'static>,
    ) -> Result<wgpu::Adapter, String> {
        let primary = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        }));

        if let Ok(adapter) = primary {
            return Ok(adapter);
        }

        // Retry once against a fallback (software) adapter before giving up.
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(surface),
            force_fallback_adapter: true,
        }))
        .map_err(|e| {
            format!(
                "Failed to find a compatible GPU adapter, even with a software \
                 fallback adapter:\n{e}\n\n\
                 roxel requires a GPU (or software renderer) reachable via \
                 Vulkan, Metal, or DX12. Please check that your graphics \
                 drivers are installed and up to date."
            )
        })
    }
}

/// Resources produced by a successful GPU initialization.
struct GpuInit {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

/// Show a blocking error dialog describing a fatal GPU initialization
/// failure.
fn show_gpu_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title("roxel")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

impl ApplicationHandler for RoxelApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("Roxel — Volume workspace")
            .with_min_inner_size(winit::dpi::LogicalSize::new(1100, 720))
            .with_inner_size(capture_window_size());

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        self.init_gpu(window);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => return,
        };

        if let Some(app) = &mut self.app {
            if app.handle_event(&window, &event) {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let (Some(surface), Some(config), Some(device)) =
                    (&self.surface, &mut self.surface_config, &self.device)
                {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, config);
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let (surface, config, app) =
                    match (&self.surface, &self.surface_config, &mut self.app) {
                        (Some(s), Some(c), Some(a)) => (s, c, a),
                        _ => return,
                    };

                let output = match surface.get_current_texture() {
                    Ok(t) => t,
                    Err(wgpu::SurfaceError::Lost) => {
                        if let Some(device) = &self.device {
                            surface.configure(device, config);
                        }
                        return;
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        self.screenshot = None;
                        event_loop.exit();
                        return;
                    }
                    Err(_) => return,
                };

                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let wants_repaint = app.render(&window, &view, config.width, config.height);
                if self.screenshot.is_some() {
                    if app.is_loading() {
                        self.settled_frames = 0;
                    } else {
                        self.settled_frames += 1;
                    }
                    if self.settled_frames >= 6 {
                        let device = self.device.as_ref().unwrap();
                        let queue = self.queue.as_ref().unwrap();
                        let texture = device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("screenshot"),
                            size: wgpu::Extent3d {
                                width: config.width,
                                height: config.height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: config.format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_SRC,
                            view_formats: &[],
                        });
                        app.render(
                            &window,
                            &texture.create_view(&Default::default()),
                            config.width,
                            config.height,
                        );
                        save_screenshot(device, queue, &texture, self.screenshot.as_ref().unwrap());
                        self.screenshot = None;
                        event_loop.exit();
                    } else {
                        window.request_redraw();
                    }
                }
                output.present();
                if wants_repaint {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::init();

    // Accept an optional path argument (file or directory).
    let initial_path = std::env::args().nth(1).map(PathBuf::from);

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = RoxelApp::new(initial_path);
    event_loop.run_app(&mut app).expect("event loop error");
}

/// Read back an actual application frame, including the volume and egui overlay.
fn save_screenshot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    path: &std::path::Path,
) {
    let width = texture.width();
    let height = texture.height();
    let stride = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot readback"),
        size: stride as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU screenshot readback");
    rx.recv().unwrap().expect("screenshot mapping");
    let mapped = buffer.slice(..).get_mapped_range();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks(stride as usize) {
        rgba.extend_from_slice(&row[..width as usize * 4]);
    }
    if matches!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(path).expect("screenshot output")),
        width,
        height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgba)
        .unwrap();
    eprintln!("Screenshot saved: {}", path.display());
}

fn capture_window_size() -> winit::dpi::LogicalSize<u32> {
    // Optional reproducible review viewport; ordinary launches keep the default.
    if std::env::var_os("ROXEL_SCREENSHOT").is_some() {
        if let Ok(value) = std::env::var("ROXEL_SCREENSHOT_SIZE") {
            if let Some((w, h)) = value.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    return winit::dpi::LogicalSize::new(w.clamp(1100, 3840), h.clamp(720, 2160));
                }
            }
        }
    }
    winit::dpi::LogicalSize::new(1440, 900)
}
