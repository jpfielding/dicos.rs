//! roxel -- GPU-accelerated DICOS volume viewer.
//!
//! Usage:
//!   roxel                      # Launch with empty viewport
//!   roxel scan.dcs             # Load a single DICOS file
//!   roxel /path/to/slices/     # Load a directory of .dcs files as a volume
//!
//! On `wasm32` the same binary runs in the browser (WebGPU) and attaches to the
//! page's `<canvas id="roxel">`.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Events sent to the event loop from async tasks.
#[cfg(target_arch = "wasm32")]
enum UserEvent {
    /// Async GPU initialization finished.
    GpuReady(Result<GpuInit, String>),
}

#[cfg(target_arch = "wasm32")]
type AppEvent = UserEvent;
#[cfg(not(target_arch = "wasm32"))]
type AppEvent = ();

struct RoxelApp {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    app: Option<roxel::app::App>,
    /// When egui asked for a delayed repaint (debounce, animation), if any.
    repaint_at: Option<Instant>,
    /// Path to load on startup (from CLI args).
    #[cfg(not(target_arch = "wasm32"))]
    initial_path: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    screenshot: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    settled_frames: u32,
    /// Used to hand the async GPU init result back to the event loop.
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
}

impl RoxelApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(initial_path: Option<PathBuf>) -> Self {
        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            app: None,
            repaint_at: None,
            initial_path,
            screenshot: std::env::var_os("ROXEL_SCREENSHOT").map(PathBuf::from),
            settled_frames: 0,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn new(proxy: winit::event_loop::EventLoopProxy<UserEvent>) -> Self {
        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            app: None,
            repaint_at: None,
            proxy,
        }
    }

    /// Install the result of GPU initialization: build the `App` on success,
    /// or report the error to the user.
    fn finish_init(&mut self, window: Arc<Window>, result: Result<GpuInit, String>) {
        let gpu = match result {
            Ok(gpu) => gpu,
            Err(message) => {
                report_gpu_error(&message);
                return;
            }
        };
        let device = Arc::new(gpu.device);
        let queue = Arc::new(gpu.queue);

        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut app =
            roxel::app::App::new(&window, device.clone(), queue.clone(), gpu.config.format);

        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.screenshot.is_some() && std::env::var_os("ROXEL_SCREENSHOT_EXPANDED").is_some()
            {
                app.expand_slice_view();
            }

            if self.screenshot.is_some() && std::env::var_os("ROXEL_SCREENSHOT_FOCUS").is_some() {
                app.focus_volume();
            }
            if self.screenshot.is_some() && std::env::var_os("ROXEL_SCREENSHOT_CUTAWAY").is_some() {
                app.enable_cutaway();
            }
            // Load the initial path if provided via CLI.
            if let Some(path) = self.initial_path.take() {
                app.load_path(&path);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            app.set_redraw_waker(window.clone());
            // Don't greet first-time visitors with an empty screen.
            roxel::web::load_sample();
        }

        self.window = Some(window);
        self.surface = Some(gpu.surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(gpu.config);
        self.app = Some(app);
    }

    /// Initialize the GPU surface, adapter, and device.
    ///
    /// Returns a descriptive error message (rather than panicking) on
    /// failure so the caller can present it to the user before exiting.
    async fn init_gpu_async(window: Arc<Window>) -> Result<GpuInit, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: BACKENDS,
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

        let adapter = Self::request_adapter(&instance, &surface).await?;

        // Both the volume LUT sampler and WebGPU need filterable float32
        // textures. 16-bit norm is optional: without it the volume falls back
        // to Rgba16Float (see `VolumeRenderer`).
        let adapter_features = adapter.features();
        if !adapter_features.contains(wgpu::Features::FLOAT32_FILTERABLE) {
            return Err(format!(
                "Your GPU/browser does not support the {:?} feature, which roxel \
                 requires for its transfer-function lookup. Please update your \
                 graphics drivers or browser, or try a different GPU.",
                wgpu::Features::FLOAT32_FILTERABLE
            ));
        }
        let required_features = wgpu::Features::FLOAT32_FILTERABLE
            | (adapter_features & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
        let required_limits = wgpu::Limits::default();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("roxel_device"),
                required_features,
                required_limits: required_limits.clone(),
                memory_hints: Default::default(),
                trace: Default::default(),
                experimental_features: Default::default(),
            })
            .await
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
    async fn request_adapter(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'static>,
    ) -> Result<wgpu::Adapter, String> {
        let primary = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
            })
            .await;

        if let Ok(adapter) = primary {
            return Ok(adapter);
        }

        // Retry once against a fallback (software) adapter before giving up.
        instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(surface),
                force_fallback_adapter: true,
            })
            .await
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

#[cfg(not(target_arch = "wasm32"))]
const BACKENDS: wgpu::Backends = wgpu::Backends::all();
#[cfg(target_arch = "wasm32")]
const BACKENDS: wgpu::Backends = wgpu::Backends::BROWSER_WEBGPU;

/// Present a fatal GPU initialization failure to the user.
#[cfg(not(target_arch = "wasm32"))]
fn report_gpu_error(message: &str) {
    native::show_gpu_error(message);
    std::process::exit(1);
}

#[cfg(target_arch = "wasm32")]
fn report_gpu_error(message: &str) {
    roxel::web::show_error(message);
}

impl ApplicationHandler<AppEvent> for RoxelApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        let attrs = WindowAttributes::default()
            .with_title("Roxel — Volume workspace")
            .with_min_inner_size(native::min_window_size())
            .with_inner_size(native::capture_window_size());
        // On the web the canvas is sized by the page's CSS.
        #[cfg(target_arch = "wasm32")]
        let attrs = {
            use winit::platform::web::WindowAttributesExtWebSys;
            let attrs = WindowAttributes::default().with_title("Roxel — Volume workspace");
            match roxel::web::find_canvas() {
                Some(canvas) => attrs.with_canvas(Some(canvas)),
                None => attrs.with_append(true),
            }
        };

        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                report_gpu_error(&format!("Failed to create a window: {e}"));
                return;
            }
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = pollster::block_on(Self::init_gpu_async(window.clone()));
            self.window = Some(window.clone());
            self.finish_init(window, result);
        }
        // Browsers can't block: initialize asynchronously and finish in
        // `user_event`. Until then `window_event` has no surface or app to
        // act on, so redraws and resizes are ignored.
        #[cfg(target_arch = "wasm32")]
        {
            self.window = Some(window.clone());
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = Self::init_gpu_async(window).await;
                let _ = proxy.send_event(UserEvent::GpuReady(result));
            });
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::GpuReady(result) = event;
        let Some(window) = self.window.clone() else {
            return;
        };
        self.finish_init(window.clone(), result);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => return,
        };

        // egui-winit reports `repaint` for RedrawRequested itself; acting on it
        // made every frame request the next, rendering flat out while idle.
        // The frame's own repaint decision comes from `App::render`.
        if let Some(app) = &mut self.app {
            if !matches!(event, WindowEvent::RedrawRequested) && app.handle_event(&window, &event) {
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
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.screenshot = None;
                        }
                        event_loop.exit();
                        return;
                    }
                    Err(_) => return,
                };

                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let repaint_delay = app.render(&window, &view, config.width, config.height);
                #[cfg(not(target_arch = "wasm32"))]
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
                        native::save_screenshot(
                            device,
                            queue,
                            &texture,
                            self.screenshot.as_ref().unwrap(),
                        );
                        self.screenshot = None;
                        event_loop.exit();
                    } else {
                        window.request_redraw();
                    }
                }
                output.present();
                if repaint_delay.is_zero() {
                    self.repaint_at = None;
                    window.request_redraw();
                } else {
                    // `Duration::MAX` (idle) overflows: wait for input instead.
                    self.repaint_at = Instant::now().checked_add(repaint_delay);
                }
            }
            _ => {}
        }
    }

    /// Fire a delayed repaint once its deadline passes; otherwise sleep until
    /// the deadline or the next event, without busy-looping.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.repaint_at {
            Some(at) if Instant::now() >= at => {
                self.repaint_at = None;
                event_loop.set_control_flow(ControlFlow::Wait);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::init();

    // Accept an optional path argument (file or directory).
    let initial_path = std::env::args().nth(1).map(PathBuf::from);

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = RoxelApp::new(initial_path);
    event_loop.run_app(&mut app).expect("event loop error");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use winit::platform::web::EventLoopExtWebSys;

    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    let proxy = event_loop.create_proxy();
    event_loop.spawn_app(RoxelApp::new(proxy));
}

/// Native-only helpers: error dialogs and reproducible screenshot capture.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    /// Show a blocking error dialog describing a fatal GPU initialization
    /// failure.
    pub(super) fn show_gpu_error(message: &str) {
        rfd::MessageDialog::new()
            .set_title("roxel")
            .set_description(message)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }

    /// Read back an actual application frame, including the volume and egui overlay.
    pub(super) fn save_screenshot(
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

    /// Minimum window size; a review capture may request a smaller (phone) viewport.
    pub(super) fn min_window_size() -> winit::dpi::LogicalSize<u32> {
        let capture = capture_window_size();
        winit::dpi::LogicalSize::new(capture.width.min(1100), capture.height.min(720))
    }

    pub(super) fn capture_window_size() -> winit::dpi::LogicalSize<u32> {
        // Optional reproducible review viewport; ordinary launches keep the default.
        if std::env::var_os("ROXEL_SCREENSHOT").is_some() {
            if let Ok(value) = std::env::var("ROXEL_SCREENSHOT_SIZE") {
                if let Some((w, h)) = value.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        return winit::dpi::LogicalSize::new(
                            w.clamp(320, 3840),
                            h.clamp(320, 2160),
                        );
                    }
                }
            }
        }
        winit::dpi::LogicalSize::new(1440, 900)
    }
}
