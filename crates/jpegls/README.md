# JPEG-LS Codec

Pure Rust implementation of JPEG-LS (ITU-T T.87 / ISO/IEC 14495-1) encoder and decoder.

## Features

- **Lossless compression** using the LOCO-I algorithm
- **Context-based adaptive prediction** with Median Edge Detection (MED)
- **Golomb-Rice entropy coding** with adaptive parameter selection
- **Run-length mode** for efficient coding of uniform regions
- **8-bit and 16-bit** grayscale images
- **DICOS/DICOM compatible**: Transfer Syntax `1.2.840.10008.1.2.4.80`
- **Pure Rust**: No external codec dependencies

## Transfer Syntax

DICOM UID: `1.2.840.10008.1.2.4.80` (JPEG-LS Lossless)

## Usage

Add the package under the plain import name:

```toml
[dependencies]
jpegls = { package = "pure_jpegls", version = "1.0" }
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

| Pixel Type | Precision | Description |
|------------|-----------|-------------|
| `u8` | 8-bit | 8-bit grayscale |
| `u16` | 16-bit | 16-bit grayscale |

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
EOI   - End of Image (0xFFD9)
```

## Architecture

```
jpegls/src/
  lib.rs        # Public API (encode, decode, TRANSFER_SYNTAX_UID)
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
