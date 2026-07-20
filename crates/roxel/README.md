# roxel

GPU-accelerated DICOS volume viewer with 3D ray-casting and 2D slice viewing.

![roxel 3D Rendering](https://github.com/jpfielding/dicos.rs/releases/download/v2.0.0/roxel-lrg.gif)

Renders 3D CT volumes from DICOS files using wgpu ray-casting with an egui
control panel. Supports multi-volume loading, five-band material classification
transfer functions for security screening visualization, and CPU-rendered 2D
slice/composite projection views.

## Layout

```
┌──────────┬──────────────────────┬──────────────────────┐
│ Sidebar  │   3D Volume View     │   2D Slice View      │
│          │   (ray-caster)       │   (CPU rendered)     │
│ Metadata │                      │                      │
│ Layers   ├──────────────────────┼──────────────────────┤
│ Threats  │ Quality | Opacity    │ Volume: [dropdown]   │
│          │ Preset | Bands       │ View:   [dropdown]   │
│          │ Lighting | WC/WW     │ Composite [x]        │
│          │                      │ W/L: __ W/W: __      │
│          │                      │ Slice: [slider]      │
└──────────┴──────────────────────┴──────────────────────┘
```

## Running

```sh
# Launch with empty viewport (use File > Open to load)
cargo run -p roxel --release

# Load a single DICOS file
cargo run -p roxel --release -- scan.dcs

# Load a directory of .dcs files as separate volume layers
cargo run -p roxel --release -- /path/to/slices/
```

The `--release` flag is recommended for interactive use -- debug builds are
noticeably slower during ray marching.

### Single File Loading

When a single `.dcs` or `.dcm` file is provided, its frames are extracted into
one 3D volume. Multi-frame files produce a volume with depth equal to the frame
count. Single-frame files produce a 1-slice volume.

### Directory Loading (Multi-Volume)

When a directory is provided, each `.dcs`/`.dcm` file in the directory becomes
a separate named volume layer. Files are sorted alphabetically. Each layer
appears in the sidebar's Layers section with its own enable/disable checkbox
and a "3D" button to upload that specific volume to the GPU renderer.

This matches the Go viewer's approach where volumes from a directory are kept
as separate layers rather than stacked into a single volume. The 2D slice panel
provides a dropdown to select which volume to view.

## Controls

### Mouse

| Action | Effect |
|--------|--------|
| **Left-drag** in the 3D viewport | Rotate the volume (arcball camera) |
| **Scroll wheel** | Zoom in/out (clamped 0.5x - 5.0x) |

### Keyboard

No keyboard shortcuts are currently bound. All controls are in the UI panels.

## Panel Descriptions

### Left Sidebar

The left sidebar (220px default width) provides file loading and volume
management.

| Section | Controls |
|---------|----------|
| **File** | "Open file..." button (filters `.dcs`/`.dcm`), "Open folder..." button for directory loading |
| **Metadata** | Volume dimensions (XxYxZ), modality string, volume count |
| **Layers** | Per-layer checkbox (enable/disable) and "3D" upload button (visible when multiple volumes are loaded) |
| **View** | Axial / Coronal / Sagittal camera preset buttons |

### Right Panel (2D Slice View)

The right panel (320px default, 200px minimum) displays a CPU-rendered 2D
slice from the loaded volume. No GPU is required for this panel.

| Control | Description |
|---------|-------------|
| **Volume** dropdown | Select which loaded volume to view (multi-volume mode) |
| **View** dropdown | Slice orientation: Axial (XY), Coronal (XZ), Sagittal (YZ) |
| **Composite View** checkbox | Toggle MIP (Maximum Intensity Projection) mode |
| **W/L** slider | Window center (0 - 65535) |
| **W/W** slider | Window width (1 - 65536) |
| **Slice** slider | Slice index within the chosen orientation (hidden in composite mode) |

When composite mode is enabled, the viewer projects along the selected
orientation axis and takes the maximum voxel value at each pixel position
across all slices, then applies window/level normalization.

### Bottom Panel (3D Rendering Controls)

The bottom panel contains 3D rendering controls arranged in three columns.

#### Column 1: Rendering

| Control | Description |
|---------|-------------|
| **Quality** | Fast / Medium / High ray-march step size |
| **WC** slider | Window center for 3D rendering (0 - 65535) |
| **WW** slider | Window width for 3D rendering (1 - 65536) |
| **Opacity** slider | Global alpha scale (0.0 - 1.0) |
| **Density** slider | Density threshold cutoff (0.0 - 1.0) |

#### Column 2: Transfer Function

| Control | Description |
|---------|-------------|
| **Preset** buttons | Default (Bands) / Threat / Mono |
| **Band thresholds** | Per-band density sliders (Default preset only) |
| **Band alpha** | Per-band opacity sliders (Default preset only) |

#### Column 3: Lighting

| Control | Description |
|---------|-------------|
| **Ambient** slider | Ambient light intensity (0.0 - 1.0, default 0.3) |
| **Diffuse** slider | Diffuse light intensity (0.0 - 1.0, default 0.6) |
| **Specular** slider | Specular highlight intensity (0.0 - 1.0, default 0.3) |

## Transfer Function Presets

| Preset | Description | Behavior |
|--------|-------------|----------|
| **Default** | Five-band material classification | Five color bands with per-band thresholds and opacity. Density-based gradient interpolation within each band. |
| **Threat** | Red monochrome for threat highlighting | Transparent below density ~700, red ramp above with opacity 0.03 - 0.20. |
| **Mono** | Grayscale density mapping | Linear grayscale ramp from black to 67% white, with linear alpha ramp to 50%. |

### Default Material Bands

| Band | Name | Threshold | Color (RGB) | Alpha | Description |
|------|------|-----------|-------------|-------|-------------|
| 0 | Air | 8000 | 250, 200, 110 | 0.00 | Transparent, density cutoff |
| 1 | Organic | 15000 | 230, 150, 50 | 0.08 | Orange -- low-Z materials |
| 2 | Inorganic | 20000 | 80, 200, 40 | 0.15 | Green -- medium-Z materials |
| 3 | Metal | 25000 | 15, 165, 200 | 0.25 | Blue -- high-Z materials |
| 4 | Dense | 30000 | 40, 48, 180 | 0.35 | Dark blue -- very dense materials |

Thresholds are tuned for raw CT density data (0 - 30000 range). Within each
band, a brightness gradient (0.85 - 1.15) and opacity ramp (30% - 100% of band
alpha) are applied based on position within the band.

## Architecture

```
src/
  main.rs         -- winit event loop, wgpu initialization
  app.rs          -- egui application state, three-panel UI layout, multi-volume file loading
  renderer.rs     -- wgpu render pipeline, uniform buffer, texture management
  raycast.wgsl    -- WGSL fragment shader: ray-box, volume sampling, transfer fn, Phong, alpha blending
  camera.rs       -- arcball camera (orbit, zoom, preset views)
  transfer.rs     -- material color bands, transfer function generation
  volume.rs       -- DICOS file loading, volume extraction, gradient computation
  slice_view.rs   -- CPU 2D slice renderer (single slice + MIP composite)
```

### Rendering Pipeline

1. **Load** -- Parse DICOS file via the `dicos` crate, extract pixel data
   across all frames into a `Volume` struct (uint16 voxels, 0 - 65535).

2. **Gradient computation** -- Compute per-voxel gradients on CPU using central
   differences. Each voxel becomes 4 normalized `u16` channels:
   `[density, grad_x+0.5, grad_y+0.5, grad_z+0.5]` packed as `RGBA16Unorm`.
   Gradients are offset by 0.5 to fit unsigned texture range.

3. **Volume upload** -- Upload the packed data as an `Rgba16Unorm` 3D texture
   to the GPU. Dimensions match the volume exactly.

4. **Transfer function upload** -- Generate a 1024-entry RGBA lookup table from
   the selected preset or user-modified bands. Upload as an `Rgba32Float` 1D
   texture.

5. **Per-frame rendering**:
   - Update uniform buffer with camera position/orientation, window/level,
     lighting parameters, step size, and Z-scale factor.
   - Draw a full-screen triangle (3 vertices, no vertex buffers).
   - Fragment shader ray-marches from camera through the volume bounding box:
     - Ray-box intersection to find entry/exit distances.
     - Step through the volume sampling the 3D texture with trilinear filtering.
     - Apply window/level normalization and density threshold clipping.
     - Look up color and alpha from the 1D transfer function texture.
     - Compute Phong lighting (ambient + diffuse + Blinn-Phong specular) using
       pre-computed gradients as surface normals. Two light sources: main
       (0.5, 0.7, 1.0) and fill (-0.3, 0.2, 0.8).
     - Edge enhancement: scale alpha by gradient magnitude.
     - Front-to-back alpha blending, early termination at alpha > 0.98.
   - Render egui overlay on top of the 3D viewport.

6. **2D slice rendering** -- Runs on CPU independently of the GPU pipeline.
   Samples the volume along the selected orientation and applies window/level
   normalization to produce a grayscale `ColorImage` uploaded to an egui
   texture.

### Quality Levels

The ray-march step size adapts to voxel size, zoom level, and quality setting:

| Quality | Step Multiplier | Description |
|---------|----------------|-------------|
| **Fast** | 1.5x base | Coarse steps, lower fidelity, faster frame rate |
| **Medium** | 1.0x base | Balanced (default) |
| **High** | 0.5x base | Fine steps, highest fidelity, slower frame rate |

Base step size is `1.0 / max_dimension`, scaled by inverse zoom distance,
clamped to `[0.0005, 0.02]`. Maximum 4096 steps per ray.

## GPU Requirements

Requires a GPU with wgpu support:

| Platform | Backend |
|----------|---------|
| macOS | Metal (automatic) |
| Linux | Vulkan |
| Windows | Vulkan or DX12 |

The shader requires:
- 3D texture sampling with trilinear filtering
- 1D texture sampling for the transfer function
- `Rgba16Unorm` (volume) and `Rgba32Float` (transfer function) texture support

The adapter is requested with `HighPerformance` power preference. No optional
wgpu features are required beyond the defaults.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dicos` | DICOS file parsing and codec support (with `all-codecs` feature) |
| `wgpu` | GPU compute and rendering |
| `winit` | Window creation and event handling |
| `egui` / `egui-wgpu` / `egui-winit` | Immediate-mode UI |
| `glam` | 3D math (Vec3 for camera and center-of-mass) |
| `bytemuck` | Safe transmute for GPU buffer data |
| `rfd` | Native file dialog (open file, open folder) |
| `pollster` | Block on async GPU initialization |
| `env_logger` | Logging (set `RUST_LOG=info` for load diagnostics) |
| `thiserror` | Error type derivation |
| `log` | Logging facade |

## Acknowledgements

Built with [Claude Code](https://claude.ai/claude-code) by Anthropic and Codex.
Also developed with Codex by OpenAI.
