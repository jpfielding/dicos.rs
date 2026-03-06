//! JPEG-LS encoder.
//!
//! Writes a compliant JPEG-LS bitstream (SOI, SOF55, SOS, entropy-coded
//! scan data, EOI) for a single-component grayscale image.

use std::io::Write;

use crate::error::CodecError;

use crate::bitstream::BitWriter;
use crate::context::ContextModel;
use crate::predictor::{clamp, predict_med};

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

    let mut bw = BitWriter::new(w);

    // SOI
    write_marker(&mut bw, MARKER_SOI)?;

    // SOF55
    write_sof(&mut bw, precision as u8, height as u16, width as u16, 1)?;

    // SOS (Near=0, ILV=0)
    write_sos(&mut bw, 1, 0)?;

    // Context model
    let mut ctx = ContextModel::new(max_val, 0, 64);

    // Scan encoding
    encode_scan(
        &mut bw,
        &mut ctx,
        pixels,
        width_usize,
        height_usize,
        max_val,
    )?;

    // Flush remaining bits
    bw.flush()?;

    // EOI -- write through the inner writer since we just flushed.
    bw.write_byte((MARKER_EOI >> 8) as u8)?;
    bw.write_byte((MARKER_EOI & 0xFF) as u8)?;

    // Final flush
    bw.inner_mut().flush()?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Marker writers
// ---------------------------------------------------------------------------

fn write_marker<W: Write>(bw: &mut BitWriter<W>, marker: u16) -> Result<(), CodecError> {
    bw.write_byte((marker >> 8) as u8)?;
    bw.write_byte((marker & 0xFF) as u8)?;
    Ok(())
}

fn write_sof<W: Write>(
    bw: &mut BitWriter<W>,
    precision: u8,
    height: u16,
    width: u16,
    components: u8,
) -> Result<(), CodecError> {
    write_marker(bw, MARKER_SOF55)?;

    // Length: 2 + 1(P) + 2(Y) + 2(X) + 1(Nf) + Nf*3
    let length: u16 = 8 + u16::from(components) * 3;
    bw.write_u16be(length)?;

    bw.write_byte(precision)?;
    bw.write_u16be(height)?;
    bw.write_u16be(width)?;
    bw.write_byte(components)?;

    for i in 0..components {
        bw.write_byte(i + 1)?; // Component ID
        bw.write_byte(0x11)?; // H=1, V=1
        bw.write_byte(0x00)?; // Tq=0
    }
    Ok(())
}

fn write_sos<W: Write>(bw: &mut BitWriter<W>, components: u8, near: u8) -> Result<(), CodecError> {
    write_marker(bw, MARKER_SOS)?;

    // Length: 2 + 1(Ns) + Ns*2 + 3
    let length: u16 = 6 + u16::from(components) * 2;
    bw.write_u16be(length)?;

    bw.write_byte(components)?;

    for i in 0..components {
        bw.write_byte(i + 1)?; // Component ID
        bw.write_byte(0x00)?; // Mapping table selector
    }

    bw.write_byte(near)?; // Near
    bw.write_byte(0x00)?; // ILV = 0
    bw.write_byte(0x00)?; // Al=0, Ah=0

    Ok(())
}

// ---------------------------------------------------------------------------
// Scan encoder
// ---------------------------------------------------------------------------

fn encode_scan<W: Write>(
    bw: &mut BitWriter<W>,
    ctx: &mut ContextModel,
    pixels: &[u16],
    w: usize,
    h: usize,
    max_val: i32,
) -> Result<(), CodecError> {
    let mut curr_line = vec![0i32; w];
    let mut prev_line = vec![0i32; w];

    let range_val = max_val + 1;

    for y in 0..h {
        ctx.run_index = 0;

        // Read current line into curr_line.
        for x in 0..w {
            curr_line[x] = i32::from(pixels[y * w + x]);
        }

        let mut x = 0usize;
        while x < w {
            // Compute neighbours.
            let ra = if x > 0 {
                curr_line[x - 1]
            } else if y > 0 {
                prev_line[0]
            } else {
                0
            };

            let rb = if y > 0 { prev_line[x] } else { 0 };

            let rc = if y > 0 {
                if x > 0 {
                    prev_line[x - 1]
                } else {
                    prev_line[0]
                }
            } else {
                0
            };

            let rd = if y > 0 {
                if x < w - 1 {
                    prev_line[x + 1]
                } else {
                    rb
                }
            } else {
                0
            };

            // Gradients
            let d1 = rd - rb;
            let d2 = rb - rc;
            let d3 = rc - ra;

            // Run mode is intentionally disabled for compatibility with
            // existing DICOS files produced by the Go codec.
            // Regular mode:
            let (q, sign) = ctx.get_context_index(d1, d2, d3);

            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let ix = curr_line[x];
            let mut err_val = ix - px;
            if sign == -1 {
                err_val = -err_val;
            }

            // Modulo reduction.
            if err_val < -range_val / 2 {
                err_val += range_val;
            }
            if err_val > range_val / 2 {
                err_val -= range_val;
            }

            // Map error to non-negative.
            let mapped = if err_val >= 0 {
                (2 * err_val) as u32
            } else {
                (-2 * err_val - 1) as u32
            };

            let k = ctx.compute_k(q);
            bw.write_golomb(k, mapped)?;

            ctx.update_stats(q, err_val);
            x += 1;
        }

        prev_line.copy_from_slice(&curr_line);
    }
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
