# dicos.rs

A pure-Rust library and toolkit for working with **DICOS** (Digital Imaging and
Communication in Security) files. DICOS is the NEMA IIC 1 standard used by
security screening equipment (CT scanners, X-ray systems) to encode volumetric
and projection image data. It is closely related to DICOM but tailored for
aviation and checkpoint security workflows.

This project implements the NEMA IIC 1 v04-2023 specification and provides:

- A core parsing library for reading DICOS datasets and pixel data.
- Four standalone lossless compression codecs (no DICOS dependency).
- A CLI tool (`dicosctl`) for inspecting files from the terminal.
- A GPU-accelerated volume viewer (`roxel`) with 3D ray-casting and 2D slice
  display.

Ported from [dicos.go](../dicos.go). Pure Rust -- no C dependencies.

## Project Components

### dicos -- Core Library

The `dicos` crate parses DICOS/DICOM binary files into an in-memory `Dataset`
of typed elements. It handles explicit and implicit VR transfer syntaxes,
encapsulated pixel data, and multi-frame volumes. Codec crates are wired in
through optional feature flags so downstream consumers only pay for what they
use.

### Compression Codecs

Pure Rust implementations of the lossless image compression formats used by
DICOS and DICOM. Each codec is a standalone crate with **zero dependency on
`dicos`**, so they can be used independently in any imaging pipeline.

As of 2.0.0 these codecs emit **standards-conformant** streams within an
explicit, documented scope; anything legal-but-unsupported is rejected loudly
rather than encoded incorrectly. See the "Conformance scope" table below and
each crate's README for the full support matrix.

| Package | Imported as | Standard | Algorithm |
|---------|-------------|----------|-----------|
| **pure_jpeg2k** | `jpeg2k` | ITU-T T.800 | Wavelet (5/3 DWT + EBCOT + MQ) |
| **pure_jpegli** | `jpegli` | ITU-T T.81 Annex H | DPCM (Process 14 SV1) |
| **pure_jpegls** | `jpegls` | ISO/IEC 14495-1 / ITU-T T.87 | LOCO-I |
| **pure_jpegrle** | `jpegrle` | DICOM Part 5 Section 8.1.1 | PackBits RLE |

#### Conformance scope (2.0.0)

| Codec | Conformant profile | Verified against |
|-------|--------------------|------------------|
| **pure_jpeg2k** | T.800 lossless codestreams: 1 tile, 1 component (unsigned 16-bit), reversible 5/3 DWT, LRCP, 1 layer, `cb_style=0`, zero grid origins. Everything else legal-but-unsupported is rejected via a validated support matrix. v1.0.0 / Go raw-DWT files remain decodable through `LegacyPolicy` (`Auto` by default; the dicos registry adapter uses `StandardOnly`). | **OpenJPEG** (`opj_compress`/`opj_decompress`), both directions, in CI |
| **pure_jpegls** | T.87 single-component, `ILV=0`, LSE ID=1 presets honored, run mode, near-lossless. `DRI`/restart markers and `Nf≠1`/`ILV≠0` are rejected as `Unsupported`. `Profile::LegacyGo` reproduces the frozen 1.0.0 / Go-compatible bytes. | Round-trip + a **CharLS** decode harness (`tests/interop.rs`); the harness self-skips until `.jls` fixtures are supplied — see its README for the generation commands |
| **pure_jpegli** | T.81 Process 14 lossless: default selection value 1 (SV1), predictors 1–7 selectable, point transform, restart intervals (row-aligned per H.1.1). | **libjpeg-turbo ≥ 3.0** (`cjpeg -lossless` / `djpeg`), both directions |
| **pure_jpegrle** | DICOM Part 5 §8.1.1 PackBits RLE; 16-bit split into high/low byte segments. | Round-trip + DICOM fixtures |

### dicosctl -- CLI Inspector

A command-line tool for quick inspection of DICOS files. Built as a binary
inside the `dicos` crate behind the `cli` feature flag.

### roxel -- GPU Volume Viewer

A three-panel desktop application for visualizing 3D CT volumes and 2D slices
from DICOS files. Uses `wgpu` for GPU ray-casting and `egui` for the UI.

![roxel 3D Rendering](https://github.com/jpfielding/dicos.rs/releases/download/v2.0.0/roxel-lrg.gif)

```text
+------------+------------------------+------------------------+
| Sidebar    |   3D Volume View       |   2D Slice View        |
|            |   (GPU ray-caster)     |   (CPU rendered)       |
| Metadata   |                        |                        |
| Layers     +------------------------+------------------------+
| Threats    | Quality | Opacity      | Volume: [dropdown]     |
|            | Preset  | Bands        | View:   [dropdown]     |
|            | Lighting | WC/WW       | Composite [x]          |
|            |                        | W/L: __ W/W: __        |
|            |                        | Slice: [slider]        |
+------------+------------------------+------------------------+
```

- **Left sidebar** -- file open, metadata display, volume layer toggles,
  rendering quality, transfer function presets (Bands / Threat / Monochrome),
  material band sliders, and Phong lighting controls.
- **Center panel** -- GPU ray-cast 3D volume rendering with arcball camera
  (left-drag to rotate, scroll to zoom, axial/coronal/sagittal preset views).
- **Right panel** -- CPU-rendered 2D slice viewer with orientation selector,
  window/level controls, slice index slider, and MIP composite toggle.

Requires a GPU with Vulkan, Metal, or DX12 support.

## Workspace Structure

```
dicos.rs/
  Cargo.toml              # Workspace root
  testdata/               # Generated fixtures (gitignored; see "Test data")
  crates/
    dicos/                # Core DICOS library + dicosctl binary
    jpegrle/              # RLE PackBits codec (standalone)
    jpegls/               # JPEG-LS codec (standalone)
    jpegli/               # JPEG Lossless codec (standalone)
    jpeg2k/               # JPEG 2000 codec (standalone)
    roxel/                # GPU-accelerated volume viewer
```

## Dependency Graph

```
jpegrle ----+
jpegls  ----|  (standalone, zero dicos dependency)
jpegli  ----|
jpeg2k  ----+
                dicos --> optionally uses codec crates via feature flags
                  |       includes dicosctl binary (--features cli)
                  |
           roxel --> dicos + wgpu + egui
```

## Building

```sh
# Build the entire workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Build dicosctl (the CLI tool)
cargo build -p dicos --features cli

# Build roxel in release mode (recommended for interactive use)
cargo build -p roxel --release
```

## Test data

As of 2.0.0 the `testdata/` directory is **generated on demand and gitignored**
(the 1.x tree checked in ~37 MB of fixtures). Integration tests that read
`testdata/` files skip gracefully when a fixture is absent, so a fresh checkout
tests green with no fixtures present.

To produce a synthetic public-safe CT volume via the example generator:

```sh
mkdir -p testdata/synthetic
cargo run -p dicos --example gen-luggage-fishtank -- \
  --size 64,64,48 --out testdata/synthetic/luggage_fishtank_ct.dcs
```

`--size x,y,z` accepts any axis ≥ 32 (x/y ≤ 65535); omit `--out` to write the
default path. CI runs this step before the test suite. See
[MIGRATION.md](MIGRATION.md) for the full 1.x → 2.0 upgrade guide.

## Usage

### dicosctl

The `dicos` crate ships `dicosctl` as a binary (behind the `cli` feature); the
fixture generator now lives at `examples/gen-luggage-fishtank.rs` (run with
`--example gen-luggage-fishtank`, see [Test data](#test-data)).

```sh
# Print a human-readable summary of a DICOS file
cargo run --bin dicosctl -p dicos --features cli -- info scan.dcs

# Example output:
#   File: scan.dcs
#   Elements: 42
#   Modality: CT
#   Dimensions: 512x512
#   Frames: 256
#   Bits Allocated: 16
#   Transfer Syntax: JPEG-LS Lossless (1.2.840.10008.1.2.4.80)
#   SOP Class: 1.2.840.10008.5.1.4.1.1.501.2.1
#   Manufacturer: L3 Technologies
#   Model: CX100

# Dump all metadata as JSON (pretty-printed by default)
cargo run --bin dicosctl -p dicos --features cli -- dump scan.dcs

# Pipe JSON output to jq for querying
cargo run --bin dicosctl -p dicos --features cli -- dump scan.dcs | jq '.Modality'

# Compact (single-line) JSON
cargo run --bin dicosctl -p dicos --features cli -- dump --compact scan.dcs
```

### roxel

```sh
# Launch with an empty viewport, then use File > Open to load
cargo run -p roxel --release

# Load a single DICOS file directly
cargo run -p roxel --release -- scan.dcs

# Load a directory of single-frame .dcs files as one volume
cargo run -p roxel --release -- /path/to/slices/
```

**Mouse controls:**

| Action | Effect |
|--------|--------|
| Left-drag in 3D view | Rotate volume (arcball) |
| Scroll wheel | Zoom in/out |

**Transfer function presets:**

| Preset | Description |
|--------|-------------|
| Default | Five-band material: Air (transparent), Organic (orange), Inorganic (green), Metal (blue), Dense (dark blue) |
| Threat | Red monochrome for threat highlighting |
| Mono | Grayscale density mapping |

See [crates/roxel/README.md](crates/roxel/README.md) for full documentation
including architecture, rendering pipeline, and GPU requirements.

## Test Results

Run `cargo test --workspace --all-features` to reproduce. Current counts:

```
dicos:  120 passed, 0 failed
roxel:   57 passed, 0 failed
jpeg2k: 140 passed, 0 failed
jpegli:  95 passed, 0 failed
jpegls:  92 passed, 0 failed
jpegrle: 32 passed, 0 failed
---------------------------------
Total:  536 passed, 0 failed, 0 ignored
```

## References

- [NEMA IIC 1 -- DICOS Standard](https://www.nema.org/standards/view/Digital-Imaging-and-Communications-in-Security)
- [DICOM Standard](https://www.dicomstandard.org/)
- [suyashkumar/dicom (Go reference)](https://github.com/suyashkumar/dicom)
- [Stratovan DICOS/DICOM format](https://www.stratovan.com/products/dicos)
- [JPEG-LS (ISO/IEC 14495-1)](https://en.wikipedia.org/wiki/Lossless_JPEG#JPEG-LS)
- [JPEG 2000 (ITU-T T.800)](https://en.wikipedia.org/wiki/JPEG_2000)
- [JPEG Lossless (ITU-T T.81 Annex H)](https://en.wikipedia.org/wiki/Lossless_JPEG)
- [DICOM RLE (Part 5 Section 8.1.1)](https://dicom.nema.org/medical/dicom/current/output/chtml/part05/sect_8.2.html)

## Acknowledgements

Built with [Claude Code](https://claude.ai/claude-code) by Anthropic and Codex.
