# JPEG Lossless Codec (jpegli)

Pure Rust implementation of JPEG Lossless (ITU-T T.81 Annex H) encoder and decoder.

## Conformance scope (2.0.0)

This crate emits **conformant ITU-T T.81 Process 14 SV1** (non-hierarchical,
first-order) lossless streams for **single-component** images. Supported:
predictors **1–7**, **point transform** (reduced-domain coding), and
**restart intervals** (row-aligned per H.1.1 — the DRI value is a multiple of
the image width). Precision `2..=16`. Interop is verified against
**libjpeg-turbo ≥ 3.0** (`cjpeg -lossless` / `djpeg`).

Encoder tuning is via `EncodeOptions { predictor, point_transform,
restart_interval_rows, precision }` through `encode_with_options`; plain
`encode` uses predictor 1, no point transform, no restarts, 16-bit. Note: the
`.70` transfer syntax specifically means predictor 1, so registry encodes pin
defaults to keep the emitted UID truthful.

## Features

- **Lossless compression** using differential pulse code modulation (DPCM)
- **Predictors 1-7** supported for optimal compression
- **Point transform** and **row-aligned restart intervals**
- **Huffman entropy coding** for prediction residuals
- **Single-component** grayscale images, precision `2..=16`
- **DICOS/DICOM compatible**: Transfer Syntax `1.2.840.10008.1.2.4.70`
- **Pure Rust**: No external codec dependencies

## Transfer Syntax

DICOM UID: `1.2.840.10008.1.2.4.70` (JPEG Lossless, Non-Hierarchical, Process 14 SV1)

## Usage

Add the package under the plain import name:

```toml
[dependencies]
jpegli = { package = "pure_jpegli", version = "2.0" }
```

### Encoding

```rust
use jpegli::encode;

// Encode with predictor 1 (left neighbor)
let pixels = vec![100u16, 200, 300, 400];
let mut compressed = Vec::new();
encode(&pixels, 2, 2, &mut compressed).unwrap();
```

### Decoding

```rust
use jpegli::decode;

let (decoded, width, height) = decode(&compressed, 2, 2).unwrap();
assert_eq!(decoded, pixels);
```

## Predictors

JPEG Lossless defines 7 predictors based on up to three neighboring samples:

```
      Rc  Rb
      Ra   x
```

Where:
- **Ra** = sample immediately to the left of `x`
- **Rb** = sample immediately above `x`
- **Rc** = sample diagonally above-left of `x` (above Ra, left of Rb)

| Selection | Formula | Name |
|-----------|---------|------|
| 1 | Ra | Left |
| 2 | Rb | Above |
| 3 | Rc | Above-left |
| 4 | Ra + Rb - Rc | Linear interpolation |
| 5 | Ra + (Rb - Rc) / 2 | Weighted left + half vertical gradient |
| 6 | Rb + (Ra - Rc) / 2 | Weighted above + half horizontal gradient |
| 7 | (Ra + Rb) / 2 | Average of left and above |

The default encoding uses predictor 1 (left neighbor). For the first row,
prediction uses the previous sample in scan order. For the first pixel,
prediction uses 2^(precision-1) as the initial value.

## Supported Image Types

| Pixel Type | Precision | Components | Description |
|------------|-----------|------------|-------------|
| `u16` | `2..=16` bits | 1 (`Nf = 1`) | single-component grayscale |

## JPEG Lossless Stream Format

```
SOI  - Start of Image (0xFFD8)
SOF3 - Start of Frame, Lossless Huffman (0xFFC3)
       Precision, height, width, components
DHT  - Define Huffman Table (0xFFC4)
       Table class, table ID, code counts, code values
SOS  - Start of Scan (0xFFDA)
       Component selector, predictor selection, point transform
[entropy-coded DPCM differences]
EOI  - End of Image (0xFFD9)
```

The SOF3 marker (0xFFC3) distinguishes JPEG Lossless from other JPEG modes.
The entropy-coded segment contains Huffman-coded prediction residuals, where
each residual is the difference between the actual pixel value and the
predicted value from the selected predictor.

## Architecture

```
jpegli/src/
  lib.rs      # Public API (encode, decode, TRANSFER_SYNTAX_UID)
  encode.rs   # Encoder: DPCM prediction + Huffman output
  decode.rs   # Decoder: Huffman input + DPCM reconstruction
  huffman.rs  # Huffman table construction and coding
  scan.rs     # Scan-level encode/decode (SOS segment processing)
  error.rs    # Error types
```

## References

- ITU-T Rec. T.81 | ISO/IEC 10918-1 (JPEG), Annex H (Lossless Mode)
- DICOM Transfer Syntax: `1.2.840.10008.1.2.4.70` (JPEG Lossless, First-Order Prediction, Process 14 SV1)
