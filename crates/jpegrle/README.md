# DICOM RLE Codec

Pure Rust implementation of DICOM RLE (Run Length Encoding) using PackBits compression.

## Features

- **Lossless compression** using PackBits algorithm
- **8-bit and 16-bit** grayscale images
- **Byte-plane separation** for 16-bit images (improved compression)
- **DICOS/DICOM compatible**: Transfer Syntax `1.2.840.10008.1.2.5`
- **Pure Rust**: No external codec dependencies

## Transfer Syntax

DICOM UID: `1.2.840.10008.1.2.5` (RLE Lossless)

## Usage

Add the package under the plain import name:

```toml
[dependencies]
jpegrle = { package = "pure_jpegrle", version = "1.0" }
```

### Encoding

```rust
use jpegrle::encode;

let pixels: Vec<u16> = vec![100, 200, 300, 400];
let mut compressed = Vec::new();
encode(&pixels, 2, 2, &mut compressed).unwrap();
```

### Decoding

```rust
use jpegrle::decode;

// Width and height must be provided (not stored in RLE stream)
let (decoded, width, height) = decode(&compressed, 2, 2).unwrap();
assert_eq!(decoded, pixels);
```

## RLE Algorithm

DICOM RLE uses the PackBits algorithm, a simple run-length encoding scheme
that compresses data by replacing repeated byte sequences with a control byte
and a single copy of the repeated value.

| Control Byte | Action |
|--------------|--------|
| 0 to 127 | Copy next (n+1) bytes literally |
| -1 to -127 | Repeat next byte (-n+1) times |
| -128 | No operation (padding) |

### 16-bit Image Handling

For 16-bit grayscale images, pixels are split into two segments:
1. **Segment 1**: High bytes of all pixels
2. **Segment 2**: Low bytes of all pixels

This byte-plane separation improves compression because adjacent high bytes
often have similar values. For example, a row of 16-bit pixels where values
change slowly will have many identical high bytes, producing long runs that
PackBits compresses efficiently.

## Supported Image Types

| Pixel Type | Segments | Description |
|------------|----------|-------------|
| `u8` | 1 | 8-bit grayscale pixels |
| `u16` | 2 | High/low byte planes |

## DICOM RLE Format

```
Header (64 bytes):
  Bytes 0-3:   Number of segments (uint32 LE)
  Bytes 4-7:   Offset to segment 1
  Bytes 8-11:  Offset to segment 2
  ...
  Bytes 60-63: Offset to segment 15 (if present)

Segments:
  [PackBits compressed data for segment 1]
  [PackBits compressed data for segment 2]
  ...
```

The header always occupies 64 bytes regardless of the number of segments.
Up to 15 segment offsets can be stored (bytes 4-63, four bytes each).

## Architecture

```
jpegrle/src/
  lib.rs       # Public API (encode, decode, TRANSFER_SYNTAX_UID)
  encode.rs    # Encoder: pixel splitting + PackBits compression
  decode.rs    # Decoder: PackBits decompression + pixel reassembly
  packbits.rs  # PackBits run-length encoding/decoding
  error.rs     # Error types
```

## References

- DICOM Part 5, Section 8.2.2 / Annex G (RLE Compression)
- DICOM Transfer Syntax: `1.2.840.10008.1.2.5` (RLE Lossless)
- Apple PackBits compression format
