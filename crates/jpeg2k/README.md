# JPEG 2000 Codec

Pure Rust implementation of JPEG 2000 Part-1 (ITU-T T.800 / ISO/IEC 15444-1) encoder and decoder.

## Conformance scope (2.0.0)

This crate emits **conformant ITU-T T.800 lossless codestreams** within an
explicit profile. Everything legal-but-unsupported is **rejected loudly** via a
validated support matrix rather than mis-encoded.

Supported: 1 tile, 1 component (**unsigned 16-bit**), reversible 5/3 DWT, LRCP
progression, 1 layer, `cb_style = 0`, zero grid origins, code-block exponents
`2..=10` (sum ≤ 12). Configurable decomposition levels and code-block size.

Rejected as `Unsupported`: multiple tiles or components, non-16-bit or signed
precision, MCT ≠ 0, progression ≠ LRCP, layers ≠ 1, `cb_style ≠ 0`, transform ≠
5/3, non-zero origins, `POC/COC/QCC/PPM/PPT/PLM/PLT`, and `SOP/EPH`.

Interop is verified in CI against **OpenJPEG** (`opj_compress`/`opj_decompress`)
in both directions.

### Legacy (1.0.0 / Go) files

1.0.0 shipped a non-conformant raw-DWT format. Those files remain decodable via
`DecodeOptions { legacy: LegacyPolicy }`: `decode()` uses `Auto` (try standard,
fall back to the legacy fingerprint), and `StandardOnly` / `LegacyOnly` are
available. The dicos registry adapter uses `StandardOnly` for untrusted,
transfer-syntax-tagged data.

## Features

- **Lossless compression** using 5/3 reversible discrete wavelet transform (DWT)
- **EBCOT tier-1 block coding** with the MQ arithmetic coder
- **MQ arithmetic coder** (ITU-T T.800 Annex C)
- **Configurable decomposition levels** and code-block sizes
- **Single-component unsigned 16-bit** grayscale images
- **DICOS/DICOM compatible**: Transfer Syntax `1.2.840.10008.1.2.4.90`
- **Pure Rust**: No external codec dependencies

## Transfer Syntax

DICOM UID: `1.2.840.10008.1.2.4.90` (JPEG 2000 Lossless)

## Usage

Add the package under the plain import name:

```toml
[dependencies]
jpeg2k = { package = "pure_jpeg2k", version = "2.0" }
```

### Encoding

```rust
use jpeg2k::{encode, Jpeg2kOptions};

let pixels: Vec<u16> = (0..64).map(|i| i * 1000).collect();
let opts = Jpeg2kOptions::default();
let mut compressed = Vec::new();
encode(&pixels, 8, 8, &opts, &mut compressed).unwrap();
```

### Encoding with Custom Options

```rust
use jpeg2k::{encode, Jpeg2kOptions};

let opts = Jpeg2kOptions {
    tile_width: 0,           // 0 = single tile (whole image)
    tile_height: 0,
    cb_width_exp: 6,         // Code-block width: 2^6 = 64
    cb_height_exp: 6,        // Code-block height: 2^6 = 64
    num_decomp_levels: 5,    // DWT decomposition levels
};
let mut compressed = Vec::new();
encode(&pixels, 8, 8, &opts, &mut compressed).unwrap();
```

### Decoding

```rust
use jpeg2k::decode;

let (decoded, width, height) = decode(&compressed, 8, 8).unwrap();
assert_eq!(decoded, pixels);
```

## Supported Image Types

| Pixel Type | Precision | Components | Description |
|------------|-----------|------------|-------------|
| `u16` | 16-bit unsigned | 1 | 16-bit grayscale (the only conformant profile) |

## Architecture

```
jpeg2k/src/
  lib.rs          # Public API (encode, decode, Jpeg2kOptions, TRANSFER_SYNTAX_UID)
  codestream.rs   # Codestream reader/writer, top-level encode/decode
  markers.rs      # JPEG 2000 marker constants and segment structures
  bitstream.rs    # Bit-level I/O utilities
  dwt.rs          # 5/3 reversible discrete wavelet transform (lifting scheme)
  tile.rs         # Tile-level encoding and decoding
  rct.rs          # Reversible Color Transform (identity for grayscale)
  mq.rs           # MQ binary adaptive arithmetic coder
  ebcot.rs        # EBCOT block coding (Tier-1 and Tier-2)
  error.rs        # Error types
```

### Processing Pipeline

The encoder processes an image through these stages:

1. **Tiling** -- Partition the image into tiles (default: single tile)
2. **RCT** -- Reversible Color Transform (identity for single-component)
3. **DWT** -- 5/3 reversible wavelet transform via lifting, producing LL/LH/HL/HH subbands at each decomposition level
4. **Quantization** -- Identity (no quantization) for lossless mode
5. **EBCOT Tier-1** -- Code each code-block independently using three coding passes (significance, refinement, cleanup) with the MQ coder
6. **EBCOT Tier-2** -- Organize coded data into packets and layers
7. **Codestream** -- Write markers and tile-part data

## JPEG 2000 Codestream Format

```
SOC  - Start of Codestream (0xFF4F)
SIZ  - Image and Tile Size (0xFF51)
       Image dimensions, tile dimensions, component count, precisions
COD  - Coding Style Default (0xFF52)
       Progression order, decomposition levels, code-block size, transform
QCD  - Quantization Default (0xFF5C)
       Quantization style, step sizes per subband
SOT  - Start of Tile-part (0xFF90)
       Tile index, tile-part length
SOD  - Start of Data (0xFF93)
[tile-part coded data: packets containing EBCOT code-block bitstreams]
EOC  - End of Codestream (0xFFD9)
```

The conformant profile emits exactly one SOT/SOD tile-part (single tile);
multi-tile codestreams are rejected on decode.

## References

- ITU-T Rec. T.800 | ISO/IEC 15444-1 (JPEG 2000 Part-1 Core Coding)
- ITU-T Rec. T.800 Annex C (MQ Arithmetic Coder)
- D. Taubman and M. Marcellin, "JPEG2000: Image Compression Fundamentals,
  Standards and Practice", Kluwer Academic Publishers, 2002
- DICOM Transfer Syntax: `1.2.840.10008.1.2.4.90` (JPEG 2000 Lossless)
