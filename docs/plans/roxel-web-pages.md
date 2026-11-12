# Plan: Roxel in the browser (wasm + WebGPU) on GitHub Pages

- **Branch:** `feat/roxel-web` (based on `main` @ `303acc3`)
- **Target URL:** `https://jpfielding.github.io/dicos.rs/` (the repo is public)
- **Goal:** Ship a browser build of `crates/roxel` that opens a bundled sample scan and lets a user load their own `.dcs` files. The native build must behave exactly as it does today.
- **Non-goals:** WebGL2 fallback, mobile layout, worker-thread parsing, folder picking in the browser, and publishing to crates.io.

---

## 0. Ground truth (verified on 2026-11-11, not guessed)

| Fact | Evidence |
|---|---|
| `dicos` and all of its codecs already compile for `wasm32-unknown-unknown` | `cargo check -p dicos --features all-codecs --target wasm32-unknown-unknown` finishes with no errors |
| `roxel` fails first on `arboard` | It comes in through `egui-winit`'s default `clipboard` feature. Errors inside roxel's own code are not visible until this is fixed. |
| wgpu 27's WebGPU backend **does not** map `TEXTURE_FORMAT_16BIT_NORM` | `wgpu-27.0.1/src/backend/webgpu.rs` `FEATURES_MAPPING` has no entry for it. It does map `FLOAT32_FILTERABLE`. |
| The volume texture is `Rgba16Unorm` | `crates/roxel/src/renderer.rs:490` |
| The transfer LUT is `Rgba32Float` and needs to be filterable | `renderer.rs:535` and `raycast.wgsl:143`. On the web this requires the optional `float32-filterable` feature. |
| The shaders sample through `texture_3d<f32>` / `texture_2d<f32>` | `raycast.wgsl:35,37`, so changing a texture format needs **no shader changes** |
| Required features are hard-coded | `main.rs:107`: `TEXTURE_FORMAT_16BIT_NORM \| FLOAT32_FILTERABLE` |
| Blocking or native-only APIs in use | `pollster::block_on` (main.rs:111,168,179), `std::thread::spawn` (loader.rs:76), `std::fs` (loader.rs:156, dataset.rs:169, threats.rs:216), `rayon` (slice_view.rs:302), `std::time::Instant` (app.rs:29, state.rs:22; it **panics** on wasm), `rfd::FileDialog` sync calls (sidebar.rs:28,40; ui/mod.rs:182), `rfd::MessageDialog`, `env_logger`, `std::env`, `std::process::exit`, PNG screenshot path (main.rs) |
| `std::panic::catch_unwind` in `load_dicos_path` (dataset.rs:152) | wasm32 uses panic=abort, so a codec panic will take down the tab. Accept this for the demo and log it. |
| Sample data is synthetic (Blender-generated) | `testdata/*/ct.dcs` are about 16.8 MB each, `tdr.dcs` about 1–2 KB. They are safe to publish. |
| `web-time` is already in `Cargo.lock` | It comes in through egui/winit, so it adds no new download |
| CI uses `RUSTFLAGS=-D warnings` | `.github/workflows/ci.yml`. The wasm build must have no warnings, including dead code caused by `cfg`. |

---

## 1. Architecture decisions

1. **WebGPU only.** If `navigator.gpu` is missing, or the adapter lacks `float32-filterable`, show a plain HTML message in `index.html` that names the supported browsers and the reason. Don't build a WebGL path.
2. **Pick the volume texture format at runtime, not by `cfg`.** At startup:
   - If the adapter supports `TEXTURE_FORMAT_16BIT_NORM`, request it and use `Rgba16Unorm`. This is today's native path and must not change.
   - Otherwise use `Rgba16Float`, converting the packed `u16` values to `half::f16` (`v as f32 / 65535.0`).
   - Store the chosen format on the renderer or app and pass it down.
   - This also helps native GPUs that lack 16-bit norm support. No shader change is needed.
   - **Known trade-off:** f16 has a 10-bit mantissa, so close to 1.0 one step is about 32 u16 counts. Check visually that band edges look right (see §5).
3. **One binary with `cfg(target_arch = "wasm32")` entry points.** Keep `src/main.rs` as the native entry point and add a wasm `main` under `cfg`, so Trunk builds the same `[[bin]] roxel`. Move native-only helpers (screenshot, env-var capture size, MessageDialog) into a `#[cfg(not(target_arch = "wasm32"))]` module so the wasm build has no dead-code warnings.
4. **Make loading work on bytes.** Add byte-based loaders and turn the path-based loaders into thin wrappers around them. The web build only uses the byte API.
5. **Parse on the main thread on the web.** A 16 MB file will freeze the page briefly. Show a "Loading…" state for one frame before parsing. Worker-based parsing is out of scope.
6. **Deploy with Trunk.** Trunk fetches the `wasm-bindgen-cli` version that matches `Cargo.lock`. Use the official Pages actions (`actions/upload-pages-artifact` and `actions/deploy-pages`) rather than a `gh-pages` branch.

---

## 2. Work breakdown (in order, one commit each)

### Step 0: Devcontainer build support (`.devcontainer/Dockerfile`, `.devcontainer/devcontainer.json`)
- rustup install adds `-t wasm32-unknown-unknown`.
- Install Trunk from the pinned release binary (`TRUNK_VERSION=0.22.1`, `trunk-$(uname -m)-unknown-linux-musl.tar.gz`), verified against the release's `.sha256` file. The `.sha256` file holds the bare hash with no filename, so it is checked with `echo "<hash>  <asset>" | sha256sum -c -`. Trunk downloads the `wasm-bindgen` and `wasm-opt` versions it needs on first use, into `~/.cache/trunk`.
- `devcontainer.json`: forward port 8080 for `trunk serve`.
- **Done when:** `podman build --target user .devcontainer` succeeds, and inside the image `trunk --version` and `rustup target list --installed` show Trunk and the wasm target.

### Step 1: Dependency gating (`crates/roxel/Cargo.toml`, workspace `Cargo.toml`)
- Workspace: set `egui-winit = { version = "0.33", default-features = false }`. In roxel, turn the native defaults back on for non-wasm targets:
  ```toml
  [target.'cfg(not(target_arch = "wasm32"))'.dependencies]
  egui-winit = { workspace = true, features = ["clipboard", "links", "wayland", "x11"] }
  pollster.workspace = true
  rayon.workspace = true
  env_logger.workspace = true
  png = "0.18"

  [target.'cfg(target_arch = "wasm32")'.dependencies]
  wasm-bindgen = "0.2"
  wasm-bindgen-futures = "0.4"
  web-sys = { version = "0.3", features = ["Window", "Document", "Element", "HtmlCanvasElement", "Response"] }
  console_log = "1"
  console_error_panic_hook = "0.1"
  ```
- Add `half = { version = "2", features = ["bytemuck"] }` for all targets, since it's used by the runtime format fallback.
- Add `web-time = "1"` for all targets.
- `rfd`: keep it for all targets. Its async API works on wasm, but the `FileDialog` sync calls and `pick_folder` must be `cfg`-gated.
- **Check (done 2026-11-11):** `cargo tree -p roxel -i getrandom@{0.3.4,0.4.2} --target wasm32-unknown-unknown -e normal` prints nothing, so `getrandom` is **not** in roxel's wasm graph and needs no feature work. Re-check after adding the wasm deps.
- **Done when:** `cargo check -p roxel --target wasm32-unknown-unknown` gets past dependency compilation and shows only roxel-source errors, and `cargo check -p roxel` (native) still passes.

### Step 2: Portability fixes that change nothing natively
- Replace `std::time::Instant` with `web_time::Instant` in `app.rs` and `state.rs`. Do this unconditionally; on native it re-exports std.
- `slice_view.rs:302`: under `cfg(target_arch = "wasm32")` use `into_iter()` / `iter_mut()` instead of `into_par_iter()`. Keep the closure body shared so the two paths can't drift apart.
- `env_logger::init()` stays native-only. On wasm use `console_log::init_with_level(log::Level::Info)` and `console_error_panic_hook::set_once()`.

### Step 3: Byte-based loading (`volume/dataset.rs`, `volume/threats.rs`, `loader.rs`)
- `dataset.rs`: add `pub fn load_dicos_bytes(bytes: &[u8]) -> Result<Volume, DicosError>` using `reader::parse(std::io::Cursor::new(bytes))`. Rewrite `load_dicos_volume(path)` to read the file and call it.
- `threats.rs`: split `load_threat_sidecars_from_dir` into directory listing (native only) and `pub fn load_threat_sidecars_from_bytes(files: &[(String, Vec<u8>)], dims: [usize; 3]) -> Vec<ThreatBox>`. Keep the existing tests passing and add one test for the bytes API.
- `loader.rs`:
  - Add a source type: `enum LoadSource { Path(PathBuf), Bytes { name: String, files: Vec<(String, Vec<u8>)> } }`. When there are several files, the largest `.dcs` is the CT and the rest are sidecars. Match `load_single_file`'s current sidecar-merge behaviour.
  - `Loader::request` on native: same thread-spawn behaviour as today. On wasm: store the request and run it in `poll()` on the **next** frame, so the spinner renders first.
  - `loaded_path: Option<PathBuf>` is used for display only (`ui/mod.rs:44,414`, `slice_panel.rs:292`, `sidebar.rs:59`). Keep the type, and on the web set it to `PathBuf::from(name)`. Don't widen this refactor.
- `std::fs` calls in these modules move behind `#[cfg(not(target_arch = "wasm32"))]`.

### Step 4: File picking in the browser (`ui/sidebar.rs`, `ui/mod.rs`, `state.rs`)
- Native: unchanged.
- Web:
  - "Open file…" calls `rfd::AsyncFileDialog::new().add_filter("DICOS", &["dcs","dcm"]).pick_files()`. Use multi-select so the user can choose `ct.dcs` and `tdr.dcs` together.
  - Run it with `wasm_bindgen_futures::spawn_local`, read every handle, and send `LoadSource::Bytes` through an `Rc<RefCell<Option<…>>>` or a `std::sync::mpsc` channel that `App` drains each frame. **Note:** the browser only opens the dialog from a user gesture. egui's click handling runs inside the winit event callback, which should count, but check this in Chrome and Safari.
  - Hide "Open folder…" on wasm.
- **Drag and drop:** handle `WindowEvent::DroppedFile` on native only (it gives a path). On the web, winit 0.30 doesn't deliver file bytes, so skip it for this branch and list it under Follow-ups.
- Add a **"Load sample"** button (web only) that `fetch`es `samples/fishtank-knife/ct.dcs` and `tdr.dcs` (relative URL, which works under the `/dicos.rs/` base path) and sends `LoadSource::Bytes`. Also load the sample automatically on first launch so the demo isn't an empty screen.

### Step 5: Async GPU startup (`main.rs`, `renderer.rs`)
This is the biggest structural change. In winit 0.30 on the web, `resumed()` can't block, so adapter and device creation must be async.
- Split `RoxelApp` startup into:
  - `async fn init_gpu_async(window: Arc<Window>) -> Result<Gpu, String>` with the adapter and device logic from `try_init_gpu` / `request_adapter`, no `pollster`.
  - Native: `pollster::block_on(init_gpu_async(...))`, so behaviour is the same as today.
  - Web: `resumed()` creates the window with `WindowAttributesExtWebSys::with_append(true)` (or `with_canvas` pointing at `<canvas id="roxel">`), then calls `spawn_local`. When the future finishes, it sends the `Gpu` back through an `EventLoopProxy<UserEvent>` (`EventLoop::<UserEvent>::with_user_event()`), and `user_event()` finishes the setup that `init_gpu` does today.
  - Until the GPU is ready, `window_event` must ignore redraw and resize.
- Required features: query `adapter.features()`:
  - `FLOAT32_FILTERABLE` is required. If it's missing, show an error (web: write to a DOM `<div id="roxel-error">`; native: today's MessageDialog).
  - `TEXTURE_FORMAT_16BIT_NORM` is optional and decides the volume format (§1.2).
- `renderer.rs:484-510`: take the chosen `TextureFormat`. If it is `Rgba16Float`, convert `packed: &[u16]` to `Vec<half::f16>` before `write_texture`. The bytes per row stay `dim_x * 8`.
- Surface: on the web, set the canvas size from `window.inner_size()`, which winit derives from the CSS size. Handle `ScaleFactorChanged`. `capture_window_size()` and everything to do with screenshots are native-only.
- Web `main`:
  ```rust
  #[cfg(target_arch = "wasm32")]
  fn main() {
      console_error_panic_hook::set_once();
      let _ = console_log::init_with_level(log::Level::Info);
      let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
      let proxy = event_loop.create_proxy();
      use winit::platform::web::EventLoopExtWebSys;
      event_loop.spawn_app(RoxelApp::new(None, proxy));
  }
  ```
- **Done when:** `cargo build -p roxel --target wasm32-unknown-unknown --release` has no warnings, and `cargo clippy --workspace --all-targets --all-features` (native) is clean.

### Step 6: Web shell (`crates/roxel/index.html`, `crates/roxel/Trunk.toml`)
- `index.html`: a full-viewport `<canvas id="roxel">`, a `<noscript>`, and a small inline script that checks `navigator.gpu` and shows an "unsupported browser" message before the wasm loads.
  ```html
  <link data-trunk rel="rust" href="Cargo.toml" data-bin="roxel" data-wasm-opt="s" />
  ```
- `Trunk.toml`: `[build] public_url = "./"` (relative, so it works at `/dicos.rs/` and locally).
- Samples: **don't** copy them through Trunk from `testdata/`, because the folder names contain `+`. The CI step in §Step 7 copies one sample into `dist/samples/fishtank-knife/`.
- Local run: `cd crates/roxel && trunk serve`, then copy the sample into `dist/samples/...` by hand, or add `--features` later. Document this in the crate README.

### Step 7: CI and Pages (`.github/workflows/`)
- In `ci.yml`, add a job `wasm-check`: install the stable toolchain with `targets: wasm32-unknown-unknown`, then run `cargo clippy -p roxel --target wasm32-unknown-unknown -- -D warnings`. This stops the web build from quietly rotting.
- New `pages.yml`:
  - Triggers: `push` to `main` and `workflow_dispatch`.
  - Permissions: `pages: write`, `id-token: write`, `contents: read`.
  - Concurrency: group `pages`, `cancel-in-progress: true`.
  - Steps: checkout → toolchain + wasm target → `Swatinem/rust-cache` → install Trunk (`jetli/trunk-action` or `cargo binstall trunk`, pinned to the **same** `TRUNK_VERSION` as `.devcontainer/Dockerfile`, so the container and CI build the same way) → `cd crates/roxel && trunk build --release` → copy `testdata/backpack+purse_fishtank+goldfish_THREAT-knife_20260227-191512/{ct,tdr}.dcs` into `crates/roxel/dist/samples/fishtank-knife/` → `actions/upload-pages-artifact` (path `crates/roxel/dist`) → `actions/deploy-pages`.
- **Owner action, which Claude can't do:** in GitHub, go to Settings → Pages → Source and choose **GitHub Actions**.

### Step 8: Docs
- `crates/roxel/README.md` (or the root `README.md`): add a "Web demo" section with the link, browser requirements, how to run `trunk serve` locally, and the known limits (no folder open, single-threaded parse, a codec panic kills the tab, f16 volume precision).

---

## 3. Files touched (expected)

| File | Change |
|---|---|
| `Cargo.toml` (workspace) | `egui-winit` with `default-features = false` |
| `crates/roxel/Cargo.toml` | target-specific deps, `half`, `web-time` |
| `crates/roxel/src/main.rs` | async GPU init, `UserEvent`, wasm `main`, native-only module for screenshots and dialogs |
| `crates/roxel/src/renderer.rs` | volume format parameter and f16 conversion |
| `crates/roxel/src/app.rs`, `state.rs` | `web_time::Instant`, drain web load channel |
| `crates/roxel/src/loader.rs` | `LoadSource`, wasm deferred-run path |
| `crates/roxel/src/volume/dataset.rs`, `threats.rs` | byte-based APIs, `fs` behind `cfg` |
| `crates/roxel/src/slice_view.rs` | rayon gated by `cfg` |
| `crates/roxel/src/ui/sidebar.rs`, `ui/mod.rs` | async picker and "Load sample" on the web |
| `crates/roxel/index.html`, `Trunk.toml` | new |
| `.devcontainer/Dockerfile`, `devcontainer.json` | wasm target, pinned Trunk, port 8080 |
| `.github/workflows/ci.yml` | `wasm-check` job |
| `.github/workflows/pages.yml` | new |
| README | web demo section |

Leave everything else alone, especially the `dicos` and codec crates, which already compile for wasm.

---

## 4. Risks and open questions (flag them, don't paper over them)

1. **f16 precision versus u16 norm.** Band thresholds near the top of the range lose about 5 bits. If the transfer-function bands visibly shift, the fallback is two `R16Uint`-style textures with manual trilinear filtering in WGSL. That's a much bigger change, so stop and report rather than doing it unasked.
2. **`float32-filterable` availability.** Desktop Chrome, Edge and Safari support it; some mobile GPUs don't. If you need to widen support, store the transfer LUT as `Rgba16Float` (it holds 0–1 colour and opacity, so precision is fine). This is a small change, but only do it if testing shows it's needed.
3. **The file dialog's user-gesture requirement** inside egui click handling. Verify it in Chrome and Safari.
4. **Wasm size.** Four codecs plus wgpu plus egui will probably come to 5–10 MB before compression, which is acceptable. Record the actual size in the PR.
5. **winit 0.30 canvas sizing and HiDPI** quirks on Safari.
6. **Main-thread parse time** for a 16 MB JPEG-LS or J2K scan could be several seconds. Measure it and note it in the PR. Moving parsing to a worker is a follow-up.

---

## 5. Verification checklist (all items must pass before the PR is opened)

- [ ] **Devcontainer:** inside the rebuilt image, `cd crates/roxel && trunk build --release` succeeds
- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features` (native, `-D warnings`)
- [ ] `cargo clippy -p roxel --target wasm32-unknown-unknown -- -D warnings`
- [ ] `cargo test --workspace --all-features` (including the new bytes-API threat test)
- [ ] **Native regression:** `cargo run -p roxel -- testdata/bag_ct.dcs` looks and behaves as it does on `main`. Use the existing `ROXEL_SCREENSHOT` env var to capture before and after PNGs and compare them. They should match pixel for pixel, because the native path still uses `Rgba16Unorm`.
- [ ] **Web:** `trunk serve` in Chrome. The sample loads automatically, the volume renders, threat boxes from `tdr.dcs` show, the slice panel works, band sliders respond, and resize and HiDPI behave.
- [ ] **Web precision check:** compare a screenshot of the web build against a native screenshot of the same sample with the same camera and bands. Note any visible difference in the PR.
- [ ] **Web:** "Open file…" with multi-select of `ct.dcs` and `tdr.dcs` from another sample loads with threats.
- [ ] **Web:** Safari (macOS 26+). The page either works or shows the unsupported-browser message; it must never show a blank canvas.
- [ ] The Pages workflow passes on the branch through `workflow_dispatch` (deploy only after merge, unless the owner says otherwise).

---

## 6. Follow-ups (out of scope for this branch)
- Drag and drop on the web (DOM `drop` listener → `FileReader` → `LoadSource::Bytes`).
- Parsing in a Web Worker so the UI doesn't freeze.
- A WebGL2 fallback (`Rgba16Float` 3D textures are filterable on WebGL2).
- A sample picker for both bundled scans.

## 7. Hand-off notes for the implementing agent
- Work in the worktree `~/worktrees/dicos.rs/roxel-web` on branch `feat/roxel-web`.
- Commit one step at a time with clear messages. Native must build and pass tests at every commit.
- If any risk in §4 comes true, stop and report with evidence (screenshots or numbers) instead of widening scope.
- Don't push or open a PR until the §5 checklist passes. When the PR is ready, link this plan in it.
