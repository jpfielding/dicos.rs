# JPEG-LS Codec

Pure Rust implementation of JPEG-LS (ITU-T T.87 / ISO/IEC 14495-1) encoder and decoder.

## Conformance scope

This crate emits **conformant ITU-T T.87 streams**. Supported: LSE ID=1 presets
(MAXVAL/T1/T2/T3/RESET) honored on decode, run mode, near-lossless, sample
precision `2..=16`, for:

- **Single-component** images (`Nf = 1`) — `encode` / `decode`, and
  `encode_image` / `decode_image`.
- **Multi-component planar** images (`ILV = 0`: a shared frame header, then one
  scan per component, e.g. RGB) — `encode_image` / `decode_image`.
  `decode_image` accepts 1 or 3 components; `encode_image` accepts the same counts.

Rejected: line- and sample-interleaved scans (`ILV = 1/2`), restart markers
inside a scan (`RSTn`), LSE mapping tables, and component subsampling
(`Unsupported`); a non-zero point transform or an impossible scan component
count (`InvalidData`). A `DRI` segment on its own is tolerated and skipped.
Interop is verified against **CharLS** `.jls` fixtures (including an
LSE-bearing stream).

### Profiles

- `Profile::T87` (default) — ITU-T T.87 conformant output.
- `Profile::LegacyGo` — reproduces the frozen 1.0.0 / Go-compatible bytes
  (T.81-style `0xFF00` stuffing, no run mode, uncapped Golomb); lossless only.

Near-lossless and explicit precision are set through `EncodeOptions` via
`encode_with_options`; `DecodeOptions` selects the decode profile.

## Features

- **Lossless and near-lossless** compression using the LOCO-I algorithm
- **Context-based adaptive prediction** with Median Edge Detection (MED)
- **Golomb-Rice entropy coding** with adaptive parameter selection
- **Run-length mode** for efficient coding of uniform regions
- **Grayscale and planar multi-component** (e.g. RGB, `ILV = 0`) images,
  precision `2..=16`
- **DICOS/DICOM compatible**: Transfer Syntax `1.2.840.10008.1.2.4.80`
- **Pure Rust**: No external codec dependencies

## Transfer Syntax

DICOM UID: `1.2.840.10008.1.2.4.80` (JPEG-LS Lossless)

## Usage

Add the package under the plain import name:

```toml
[dependencies]
jpegls = { package = "pure_jpegls", version = "2.0" }
```

### Encoding

```rust
use jpegls::encode;

let pixels: Vec<u16> = (0..256).collect();
let mut compressed = Vec::new();
encode(&pixels, 16, 16, &mut compressed).unwrap();
```

### Decoding

```rust
use jpegls::decode;

let (decoded, width, height) = decode(&compressed, 16, 16).unwrap();
assert_eq!(decoded, pixels);
```

### Planar multi-component (e.g. RGB)

`encode_image` / `decode_image` take and return **component-planar** samples
(all of component 0, then component 1, …). `decode_image` reads the geometry
from the codestream and returns it with the frame metadata.

`decode_image` limits the output sample buffer to 64 MiB by default. Use
`decode_image_with_options` with `DecodeImageOptions { max_decoded_bytes }`
to select a different budget; every component counts toward this limit.

```rust
use jpegls::{decode_image, encode_image, EncodeOptions};

let (w, h) = (16u32, 16u32);
let planar: Vec<u16> = (0..3 * 256).map(|i| (i % 256) as u16).collect(); // R, G, B planes
let mut rgb = Vec::new();
encode_image(&planar, w, h, 3, &EncodeOptions::default(), &mut rgb).unwrap();

let img = decode_image(&rgb).unwrap();
assert_eq!((img.width, img.height, img.components), (w, h, 3));
assert_eq!(img.samples, planar);
```

## JPEG-LS Algorithm

JPEG-LS uses the LOCO-I (LOw COmplexity LOssless COmpression for Images) algorithm:

1. **Edge detection** using local gradients to classify the coding context
2. **Context modeling** based on neighboring pixel relationships
3. **Adaptive prediction** using the Median Edge Detector (MED)
4. **Golomb-Rice coding** for prediction residuals with adaptive k parameter
5. **Run-length encoding** for uniform regions where all local gradients are zero

### Prediction Context

```
      c  b  d
      a  x
```

Where `x` is the current sample being encoded, and `a`, `b`, `c`, `d` are
neighboring samples used for prediction and context determination:

- **a** = sample immediately to the left
- **b** = sample immediately above
- **c** = sample diagonally above-left
- **d** = sample diagonally above-right

### Context Modeling

The encoder maintains 365 regular contexts plus 2 run-mode contexts, each
with adaptive A/B/C/N statistics that are updated after every sample. These
statistics drive both the bias cancellation and the Golomb-Rice parameter
selection, allowing the codec to adapt to local image characteristics.

## Supported Image Types

| Pixel Type | Precision | Components | Description |
|------------|-----------|------------|-------------|
| `u16` | `2..=16` bits | 1 (`Nf = 1`, `ILV = 0`) | single-component grayscale |
| `u16` | `2..=16` bits | 3 (`Nf = 3`, `ILV = 0`) | planar multi-component, e.g. RGB  |

## JPEG-LS Stream Format

```
SOI   - Start of Image (0xFFD8)
SOF55 - Start of Frame, JPEG-LS (0xFFF7)
        Precision, height, width, components
LSE   - JPEG-LS Preset Parameters (0xFFF8, optional)
        Custom MAXVAL, T1, T2, T3, RESET thresholds
SOS   - Start of Scan (0xFFDA)
        Component selector, Near parameter (0 = lossless)
[entropy-coded image data]
(SOS + scan repeat once per component for planar, ILV = 0, multi-component)
EOI   - End of Image (0xFFD9)
```

## Architecture

```
jpegls/src/
  lib.rs        # Public API (encode, decode, encode_image, decode_image, TRANSFER_SYNTAX_UID)
  encode.rs     # Encoder: prediction + context + Golomb-Rice output
  decode.rs     # Decoder: Golomb-Rice input + context + reconstruction
  predictor.rs  # MED predictor and gradient computation
  context.rs    # Context modeling with adaptive A/B/C/N statistics
  run_mode.rs   # Run-length mode for uniform regions
  bitstream.rs  # Bit-level I/O utilities
  error.rs      # Error types
```

## References

- ITU-T Rec. T.87 | ISO/IEC 14495-1 (JPEG-LS baseline)
- M. Weinberger, G. Seroussi, G. Sapiro, "The LOCO-I Lossless Image Compression Algorithm:
  Principles and Standardization into JPEG-LS", IEEE Trans. Image Processing, 2000
- DICOM Transfer Syntax: `1.2.840.10008.1.2.4.80` (JPEG-LS Lossless)
