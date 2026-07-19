//! JPEG-LS decoder.
//!
//! Parses SOI, SOF55, LSE, SOS markers and then entropy-decodes the scan data
//! to reconstruct a 16-bit grayscale image. The default [`Profile::T87`] path
//! is ITU-T T.87 conformant; [`Profile::LegacyGo`] reads frozen 1.0.0 bytes.

use crate::bitstream::BitReader;
use crate::context::ContextModel;
use crate::error::CodecError;
use crate::predictor::{clamp, predict_med};
use crate::run_mode;
use crate::{DecodeOptions, Profile};

use crate::legacy;

// ---------------------------------------------------------------------------
// JPEG-LS markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_SOF55: u16 = 0xFFF7;
const MARKER_LSE: u16 = 0xFFF8;
const MARKER_DRI: u16 = 0xFFDD;

/// Default statistics reset threshold (T.87 A.2.1 RESET).
const DEFAULT_RESET: i32 = 64;

// ---------------------------------------------------------------------------
// Frame / scan headers + LSE presets
// ---------------------------------------------------------------------------

struct FrameHeader {
    precision: u32,
    height: usize,
    width: usize,
}

struct ScanHeader {
    near: i32,
}

/// LSE ID=1 coding-parameter presets (T.87 C.2.4.1.1); `None` = spec default.
#[derive(Default)]
struct Presets {
    maxval: Option<i32>,
    t1: Option<i32>,
    t2: Option<i32>,
    t3: Option<i32>,
    reset: Option<i32>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Decode a JPEG-LS compressed bitstream into a 16-bit grayscale pixel buffer.
///
/// `width` and `height` are the *expected* image dimensions, cross-checked
/// against the SOF55 marker. Uses [`DecodeOptions::default`] ([`Profile::T87`]).
///
/// Returns `(pixels, width, height)` where pixels is a row-major `Vec<u16>`.
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    decode_with_options(data, width, height, &DecodeOptions::default())
}

/// Decode a JPEG-LS bitstream with explicit [`DecodeOptions`].
pub fn decode_with_options(
    data: &[u8],
    width: u32,
    height: u32,
    opts: &DecodeOptions,
) -> Result<(Vec<u16>, u32, u32), CodecError> {
    let mut dec = Decoder::new(data);
    dec.decode(width, height, opts.profile)
}

// ---------------------------------------------------------------------------
// Decoder state
// ---------------------------------------------------------------------------

struct Decoder<'a> {
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
        if !(2..=16).contains(&p) {
            return Err(CodecError::InvalidParameter {
                name: "precision",
                value: i64::from(p),
                allowed: "2..=16",
            });
        }
        let height = self.read_u16be()? as usize;
        let width = self.read_u16be()? as usize;
        let nf = self.read_byte()?;
        if nf != 1 {
            return Err(CodecError::Unsupported(
                "multi-component JPEG-LS (Nf != 1) not supported".into(),
            ));
        }

        // Skip component specs (Nf * 3 bytes).
        let to_skip = payload_len.saturating_sub(6);
        self.skip(to_skip)?;

        Ok(FrameHeader {
            precision: u32::from(p),
            height,
            width,
        })
    }

    /// Parse an LSE segment. Only ID=1 (coding parameters) is supported.
    fn read_lse(&mut self, payload_len: usize, presets: &mut Presets) -> Result<(), CodecError> {
        if payload_len < 1 {
            return Err(CodecError::InvalidData("empty LSE segment".into()));
        }
        let id = self.read_byte()?;
        match id {
            1 => {
                if payload_len < 11 {
                    return Err(CodecError::InvalidData("LSE ID=1 segment too short".into()));
                }
                let maxval = self.read_u16be()? as i32;
                let t1 = self.read_u16be()? as i32;
                let t2 = self.read_u16be()? as i32;
                let t3 = self.read_u16be()? as i32;
                let reset = self.read_u16be()? as i32;
                // A field of 0 means "use the default".
                presets.maxval = (maxval > 0).then_some(maxval);
                presets.t1 = (t1 > 0).then_some(t1);
                presets.t2 = (t2 > 0).then_some(t2);
                presets.t3 = (t3 > 0).then_some(t3);
                presets.reset = (reset > 0).then_some(reset);
                // Skip any trailing bytes in an over-long segment.
                self.skip(payload_len - 11)?;
                Ok(())
            }
            2..=4 => Err(CodecError::Unsupported(format!(
                "LSE ID={id} (mapping tables) not supported"
            ))),
            other => Err(CodecError::InvalidData(format!("unknown LSE ID={other}"))),
        }
    }

    fn read_sos(&mut self, _payload_len: usize) -> Result<ScanHeader, CodecError> {
        let ns = self.read_byte()?;
        if ns != 1 {
            return Err(CodecError::Unsupported(
                "multi-component scan (Ns != 1) not supported".into(),
            ));
        }
        self.skip(ns as usize * 2)?;

        let near = self.read_byte()?;
        let ilv = self.read_byte()?;
        let _al_ah = self.read_byte()?;

        if ilv != 0 {
            return Err(CodecError::Unsupported(
                "interleaved scan (ILV != 0) not supported".into(),
            ));
        }

        Ok(ScanHeader {
            near: i32::from(near),
        })
    }

    fn expect_eoi(&mut self) -> Result<(), CodecError> {
        let b1 = self.read_byte()?;
        let b2 = self.read_byte()?;
        let marker = u16::from(b1) << 8 | u16::from(b2);
        if marker != MARKER_EOI {
            return Err(CodecError::InvalidData(format!(
                "expected EOI (0xFFD9) after scan, got 0x{marker:04X}"
            )));
        }
        Ok(())
    }

    // -- main decode flow --------------------------------------------------

    fn decode(
        &mut self,
        exp_width: u32,
        exp_height: u32,
        profile: Profile,
    ) -> Result<(Vec<u16>, u32, u32), CodecError> {
        self.expect_marker(MARKER_SOI)?;

        let mut frame: Option<FrameHeader> = None;
        let mut presets = Presets::default();
        let scan: ScanHeader;

        loop {
            let (marker, length) = self.read_marker()?;
            match marker {
                MARKER_SOF55 => frame = Some(self.read_sof(length)?),
                MARKER_LSE => self.read_lse(length, &mut presets)?,
                MARKER_DRI => {
                    return Err(CodecError::Unsupported(
                        "JPEG-LS restart intervals (DRI) not supported".into(),
                    ));
                }
                MARKER_SOS => {
                    scan = self.read_sos(length)?;
                    break;
                }
                MARKER_EOI => {
                    return Err(CodecError::InvalidData("unexpected EOI before SOS".into()));
                }
                _ => self.skip(length)?,
            }
        }

        let frame = frame.ok_or_else(|| CodecError::InvalidData("missing SOF55 marker".into()))?;

        if frame.width != exp_width as usize || frame.height != exp_height as usize {
            return Err(CodecError::DimensionMismatch {
                expected: (exp_width as usize) * (exp_height as usize),
                actual: frame.width * frame.height,
            });
        }

        let sof_max_val = (1i32 << frame.precision) - 1;
        let max_val = presets.maxval.unwrap_or(sof_max_val);

        // NEAR validity depends on the effective MAXVAL (T.87 C.2.4.1.1).
        let near_max = (max_val / 2).min(255);
        if scan.near < 0 || scan.near > near_max {
            return Err(CodecError::InvalidParameter {
                name: "near",
                value: i64::from(scan.near),
                allowed: "0..=min(255, MAXVAL/2)",
            });
        }

        let w = exp_width as usize;
        let h = exp_height as usize;
        let mut pixels = vec![0u16; w * h];

        match profile {
            Profile::LegacyGo => {
                let scan_data = &self.data[self.pos..];
                legacy::decode_scan(scan_data, &mut pixels, w, h, max_val, scan.near)?;
            }
            Profile::T87 => {
                let reset = presets.reset.unwrap_or(DEFAULT_RESET);
                let ctx = ContextModel::with_presets(
                    max_val, scan.near, reset, presets.t1, presets.t2, presets.t3,
                );
                let scan_start = self.pos;
                let scan_data = &self.data[scan_start..];
                let consumed = decode_scan_t87(scan_data, &mut pixels, w, h, ctx, scan_start)?;
                self.pos = scan_start + consumed;
                self.expect_eoi()?;
            }
        }

        Ok((pixels, exp_width, exp_height))
    }
}

// ---------------------------------------------------------------------------
// T.87 conformant scan decode (regular + run mode)
// ---------------------------------------------------------------------------

/// Classify a bit-reader error hit mid-scan (before all samples are decoded).
fn scan_error(e: CodecError, offset: usize) -> CodecError {
    match e {
        CodecError::Marker(m) if (0xFFD0..=0xFFD7).contains(&m) => {
            CodecError::Unsupported("JPEG-LS restart markers (RSTn) in scan not supported".into())
        }
        CodecError::Marker(_) | CodecError::InvalidData(_) => CodecError::Truncated {
            offset,
            context: "entropy-coded scan",
        },
        other => other,
    }
}

/// Decode the entropy-coded scan (T.87 A.4-A.7); returns bytes consumed.
pub(crate) fn decode_scan_t87(
    scan_data: &[u8],
    pixels: &mut [u16],
    width: usize,
    height: usize,
    mut ctx: ContextModel,
    base_offset: usize,
) -> Result<usize, CodecError> {
    let mut br = BitReader::new(scan_data);
    let near = ctx.near;
    let max_val = ctx.max_val;
    let range = ctx.range;

    let mut prev = vec![0i32; width + 2];
    let mut cur = vec![0i32; width + 2];

    for y in 0..height {
        cur[0] = prev[1];
        prev[width + 1] = prev[width];

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
                let before = x;
                x = match run_mode::decode_run(&mut br, &mut ctx, &mut cur, &prev, x, width) {
                    Ok(v) => v,
                    Err(e) => return Err(scan_error(e, base_offset + br.pos())),
                };
                for (xi, slot) in (before..x).zip(&cur[before + 1..x + 1]) {
                    pixels[y * width + xi] = *slot as u16;
                }
                continue;
            }

            let (q, sign) = ctx.get_context_index(d1, d2, d3);
            let k = ctx.compute_k(q);
            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let correction = if k == 0 && near == 0 && 2 * ctx.b[q] <= -ctx.n[q] {
                -1
            } else {
                0
            };
            let mapped = match br.read_limited_golomb(k, ctx.limit, ctx.qbpp) {
                Ok(v) => v,
                Err(e) => return Err(scan_error(e, base_offset + br.pos())),
            };
            let err_q = run_mode::unmap_error(mapped, correction);
            ctx.update_stats(q, err_q);

            let recon = run_mode::fix_reconstructed(
                px + sign * err_q * (2 * near + 1),
                near,
                range,
                max_val,
            );
            cur[x + 1] = recon;
            pixels[y * width + x] = recon as u16;
            x += 1;
        }

        std::mem::swap(&mut prev, &mut cur);
    }

    Ok(br.marker_pos())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{encode_scan_t87, encode_with_options};
    use crate::{EncodeOptions, Profile};

    fn roundtrip(pixels: &[u16], w: u32, h: u32) -> Vec<u16> {
        let mut buf = Vec::new();
        encode_with_options(pixels, w, h, &EncodeOptions::default(), &mut buf).expect("encode");
        let (decoded, dw, dh) = decode(&buf, w, h).expect("decode");
        assert_eq!((dw, dh), (w, h));
        decoded
    }

    #[test]
    fn roundtrip_small_gradient() {
        let (w, h) = (7u32, 5u32);
        let pixels: Vec<u16> = (0..w * h).map(|i| (i * 3 % 256) as u16).collect();
        assert_eq!(roundtrip(&pixels, w, h), pixels);
    }

    #[test]
    fn roundtrip_constant_forces_run() {
        let pixels = vec![777u16; 24 * 8];
        assert_eq!(roundtrip(&pixels, 24, 8), pixels);
    }

    #[test]
    fn roundtrip_1x1() {
        assert_eq!(roundtrip(&[42u16], 1, 1), vec![42u16]);
    }

    /// Hand-build a stream carrying a non-default LSE ID=1 preset wrapping our
    /// own scan bytes, and verify it decodes exactly.
    #[test]
    fn lse_id1_presets_honored() {
        let (w, h) = (16usize, 12usize);
        let pixels: Vec<u16> = (0..(w * h) as u32)
            .map(|i| ((i * 5) % 200) as u16)
            .collect();
        let (maxval, t1, t2, t3, reset) = (255i32, 5i32, 12i32, 40i32, 32i32);

        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&[0xFF, 0xD8]); // SOI
                                              // SOF55
        out.extend_from_slice(&[0xFF, 0xF7]);
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(8); // precision
        out.extend_from_slice(&(h as u16).to_be_bytes());
        out.extend_from_slice(&(w as u16).to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x11, 0x00]); // Nf + component spec
                                                    // LSE ID=1
        out.extend_from_slice(&[0xFF, 0xF8]);
        out.extend_from_slice(&13u16.to_be_bytes());
        out.push(1);
        for v in [maxval, t1, t2, t3, reset] {
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
        // SOS
        out.extend_from_slice(&[0xFF, 0xDA]);
        out.extend_from_slice(&8u16.to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x00, 0, 0, 0]);
        // scan encoded with the same presets
        let ctx = ContextModel::with_presets(maxval, 0, reset, Some(t1), Some(t2), Some(t3));
        encode_scan_t87(&mut out, &pixels, w, h, ctx).unwrap();
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI

        let (decoded, _, _) = decode(&out, w as u32, h as u32).expect("LSE decode");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn lse_id2_unsupported() {
        // SOI, SOF55, LSE ID=2 -> Unsupported.
        let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(8);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x11, 0x00]);
        out.extend_from_slice(&[0xFF, 0xF8]);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.push(2); // ID=2
        out.push(0);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn dri_unsupported() {
        let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(8);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x11, 0x00]);
        out.extend_from_slice(&[0xFF, 0xDD]);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn multi_component_unsupported() {
        let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
        out.extend_from_slice(&14u16.to_be_bytes()); // 8 + 3*Nf, Nf=3 -> but length only guards skip
        out.push(8);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.push(3); // Nf = 3
        out.extend_from_slice(&[1, 0x11, 0x00, 2, 0x11, 0x00, 3, 0x11, 0x00]);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn precision_out_of_range_rejected() {
        for bad in [0u8, 1, 17] {
            let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
            out.extend_from_slice(&11u16.to_be_bytes());
            out.push(bad);
            out.extend_from_slice(&4u16.to_be_bytes());
            out.extend_from_slice(&4u16.to_be_bytes());
            out.extend_from_slice(&[1, 1, 0x11, 0x00]);
            assert!(
                matches!(
                    decode(&out, 4, 4),
                    Err(CodecError::InvalidParameter {
                        name: "precision",
                        ..
                    })
                ),
                "precision {bad} should be rejected"
            );
        }
    }

    #[test]
    fn truncated_scan_is_error_not_panic() {
        let pixels: Vec<u16> = (0..64).map(|i| (i * 4) as u16).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, 8, 8, &EncodeOptions::default(), &mut buf).unwrap();
        // Chop off the tail (scan + EOI) mid-stream.
        let truncated = &buf[..buf.len() - 3];
        let err = decode(truncated, 8, 8).unwrap_err();
        assert!(matches!(
            err,
            CodecError::Truncated { .. } | CodecError::InvalidData(_)
        ));
    }

    #[test]
    fn missing_eoi_rejected() {
        let pixels: Vec<u16> = (0..64).map(|i| (i * 4) as u16).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, 8, 8, &EncodeOptions::default(), &mut buf).unwrap();
        // Replace the trailing EOI bytes with garbage.
        let n = buf.len();
        buf[n - 2] = 0x12;
        buf[n - 1] = 0x34;
        assert!(decode(&buf, 8, 8).is_err());
    }

    #[test]
    fn decode_rejects_bad_soi() {
        assert!(decode(&[0x00, 0x00, 0x00, 0x00], 1, 1).is_err());
    }

    #[test]
    fn t87_differs_from_legacy_bytes() {
        let pixels: Vec<u16> = (0..64).map(|i| (i * 4 % 256) as u16).collect();
        let mut t87 = Vec::new();
        encode_with_options(&pixels, 8, 8, &EncodeOptions::default(), &mut t87).unwrap();
        let mut legacy = Vec::new();
        let opts = EncodeOptions {
            near: 0,
            profile: Profile::LegacyGo,
            precision: None,
        };
        encode_with_options(&pixels, 8, 8, &opts, &mut legacy).unwrap();
        assert_ne!(t87, legacy, "default profile must differ from legacy bytes");
        // And each decodes under its own profile.
        let d1 = decode_with_options(&t87, 8, 8, &DecodeOptions::default())
            .unwrap()
            .0;
        let d2 = decode_with_options(
            &legacy,
            8,
            8,
            &DecodeOptions {
                profile: Profile::LegacyGo,
            },
        )
        .unwrap()
        .0;
        assert_eq!(d1, pixels);
        assert_eq!(d2, pixels);
    }
}
