//! JPEG-LS decoder.
//!
//! Parses SOI, SOF55, SOS markers and then entropy-decodes the scan data
//! to reconstruct a 16-bit grayscale image.

use crate::error::CodecError;

use crate::bitstream::BitReader;
use crate::context::ContextModel;
use crate::predictor::{clamp, predict_med};

// ---------------------------------------------------------------------------
// JPEG-LS markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_SOF55: u16 = 0xFFF7;
const MARKER_LSE: u16 = 0xFFF8;

// ---------------------------------------------------------------------------
// Frame / scan headers
// ---------------------------------------------------------------------------

struct FrameHeader {
    precision: u32,
    height: usize,
    width: usize,
    #[allow(dead_code)]
    components: u8,
}

struct ScanHeader {
    #[allow(dead_code)]
    components: u8,
    near: i32,
    #[allow(dead_code)]
    ilv: u8,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Decode a JPEG-LS compressed bitstream into a 16-bit grayscale pixel buffer.
///
/// `width` and `height` are the *expected* image dimensions.  They are
/// cross-checked against the dimensions stored in the SOF55 marker.
///
/// Returns `(pixels, width, height)` where pixels is a row-major `Vec<u16>`.
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    let mut dec = Decoder::new(data);
    dec.decode(width, height)
}

// ---------------------------------------------------------------------------
// Decoder state
// ---------------------------------------------------------------------------

struct Decoder<'a> {
    /// Raw byte slice; `pos` tracks the current read position for
    /// byte-level marker parsing before we hand off to `BitReader`.
    data: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    // -- byte-level helpers ------------------------------------------------

    fn read_byte(&mut self) -> Result<u8, CodecError> {
        if self.pos >= self.data.len() {
            return Err(CodecError::InvalidData(
                "unexpected end of JPEG-LS data".into(),
            ));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_u16be(&mut self) -> Result<u16, CodecError> {
        let hi = self.read_byte()?;
        let lo = self.read_byte()?;
        Ok(u16::from(hi) << 8 | u16::from(lo))
    }

    fn skip(&mut self, n: usize) -> Result<(), CodecError> {
        if self.pos + n > self.data.len() {
            return Err(CodecError::InvalidData(
                "unexpected end of data during skip".into(),
            ));
        }
        self.pos += n;
        Ok(())
    }

    // -- marker parsing ----------------------------------------------------

    fn expect_marker(&mut self, expected: u16) -> Result<(), CodecError> {
        let b1 = self.read_byte()?;
        let b2 = self.read_byte()?;
        let marker = u16::from(b1) << 8 | u16::from(b2);
        if marker != expected {
            return Err(CodecError::InvalidData(format!(
                "expected marker 0x{expected:04X}, got 0x{marker:04X}"
            )));
        }
        Ok(())
    }

    fn read_marker(&mut self) -> Result<(u16, usize), CodecError> {
        let b1 = self.read_byte()?;
        if b1 != 0xFF {
            return Err(CodecError::InvalidData(format!(
                "expected 0xFF, got 0x{b1:02X}"
            )));
        }
        let b2 = self.read_byte()?;
        let marker = 0xFF00u16 | u16::from(b2);

        let length = self.read_u16be()? as usize;
        // Length field includes its own 2 bytes.
        Ok((marker, length.saturating_sub(2)))
    }

    fn read_sof(&mut self, payload_len: usize) -> Result<FrameHeader, CodecError> {
        let p = self.read_byte()?;
        let height = self.read_u16be()? as usize;
        let width = self.read_u16be()? as usize;
        let nf = self.read_byte()?;

        // Skip component specs (Nf * 3 bytes).
        let to_skip = payload_len.saturating_sub(6);
        self.skip(to_skip)?;

        Ok(FrameHeader {
            precision: u32::from(p),
            height,
            width,
            components: nf,
        })
    }

    fn read_sos(&mut self, _payload_len: usize) -> Result<ScanHeader, CodecError> {
        let ns = self.read_byte()?;
        // Skip component specs (Ns * 2 bytes).
        self.skip(ns as usize * 2)?;

        let near = self.read_byte()?;
        let ilv = self.read_byte()?;
        let _al_ah = self.read_byte()?;

        Ok(ScanHeader {
            components: ns,
            near: i32::from(near),
            ilv,
        })
    }

    // -- main decode flow --------------------------------------------------

    fn decode(
        &mut self,
        exp_width: u32,
        exp_height: u32,
    ) -> Result<(Vec<u16>, u32, u32), CodecError> {
        // 1. SOI
        self.expect_marker(MARKER_SOI)?;

        // 2. Parse markers until SOS.
        let mut frame: Option<FrameHeader> = None;
        let scan: ScanHeader;

        loop {
            let (marker, length) = self.read_marker()?;
            match marker {
                MARKER_SOF55 => {
                    frame = Some(self.read_sof(length)?);
                }
                MARKER_LSE => {
                    self.skip(length)?;
                }
                MARKER_SOS => {
                    scan = self.read_sos(length)?;
                    break;
                }
                MARKER_EOI => {
                    return Err(CodecError::InvalidData("unexpected EOI before SOS".into()));
                }
                _ => {
                    self.skip(length)?;
                }
            }
        }

        let frame = frame.ok_or_else(|| CodecError::InvalidData("missing SOF55 marker".into()))?;

        // Cross-check dimensions.
        if frame.width != exp_width as usize || frame.height != exp_height as usize {
            return Err(CodecError::DimensionMismatch {
                expected: (exp_width as usize) * (exp_height as usize),
                actual: frame.width * frame.height,
            });
        }

        let max_val = (1i32 << frame.precision) - 1;
        let mut ctx = ContextModel::new(max_val, scan.near, 64);

        // The rest of the data (from current `pos`) is the entropy-coded scan.
        let scan_data = &self.data[self.pos..];
        let mut br = BitReader::new(scan_data);

        let w = exp_width as usize;
        let h = exp_height as usize;
        let mut pixels = vec![0u16; w * h];
        decode_scan(&mut br, &mut ctx, &mut pixels, w, h, max_val)?;

        Ok((pixels, exp_width, exp_height))
    }
}

// ---------------------------------------------------------------------------
// Scan decoder
// ---------------------------------------------------------------------------

fn decode_scan(
    br: &mut BitReader<'_>,
    ctx: &mut ContextModel,
    pixels: &mut [u16],
    w: usize,
    h: usize,
    max_val: i32,
) -> Result<(), CodecError> {
    let mut curr_line = vec![0i32; w];
    let mut prev_line = vec![0i32; w];

    let max_val_plus1 = max_val + 1;

    for y in 0..h {
        ctx.run_index = 0;

        let mut x = 0usize;
        while x < w {
            // Neighbours.
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

            // Gradients.
            let d1 = rd - rb;
            let d2 = rb - rc;
            let d3 = rc - ra;

            // Run mode is intentionally disabled for compatibility with
            // existing DICOS files produced by the Go codec. Regular mode:
            let (q, sign) = ctx.get_context_index(d1, d2, d3);

            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let k = ctx.compute_k(q);
            let mapped_err = match br.read_golomb(k) {
                Ok(v) => v,
                Err(e) => {
                    // A marker-encountered error during the last pixels of
                    // the image is normal (EOI marker).
                    if e.to_string().contains("marker encountered") {
                        return Ok(());
                    }
                    return Err(e);
                }
            };

            // Inverse-map the error (using wrapping to handle large values).
            let em = mapped_err as i32;
            let stats_err = if em & 1 == 0 {
                em >> 1
            } else {
                em.wrapping_add(1).wrapping_neg() >> 1
            };

            let mut err_val = stats_err;
            if sign == -1 {
                err_val = -err_val;
            }

            ctx.update_stats(q, stats_err);

            // Use wrapping arithmetic -- the intermediate value can exceed
            // i32 range for 16-bit images. Modulo reduction brings it back.
            let mut rx = (px as i64 + err_val as i64) as i32;

            // Modulo reduction to [0, max_val].
            if rx < 0 {
                rx += max_val_plus1;
            }
            if rx > max_val {
                rx -= max_val_plus1;
            }
            rx = rx.clamp(0, max_val);

            curr_line[x] = rx;
            pixels[y * w + x] = rx as u16;

            x += 1;
        }

        // Copy to pixel buffer (the curr_line was written pixel-by-pixel in
        // run_mode but regular mode only stored into curr_line).
        for xi in 0..w {
            pixels[y * w + xi] = curr_line[xi] as u16;
        }

        prev_line.copy_from_slice(&curr_line);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode;

    /// Helper: encode then decode and compare.
    fn roundtrip(pixels: &[u16], w: u32, h: u32) -> Vec<u16> {
        let mut buf = Vec::new();
        encode::encode(pixels, w, h, &mut buf).expect("encode failed");
        let (decoded, _, _) = decode(&buf, w, h).expect("decode failed");
        decoded
    }

    #[test]
    fn roundtrip_uniform_zero() {
        let pixels = vec![0u16; 16];
        let out = roundtrip(&pixels, 4, 4);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_uniform_nonzero() {
        let pixels = vec![128u16; 64];
        let out = roundtrip(&pixels, 8, 8);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_1x1() {
        let pixels = vec![42u16];
        let out = roundtrip(&pixels, 1, 1);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_1x1_zero() {
        let pixels = vec![0u16];
        let out = roundtrip(&pixels, 1, 1);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_single_row() {
        let pixels: Vec<u16> = (0..16).collect();
        let out = roundtrip(&pixels, 16, 1);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_single_column() {
        let pixels: Vec<u16> = (0..16).collect();
        let out = roundtrip(&pixels, 1, 16);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_horizontal_gradient() {
        let w = 32u32;
        let h = 8u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                pixels[(y * w + x) as usize] = (x * 8) as u16;
            }
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_vertical_gradient() {
        let w = 8u32;
        let h = 32u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                pixels[(y * w + x) as usize] = (y * 8) as u16;
            }
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_diagonal_gradient() {
        let w = 16u32;
        let h = 16u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                pixels[(y * w + x) as usize] = ((x + y) * 4) as u16;
            }
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_checkerboard() {
        let w = 8u32;
        let h = 8u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                pixels[(y * w + x) as usize] = if (x + y) % 2 == 0 { 200 } else { 50 };
            }
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_max_8bit() {
        let pixels = vec![255u16; 16];
        let out = roundtrip(&pixels, 4, 4);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_16bit_values() {
        let w = 8u32;
        let h = 4u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for (i, px) in pixels.iter_mut().enumerate() {
            *px = (i as u16).wrapping_mul(2048);
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_odd_dimensions() {
        let w = 7u32;
        let h = 5u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for (i, px) in pixels.iter_mut().enumerate() {
            *px = (i * 3 % 256) as u16;
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_large_image() {
        let w = 64u32;
        let h = 64u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for (i, px) in pixels.iter_mut().enumerate() {
            *px = ((i * 7 + 13) % 256) as u16;
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn roundtrip_alternating_rows() {
        let w = 16u32;
        let h = 8u32;
        let mut pixels = vec![0u16; (w * h) as usize];
        for y in 0..h {
            let val = if y % 2 == 0 { 100u16 } else { 200u16 };
            for x in 0..w {
                pixels[(y * w + x) as usize] = val;
            }
        }
        let out = roundtrip(&pixels, w, h);
        assert_eq!(pixels, out);
    }

    #[test]
    fn decode_rejects_bad_soi() {
        let bad_data = [0x00, 0x00, 0x00, 0x00];
        assert!(decode(&bad_data, 1, 1).is_err());
    }

    #[test]
    fn decode_dimension_mismatch() {
        // Encode a 4x4, then try to decode as 8x8.
        let pixels = vec![42u16; 16];
        let mut buf = Vec::new();
        encode::encode(&pixels, 4, 4, &mut buf).unwrap();

        let result = decode(&buf, 8, 8);
        assert!(result.is_err());
    }
}
