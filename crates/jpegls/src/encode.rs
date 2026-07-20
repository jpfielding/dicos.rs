//! JPEG-LS encoder.
//!
//! Writes a compliant JPEG-LS bitstream (SOI, SOF55, SOS, entropy-coded
//! scan data, EOI) for a single-component grayscale image. The default
//! [`Profile::T87`] path is ITU-T T.87 conformant (single-bit stuffing,
//! limited Golomb, run mode); [`Profile::LegacyGo`] reproduces the frozen
//! 1.0.0 / Go-compatible bytes.

use std::io::Write;

use crate::bitstream::BitWriter;
use crate::context::ContextModel;
use crate::error::CodecError;
use crate::predictor::{clamp, predict_med};
use crate::run_mode;
use crate::{EncodeOptions, Profile};

use crate::legacy;

// ---------------------------------------------------------------------------
// JPEG-LS markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_SOF55: u16 = 0xFFF7;

/// Default statistics reset threshold (T.87 A.2.1 RESET).
const DEFAULT_RESET: i32 = 64;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into JPEG-LS lossless format.
///
/// `pixels` is a row-major pixel buffer of length `width * height`.
/// The output is written to `w`. Uses [`EncodeOptions::default`]
/// ([`Profile::T87`], lossless).
pub fn encode(
    pixels: &[u16],
    width: u32,
    height: u32,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    encode_with_options(pixels, width, height, &EncodeOptions::default(), w)
}

/// Encode a 16-bit grayscale image with explicit [`EncodeOptions`].
pub fn encode_with_options(
    pixels: &[u16],
    width: u32,
    height: u32,
    opts: &EncodeOptions,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    // Dimensions: JPEG-LS SOF55 stores X/Y as 16-bit, and 0 is illegal.
    if width == 0 || width > 65535 {
        return Err(CodecError::InvalidParameter {
            name: "width",
            value: i64::from(width),
            allowed: "1..=65535",
        });
    }
    if height == 0 || height > 65535 {
        return Err(CodecError::InvalidParameter {
            name: "height",
            value: i64::from(height),
            allowed: "1..=65535",
        });
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let expected = width_usize * height_usize;
    if pixels.len() != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    // Precision: explicit override (validated 2..=16) or derived from data.
    let max_pixel = pixels.iter().copied().max().unwrap_or(0);
    let precision = match opts.precision {
        Some(p) => {
            if !(2..=16).contains(&p) {
                return Err(CodecError::InvalidParameter {
                    name: "precision",
                    value: i64::from(p),
                    allowed: "2..=16",
                });
            }
            u32::from(p)
        }
        None => effective_precision(max_pixel),
    };
    let max_val: i32 = (1i32 << precision) - 1;

    if i32::from(max_pixel) > max_val {
        return Err(CodecError::InvalidParameter {
            name: "precision",
            value: i64::from(precision as u16),
            allowed: "large enough to hold every sample",
        });
    }

    // NEAR: bounded by min(255, MAXVAL/2) (T.87 C.2.4.1.1).
    let near = i32::from(opts.near);
    let near_max = (max_val / 2).min(255);
    if near > near_max {
        return Err(CodecError::InvalidParameter {
            name: "near",
            value: i64::from(opts.near),
            allowed: "0..=min(255, MAXVAL/2)",
        });
    }

    match opts.profile {
        Profile::LegacyGo => {
            if near != 0 {
                return Err(CodecError::InvalidParameter {
                    name: "near",
                    value: i64::from(opts.near),
                    allowed: "0 (LegacyGo profile is lossless only)",
                });
            }
            write_headers(w, precision, height as u16, width as u16, 0)?;
            legacy::encode_scan(w, pixels, width_usize, height_usize, max_val, 0)?;
            write_marker(w, MARKER_EOI)?;
        }
        Profile::T87 => {
            write_headers(w, precision, height as u16, width as u16, opts.near)?;
            let ctx = ContextModel::new(max_val, near, DEFAULT_RESET);
            encode_scan_t87(w, pixels, width_usize, height_usize, ctx)?;
            write_marker(w, MARKER_EOI)?;
        }
    }

    w.flush()?;
    Ok(())
}

/// Write SOI, SOF55, and SOS headers for a single-component image.
fn write_headers(
    w: &mut dyn Write,
    precision: u32,
    height: u16,
    width: u16,
    near: u8,
) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOI)?;
    write_sof(w, precision as u8, height, width, 1)?;
    write_sos(w, 1, near)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// T.87 conformant scan (regular + run mode)
// ---------------------------------------------------------------------------

/// Encode the entropy-coded scan for a single-component image (T.87 A.4-A.7).
///
/// Takes a pre-built [`ContextModel`] so callers (and tests) can inject LSE
/// preset thresholds; the public path builds a default model.
pub(crate) fn encode_scan_t87(
    out: &mut dyn Write,
    pixels: &[u16],
    width: usize,
    height: usize,
    mut ctx: ContextModel,
) -> Result<(), CodecError> {
    let mut bw = BitWriter::new(out);
    let near = ctx.near;
    let max_val = ctx.max_val;
    let range = ctx.range;

    let mut prev = vec![0i32; width + 2];
    let mut cur = vec![0i32; width + 2];
    let mut src_row = vec![0i32; width];

    for y in 0..height {
        // T.87 A.2.1 edge seeds: Ra(0,y) = Rb, Rd(width-1,y) = Rb.
        cur[0] = prev[1];
        prev[width + 1] = prev[width];

        for (dst, &px) in src_row.iter_mut().zip(&pixels[y * width..(y + 1) * width]) {
            *dst = i32::from(px);
        }

        let mut x = 0usize;
        while x < width {
            let ra = cur[x];
            let rb = prev[x + 1];
            let rc = prev[x];
            let rd = prev[x + 2];

            let d1 = rd - rb;
            let d2 = rb - rc;
            let d3 = rc - ra;

            if d1.abs() <= near && d2.abs() <= near && d3.abs() <= near {
                x = run_mode::encode_run(&mut bw, &mut ctx, &src_row, &mut cur, &prev, x, width)?;
                continue;
            }

            let (q, sign) = ctx.get_context_index(d1, d2, d3);
            let k = ctx.compute_k(q);
            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let ix = src_row[x];
            let err_q = run_mode::modulo_range(run_mode::quantize((ix - px) * sign, near), range);

            let correction = if k == 0 && near == 0 && 2 * ctx.b[q] <= -ctx.n[q] {
                -1
            } else {
                0
            };
            let mapped = run_mode::map_error(err_q, correction);
            bw.write_limited_golomb(k, mapped, ctx.limit, ctx.qbpp)?;
            ctx.update_stats(q, err_q);

            cur[x + 1] = run_mode::fix_reconstructed(
                px + sign * err_q * (2 * near + 1),
                near,
                range,
                max_val,
            );
            x += 1;
        }

        std::mem::swap(&mut prev, &mut cur);
    }

    bw.flush()?;
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

        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0xD8);
        let n = buf.len();
        assert_eq!(buf[n - 2], 0xFF);
        assert_eq!(buf[n - 1], 0xD9);
    }

    #[test]
    fn encode_uniform_image() {
        let pixels = vec![0u16; 16];
        let mut buf = Vec::new();
        encode(&pixels, 4, 4, &mut buf).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn encode_rejects_oversize_dimension() {
        let pixels = vec![0u16; 4];
        let mut buf = Vec::new();
        assert!(matches!(
            encode_with_options(&pixels, 65536, 1, &EncodeOptions::default(), &mut buf),
            Err(CodecError::InvalidParameter { name: "width", .. })
        ));
    }

    #[test]
    fn encode_legacy_rejects_near() {
        let pixels = vec![0u16; 4];
        let mut buf = Vec::new();
        let opts = EncodeOptions {
            near: 1,
            profile: Profile::LegacyGo,
            precision: None,
        };
        assert!(matches!(
            encode_with_options(&pixels, 2, 2, &opts, &mut buf),
            Err(CodecError::InvalidParameter { name: "near", .. })
        ));
    }
}
