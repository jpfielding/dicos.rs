//! JPEG-LS encoder.
//!
//! Writes a compliant JPEG-LS bitstream (SOI, SOF55, SOS, entropy-coded
//! scan data, EOI) for a single-component grayscale image.

use std::io::Write;

use crate::error::CodecError;

use crate::legacy;

// ---------------------------------------------------------------------------
// JPEG-LS markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_SOF55: u16 = 0xFFF7;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into JPEG-LS lossless format.
///
/// `pixels` is a row-major pixel buffer of length `width * height`.
/// The output is written to `w`.
pub fn encode(
    pixels: &[u16],
    width: u32,
    height: u32,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    let width_usize = width as usize;
    let height_usize = height as usize;
    let expected = width_usize * height_usize;

    if pixels.len() != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    if width_usize == 0 || height_usize == 0 {
        return Err(CodecError::InvalidData(
            "image dimensions must be non-zero".into(),
        ));
    }

    // Determine bit depth from the actual maximum pixel value.
    let max_pixel = pixels.iter().copied().max().unwrap_or(0);
    let precision = effective_precision(max_pixel);
    let max_val: i32 = (1i32 << precision) - 1;

    // SOI
    write_marker(w, MARKER_SOI)?;

    // SOF55
    write_sof(w, precision as u8, height as u16, width as u16, 1)?;

    // SOS (Near=0, ILV=0)
    write_sos(w, 1, 0)?;

    // Entropy-coded scan (frozen 1.0.0 / Go-compatible path).
    legacy::encode_scan(w, pixels, width_usize, height_usize, max_val, 0)?;

    // EOI
    write_marker(w, MARKER_EOI)?;

    w.flush()?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Marker writers
// ---------------------------------------------------------------------------

fn write_marker(w: &mut dyn Write, marker: u16) -> Result<(), CodecError> {
    w.write_all(&marker.to_be_bytes())?;
    Ok(())
}

fn write_sof(
    w: &mut dyn Write,
    precision: u8,
    height: u16,
    width: u16,
    components: u8,
) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOF55)?;

    // Length: 2 + 1(P) + 2(Y) + 2(X) + 1(Nf) + Nf*3
    let length: u16 = 8 + u16::from(components) * 3;
    w.write_all(&length.to_be_bytes())?;

    w.write_all(&[precision])?;
    w.write_all(&height.to_be_bytes())?;
    w.write_all(&width.to_be_bytes())?;
    w.write_all(&[components])?;

    for i in 0..components {
        w.write_all(&[i + 1])?; // Component ID
        w.write_all(&[0x11])?; // H=1, V=1
        w.write_all(&[0x00])?; // Tq=0
    }
    Ok(())
}

fn write_sos(w: &mut dyn Write, components: u8, near: u8) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOS)?;

    // Length: 2 + 1(Ns) + Ns*2 + 3
    let length: u16 = 6 + u16::from(components) * 2;
    w.write_all(&length.to_be_bytes())?;

    w.write_all(&[components])?;

    for i in 0..components {
        w.write_all(&[i + 1])?; // Component ID
        w.write_all(&[0x00])?; // Mapping table selector
    }

    w.write_all(&[near])?; // Near
    w.write_all(&[0x00])?; // ILV = 0
    w.write_all(&[0x00])?; // Al=0, Ah=0

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Determine the effective bit depth needed for the maximum pixel value.
///
/// JPEG-LS supports 2..=16 bits per sample.  We always use at least 8.
fn effective_precision(max_pixel: u16) -> u32 {
    if max_pixel == 0 {
        return 8;
    }
    let bits_needed = 16 - max_pixel.leading_zeros(); // ceil(log2(max+1))
    bits_needed.clamp(8, 16)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_precision_8bit() {
        assert_eq!(effective_precision(0), 8);
        assert_eq!(effective_precision(1), 8);
        assert_eq!(effective_precision(255), 8);
    }

    #[test]
    fn effective_precision_more_than_8() {
        assert_eq!(effective_precision(256), 9);
        assert_eq!(effective_precision(1023), 10);
        assert_eq!(effective_precision(4095), 12);
        assert_eq!(effective_precision(65535), 16);
    }

    #[test]
    fn encode_rejects_empty_image() {
        let pixels: Vec<u16> = vec![];
        let mut buf = Vec::new();
        assert!(encode(&pixels, 0, 0, &mut buf).is_err());
    }

    #[test]
    fn encode_produces_soi_and_eoi() {
        let pixels = vec![42u16; 4];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        // SOI at start
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0xD8);

        // EOI at end
        let n = buf.len();
        assert_eq!(buf[n - 2], 0xFF);
        assert_eq!(buf[n - 1], 0xD9);
    }

    #[test]
    fn encode_uniform_image() {
        // All-zero image should produce valid bitstream.
        let pixels = vec![0u16; 16];
        let mut buf = Vec::new();
        encode(&pixels, 4, 4, &mut buf).unwrap();
        assert!(!buf.is_empty());
    }
}
