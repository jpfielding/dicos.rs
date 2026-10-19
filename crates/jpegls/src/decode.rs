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

/// Hard cap on decoded image size (pixels), guarding the row-major
/// `vec![0u16; width*height]` against untrusted SOF55 dimensions. Matches the
/// `jpegli` / `jpegrle` decoders (`1 << 28` = 268 435 456 pixels ~= 512 MiB).
const MAX_PIXELS: usize = 1 << 28;

/// Minimum permitted LSE RESET value (T.87 C.2.4.1.1). A RESET below this
/// would suppress statistics halving and risk i32 overflow on long scans.
const MIN_RESET: i32 = 3;

// ---------------------------------------------------------------------------
// Frame / scan headers + LSE presets
// ---------------------------------------------------------------------------

struct FrameHeader {
    precision: u32,
    height: usize,
    width: usize,
    /// Number of components (Nf). 1 (grayscale) or 3 (e.g. RGB); other counts
    /// are rejected in [`Decoder::read_sof`].
    components: usize,
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

/// A decoded JPEG-LS image with the frame metadata a DICOM caller needs.
///
/// Supports grayscale (`components == 1`) and 3-component **planar** (ILV 0)
/// frames. Line- and sample-interleaved multi-component scans (ILV 1/2) are
/// rejected with [`CodecError::Unsupported`], so `interleave_mode` is always
/// `0` on success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    /// Samples in component-planar order — all of component 0, then component
    /// 1, … (`RRR…GGG…BBB…`). Length `width * height * components`.
    pub samples: Vec<u16>,
    /// Image width in pixels (from the codestream).
    pub width: u32,
    /// Image height in pixels (from the codestream).
    pub height: u32,
    /// Sample precision in bits (2..=16).
    pub bits_per_sample: u8,
    /// Component count (1 or 3).
    pub components: u8,
    /// Interleave mode from the scan headers; `0` (none/planar) on success.
    pub interleave_mode: u8,
    /// NEAR error bound (0 = lossless).
    pub near: u8,
}

/// Decode a JPEG-LS bitstream, reading the geometry from the codestream.
///
/// Unlike [`decode`], this takes no expected dimensions and returns the full
/// frame metadata in [`DecodedImage`], including 3-component planar (ILV 0)
/// frames. T.87 conformant path only (no `LegacyGo`).
pub fn decode_image(data: &[u8]) -> Result<DecodedImage, CodecError> {
    Decoder::new(data).decode_image()
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
        let mut b = self.read_byte()?;
        if b != 0xFF {
            return Err(CodecError::InvalidData(format!(
                "expected 0xFF marker prefix, got 0x{b:02X}"
            )));
        }
        // A marker may be preceded by any number of 0xFF fill bytes
        // (T.81 B.1.1.2); skip them. The first non-0xFF byte is the code.
        while b == 0xFF {
            b = self.read_byte()?;
        }
        let marker = 0xFF00u16 | u16::from(b);

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

        // Reject degenerate / oversized frames before any large allocation.
        // T.87 permits 1..=65535 per axis; also cap the pixel product so a
        // crafted 65535x65535 header cannot request an ~8 GiB buffer.
        if width == 0 || height == 0 {
            return Err(CodecError::InvalidData(
                "SOF55 dimensions must be non-zero".into(),
            ));
        }
        if width
            .checked_mul(height)
            .filter(|&n| n <= MAX_PIXELS)
            .is_none()
        {
            return Err(CodecError::InvalidParameter {
                name: "width*height",
                value: (width as i64).saturating_mul(height as i64),
                allowed: "product <= 268435456 pixels (1<<28)",
            });
        }

        let nf = self.read_byte()?;
        // Grayscale (1) and 3-component (e.g. RGB) frames are supported. The
        // single-scan `decode()` entry remains grayscale-only (it re-checks
        // below); `decode_image()` handles 3-component planar (ILV 0) streams.
        if nf != 1 && nf != 3 {
            return Err(CodecError::Unsupported(format!(
                "JPEG-LS component count {nf} not supported (only 1 or 3)"
            )));
        }

        // Component specs: [Ci, sampling (Hi<<4 | Vi), Tqi] per component.
        // Require 1:1 sampling (no subsampling) and no per-component mapping
        // table (Tq 0); otherwise the scan would be mis-decoded. CharLS
        // enforces the same. The component id Ci is not checked.
        for _ in 0..nf {
            let _ci = self.read_byte()?;
            let sampling = self.read_byte()?;
            let tq = self.read_byte()?;
            if sampling != 0x11 {
                return Err(CodecError::Unsupported(format!(
                    "JPEG-LS component sampling {sampling:#04x} not supported (must be 1:1 = 0x11)"
                )));
            }
            if tq != 0 {
                return Err(CodecError::Unsupported(format!(
                    "JPEG-LS component mapping table {tq} not supported (must be 0)"
                )));
            }
        }
        // Skip any trailing bytes in an over-long segment (payload is 6 + Nf*3).
        let consumed = 6 + nf as usize * 3;
        if payload_len > consumed {
            self.skip(payload_len - consumed)?;
        }

        Ok(FrameHeader {
            precision: u32::from(p),
            height,
            width,
            components: nf as usize,
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
                // A field of 0 means "use the default" for MAXVAL/T1/T2/T3.
                presets.maxval = (maxval > 0).then_some(maxval);
                presets.t1 = (t1 > 0).then_some(t1);
                presets.t2 = (t2 > 0).then_some(t2);
                presets.t3 = (t3 > 0).then_some(t3);
                // RESET must be an explicit, valid halving threshold
                // (T.87 C.2.4.1.1: RESET >= 3). A crafted RESET of 0/1/2 could
                // suppress statistics halving and overflow the A/B accumulators
                // on a long scan, so reject it outright rather than treating 0
                // as a "use default" sentinel.
                if reset < MIN_RESET {
                    return Err(CodecError::InvalidData(format!(
                        "LSE RESET={reset} invalid (must be >= {MIN_RESET})"
                    )));
                }
                presets.reset = Some(reset);
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

    /// `nf` is the frame's component count when SOF55 has already been read
    /// (`None` if SOS precedes it; that stream fails later as missing SOF55).
    fn read_sos(
        &mut self,
        _payload_len: usize,
        nf: Option<usize>,
    ) -> Result<ScanHeader, CodecError> {
        let ns = self.read_byte()?;
        // T.87 C.2.3: 1 <= Ns <= 4, and a scan cannot cover more components
        // than the frame has. Such a header is malformed, not merely a scan
        // shape this decoder lacks, so it is InvalidData rather than
        // Unsupported.
        if ns == 0 || ns > 4 || nf.is_some_and(|nf| usize::from(ns) > nf) {
            let frame = nf.map_or(String::new(), |nf| format!(" for a frame with Nf = {nf}"));
            return Err(CodecError::InvalidData(format!(
                "SOS component count Ns = {ns} is invalid{frame}"
            )));
        }
        if ns != 1 {
            // This decoder codes one component per scan (ILV 0); a scan over
            // several components (ILV 1/2) is unsupported.
            return Err(CodecError::Unsupported(
                "multi-component scan (Ns != 1) not supported".into(),
            ));
        }
        // Component spec: [Csj, mapping-table selector]. The component selector
        // Csj is not checked (CharLS ignores it); a non-zero mapping-table
        // selector references an LSE mapping table this decoder does not apply,
        // so reject it rather than silently mis-decode.
        let _csj = self.read_byte()?;
        let mapping = self.read_byte()?;
        if mapping != 0 {
            return Err(CodecError::Unsupported(format!(
                "SOS mapping-table selector {mapping} not supported (must be 0)"
            )));
        }

        let near = self.read_byte()?;
        let ilv = self.read_byte()?;
        let al_ah = self.read_byte()?;

        if ilv != 0 {
            return Err(CodecError::Unsupported(
                "interleaved scan (ILV != 0) not supported".into(),
            ));
        }
        // Point transform (the Ah/Al byte) must be 0: a non-zero value shifts
        // the samples, which this decoder does not apply.
        if al_ah != 0 {
            return Err(CodecError::InvalidData(format!(
                "SOS point-transform byte {al_ah:#04x} must be 0"
            )));
        }

        Ok(ScanHeader {
            near: i32::from(near),
        })
    }

    fn expect_eoi(&mut self) -> Result<(), CodecError> {
        let b1 = self.read_byte()?;
        let mut b2 = self.read_byte()?;
        // As in `read_marker`, 0xFF fill bytes may precede the EOI (T.81
        // B.1.1.2). DCMTK pads an odd-length stream to even this way by
        // default (`FF FF D9`); skip them. The first non-0xFF byte is the code.
        if b1 == 0xFF {
            while b2 == 0xFF {
                b2 = self.read_byte()?;
            }
        }
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
                    // Tolerate and skip DRI, as CharLS does: a zero restart
                    // interval is a no-op. A non-zero interval that actually
                    // emits RSTn markers in the scan is rejected by the entropy
                    // decoder (scan_error), so restarts are still unsupported —
                    // but a benign DRI segment no longer fails an otherwise
                    // decodable stream.
                    self.skip(length)?;
                }
                MARKER_SOS => {
                    scan = self.read_sos(length, frame.as_ref().map(|f| f.components))?;
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

        // Grayscale-only contract for this entry; 3-component streams go
        // through `decode_image` (which handles planar ILV 0).
        if frame.components != 1 {
            return Err(CodecError::Unsupported(format!(
                "decode() is grayscale-only; {}-component frames need decode_image()",
                frame.components
            )));
        }

        let (max_val, reset) = validate_params(&frame, &presets, scan.near)?;

        let w = exp_width as usize;
        let h = exp_height as usize;
        let mut pixels = vec![0u16; w * h];

        match profile {
            Profile::LegacyGo => {
                let scan_data = &self.data[self.pos..];
                legacy::decode_scan(scan_data, &mut pixels, w, h, max_val, scan.near)?;
            }
            Profile::T87 => {
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

    /// Decode a full image (grayscale or 3-component planar), reading geometry
    /// from the codestream. T.87 path only.
    fn decode_image(&mut self) -> Result<DecodedImage, CodecError> {
        self.expect_marker(MARKER_SOI)?;

        let mut frame: Option<FrameHeader> = None;
        let mut presets = Presets::default();
        let first_scan: ScanHeader;

        loop {
            let (marker, length) = self.read_marker()?;
            match marker {
                MARKER_SOF55 => frame = Some(self.read_sof(length)?),
                MARKER_LSE => self.read_lse(length, &mut presets)?,
                MARKER_DRI => {
                    // Tolerate and skip DRI, as CharLS does: a zero restart
                    // interval is a no-op. A non-zero interval that actually
                    // emits RSTn markers in the scan is rejected by the entropy
                    // decoder (scan_error), so restarts are still unsupported —
                    // but a benign DRI segment no longer fails an otherwise
                    // decodable stream.
                    self.skip(length)?;
                }
                MARKER_SOS => {
                    // read_sos rejects Ns != 1 and ILV != 0, so an interleaved
                    // (ILV 1/2) multi-component scan surfaces here as
                    // Unsupported — interleaved RGB is intentionally deferred.
                    first_scan = self.read_sos(length, frame.as_ref().map(|f| f.components))?;
                    break;
                }
                MARKER_EOI => {
                    return Err(CodecError::InvalidData("unexpected EOI before SOS".into()));
                }
                _ => self.skip(length)?,
            }
        }

        let frame = frame.ok_or_else(|| CodecError::InvalidData("missing SOF55 marker".into()))?;
        let near = first_scan.near;
        let (max_val, reset) = validate_params(&frame, &presets, near)?;

        let w = frame.width;
        let h = frame.height;
        let components = frame.components;
        // w*h*components cannot overflow: read_sof caps w*h at MAX_PIXELS
        // (1<<28) and components <= 3.
        let plane = w * h;
        let mut samples = vec![0u16; plane * components];

        // ILV 0 codes one component per scan; the first SOS is already read.
        for (c, plane_slot) in samples.chunks_mut(plane).enumerate() {
            if c > 0 {
                let (marker, length) = self.read_marker()?;
                if marker != MARKER_SOS {
                    return Err(CodecError::InvalidData(format!(
                        "expected SOS for component {c}, got marker 0x{marker:04X}"
                    )));
                }
                let scan = self.read_sos(length, Some(components))?;
                if scan.near != near {
                    return Err(CodecError::InvalidData(format!(
                        "per-scan NEAR differs across components ({near} then {})",
                        scan.near
                    )));
                }
            }
            let ctx = ContextModel::with_presets(
                max_val, near, reset, presets.t1, presets.t2, presets.t3,
            );
            let scan_start = self.pos;
            let consumed =
                decode_scan_t87(&self.data[scan_start..], plane_slot, w, h, ctx, scan_start)?;
            self.pos = scan_start + consumed;
        }
        self.expect_eoi()?;

        Ok(DecodedImage {
            samples,
            width: w as u32,
            height: h as u32,
            bits_per_sample: frame.precision as u8,
            components: components as u8,
            interleave_mode: 0,
            near: near as u8,
        })
    }
}

/// Validate MAXVAL / NEAR / threshold ordering (T.87 C.2.4.1.1) for a scan and
/// return the effective `(max_val, reset)`. Shared by [`Decoder::decode`] and
/// [`Decoder::decode_image`]. `near` is the scan's NEAR; `presets` carries any
/// LSE ID=1 overrides (a `None` field means "use the spec default").
fn validate_params(
    frame: &FrameHeader,
    presets: &Presets,
    near: i32,
) -> Result<(i32, i32), CodecError> {
    let sof_max_val = (1i32 << frame.precision) - 1;
    let max_val = presets.maxval.unwrap_or(sof_max_val);

    // Effective MAXVAL must be positive and fit the SOF precision range; a
    // preset MAXVAL above (1<<precision)-1 would desync context arithmetic.
    if max_val <= 0 || max_val > sof_max_val {
        return Err(CodecError::InvalidData(format!(
            "LSE MAXVAL={max_val} outside 1..={sof_max_val} for precision {}",
            frame.precision
        )));
    }

    // NEAR validity depends on the effective MAXVAL.
    let near_max = (max_val / 2).min(255);
    if near < 0 || near > near_max {
        return Err(CodecError::InvalidParameter {
            name: "near",
            value: i64::from(near),
            allowed: "0..=min(255, MAXVAL/2)",
        });
    }

    // Thresholds must satisfy NEAR < T1 <= T2 <= T3 <= MAXVAL. Preset fields
    // left at their default fall back to the spec defaults (already valid).
    let (dt1, dt2, dt3) = ContextModel::default_thresholds(max_val, near);
    let t1 = presets.t1.unwrap_or(dt1);
    let t2 = presets.t2.unwrap_or(dt2);
    let t3 = presets.t3.unwrap_or(dt3);
    if !(near < t1 && t1 <= t2 && t2 <= t3 && t3 <= max_val) {
        return Err(CodecError::InvalidData(format!(
            "LSE thresholds violate NEAR<T1<=T2<=T3<=MAXVAL \
             (NEAR={near}, T1={t1}, T2={t2}, T3={t3}, MAXVAL={max_val})"
        )));
    }

    let reset = presets.reset.unwrap_or(DEFAULT_RESET);
    Ok((max_val, reset))
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
            run_mode::check_err_in_range(err_q, range)?;
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
    use crate::encode::{encode_image, encode_scan_t87, encode_with_options};
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

    /// Build SOI + SOF55 + LSE(ID=1) + SOS carrying the given preset fields.
    /// Validation fires before scan decode, so no scan/EOI bytes are needed.
    #[allow(clippy::too_many_arguments)]
    fn lse_preset_stream(
        precision: u8,
        w: u16,
        h: u16,
        maxval: u16,
        t1: u16,
        t2: u16,
        t3: u16,
        reset: u16,
        near: u8,
    ) -> Vec<u8> {
        let mut out: Vec<u8> = vec![0xFF, 0xD8]; // SOI
        out.extend_from_slice(&[0xFF, 0xF7]); // SOF55
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(precision);
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x11, 0x00]);
        out.extend_from_slice(&[0xFF, 0xF8]); // LSE ID=1
        out.extend_from_slice(&13u16.to_be_bytes());
        out.push(1);
        for v in [maxval, t1, t2, t3, reset] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&[0xFF, 0xDA]); // SOS
        out.extend_from_slice(&8u16.to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x00, near, 0, 0]);
        out
    }

    #[test]
    fn sof_zero_dimension_rejected() {
        // height = 0, width = 4 -> reject before allocation.
        let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(8);
        out.extend_from_slice(&0u16.to_be_bytes()); // height
        out.extend_from_slice(&4u16.to_be_bytes()); // width
        out.extend_from_slice(&[1, 1, 0x11, 0x00]);
        assert!(matches!(
            decode(&out, 4, 0),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn sof_huge_dimensions_rejected_not_aborted() {
        // 65535 x 65535 would demand ~8 GiB; the pixel cap must reject it as a
        // clean Err rather than abort or OOM.
        let mut out: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xF7];
        out.extend_from_slice(&11u16.to_be_bytes());
        out.push(8);
        out.extend_from_slice(&65535u16.to_be_bytes()); // height
        out.extend_from_slice(&65535u16.to_be_bytes()); // width
        out.extend_from_slice(&[1, 1, 0x11, 0x00]);
        assert!(matches!(
            decode(&out, 65535, 65535),
            Err(CodecError::InvalidParameter {
                name: "width*height",
                ..
            })
        ));
    }

    #[test]
    fn lse_maxval_exceeds_precision_rejected() {
        // precision 8 -> MAXVAL must be <= 255; a preset MAXVAL of 65535 fails.
        let out = lse_preset_stream(8, 4, 4, 65535, 0, 0, 0, 64, 0);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn lse_threshold_ordering_rejected() {
        // T1 > T2 violates NEAR < T1 <= T2 <= T3 <= MAXVAL.
        let out = lse_preset_stream(8, 4, 4, 0, 100, 50, 200, 64, 0);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn lse_t3_exceeds_maxval_rejected() {
        // Explicit MAXVAL 200, but T3 = 250 > MAXVAL.
        let out = lse_preset_stream(8, 4, 4, 200, 5, 12, 250, 64, 0);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn lse_reset_zero_rejected() {
        // RESET below the T.87 minimum (3) is rejected in read_lse.
        let out = lse_preset_stream(8, 4, 4, 0, 0, 0, 0, 0, 0);
        assert!(matches!(
            decode(&out, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
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
    fn dri_zero_interval_is_tolerated() {
        // A benign DRI segment (restart interval 0) before the scan must not
        // fail an otherwise decodable stream — CharLS accepts it. (A non-zero
        // interval that actually emits RSTn markers is still rejected by the
        // scan decoder.)
        let (w, h) = (4u32, 4u32);
        let pixels: Vec<u16> = (0..16).map(|i| i * 4).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, w, h, &EncodeOptions::default(), &mut buf).unwrap();
        let sos = buf
            .windows(2)
            .position(|p| p == [0xFF, 0xDA])
            .expect("SOS present");
        let mut with_dri = buf[..sos].to_vec();
        with_dri.extend_from_slice(&[0xFF, 0xDD, 0x00, 0x04, 0x00, 0x00]); // DRI, interval 0
        with_dri.extend_from_slice(&buf[sos..]);
        let (decoded, _, _) = decode(&with_dri, w, h).expect("benign DRI should decode");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn fill_bytes_before_marker_tolerated() {
        // 0xFF fill bytes may precede any marker (T.81 B.1.1.2); they must be
        // skipped, not mistaken for a marker.
        let (w, h) = (4u32, 4u32);
        let pixels: Vec<u16> = (0..16).map(|i| i * 4).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, w, h, &EncodeOptions::default(), &mut buf).unwrap();
        // Two 0xFF fill bytes right after SOI, before SOF55.
        let mut filled = buf[..2].to_vec();
        filled.extend_from_slice(&[0xFF, 0xFF]);
        filled.extend_from_slice(&buf[2..]);
        let (decoded, _, _) = decode(&filled, w, h).expect("fill bytes should be skipped");
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn fill_bytes_before_eoi_tolerated() {
        // The EOI after the last scan may be preceded by 0xFF fill bytes too:
        // DCMTK's default padding of an odd-length stream is `FF FF D9`.
        let (w, h) = (4u32, 4u32);
        let pixels: Vec<u16> = (0..16).map(|i| i * 4).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, w, h, &EncodeOptions::default(), &mut buf).unwrap();
        let (scan, eoi) = buf.split_at(buf.len() - 2);
        assert_eq!(eoi, [0xFF, 0xD9]);
        for fill in 1..=3 {
            let mut filled = scan.to_vec();
            filled.extend_from_slice(&vec![0xFF; fill]);
            filled.extend_from_slice(eoi);
            let (decoded, _, _) = decode(&filled, w, h).expect("fill bytes should be skipped");
            assert_eq!(decoded, pixels, "{fill} fill byte(s), decode");
            let image = decode_image(&filled).expect("fill bytes should be skipped");
            assert_eq!(image.samples, pixels, "{fill} fill byte(s), decode_image");
        }
    }

    #[test]
    fn fill_bytes_without_eoi_rejected() {
        // A run of 0xFF fill bytes that ends the data, or that ends in a
        // marker other than EOI, is still not an EOI.
        let (w, h) = (4u32, 4u32);
        let pixels: Vec<u16> = (0..16).map(|i| i * 4).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, w, h, &EncodeOptions::default(), &mut buf).unwrap();
        let scan = &buf[..buf.len() - 2];
        for tail in [&[0xFF][..], &[0xFF, 0xFF], &[0xFF, 0xFF, 0xD8]] {
            let mut bad = scan.to_vec();
            bad.extend_from_slice(tail);
            assert!(decode(&bad, w, h).is_err(), "tail {tail:02X?}");
            assert!(decode_image(&bad).is_err(), "tail {tail:02X?}");
        }
    }

    fn encode_mono_4x4() -> (Vec<u8>, Vec<u16>) {
        let pixels: Vec<u16> = (0..16).map(|i| i * 4).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, 4, 4, &EncodeOptions::default(), &mut buf).unwrap();
        (buf, pixels)
    }

    #[test]
    fn sos_mapping_table_and_point_transform_rejected() {
        // SOS: FF DA | Ls(2) | Ns | Csj | mapping | NEAR | ILV | Ah/Al.
        let (buf, _) = encode_mono_4x4();
        let sos = buf.windows(2).position(|p| p == [0xFF, 0xDA]).unwrap();
        let mut mapping = buf.clone();
        mapping[sos + 6] = 1; // non-zero mapping-table selector
        assert!(matches!(
            decode(&mapping, 4, 4),
            Err(CodecError::Unsupported(_))
        ));
        let mut point = buf.clone();
        point[sos + 9] = 3; // non-zero point transform
        assert!(matches!(
            decode(&point, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn sos_component_count_is_validated_against_the_frame() {
        // An impossible Ns (0, > 4, or more than the frame's Nf) is malformed;
        // a legal multi-component scan (ILV 1/2) is merely unsupported.
        let (mono, _) = encode_mono_4x4();
        let sos = mono.windows(2).position(|p| p == [0xFF, 0xDA]).unwrap();
        for ns in [0u8, 2, 5, 7] {
            let mut bad = mono.clone();
            bad[sos + 4] = ns;
            match decode(&bad, 4, 4) {
                Err(CodecError::InvalidData(m)) => assert!(m.contains("Ns = "), "{m}"),
                other => panic!("mono Ns = {ns}: {other:?}"),
            }
        }

        let planes: Vec<Vec<u16>> = (0..3).map(|c| vec![c * 40; 16]).collect();
        let rgb = build_planar_rgb(&planes, 4, 4, 8);
        let sos = rgb.windows(2).position(|p| p == [0xFF, 0xDA]).unwrap();
        let mut interleaved = rgb.clone();
        interleaved[sos + 4] = 3;
        assert!(matches!(
            decode_image(&interleaved),
            Err(CodecError::Unsupported(_))
        ));
        let mut too_many = rgb.clone();
        too_many[sos + 4] = 4;
        assert!(matches!(
            decode_image(&too_many),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn sof_subsampling_and_component_mapping_rejected() {
        // SOF55: FF F7 | Ls(2) | P | H(2) | W(2) | Nf | [Ci, sampling, Tq]...
        let (buf, _) = encode_mono_4x4();
        let sof = buf.windows(2).position(|p| p == [0xFF, 0xF7]).unwrap();
        let mut sub = buf.clone();
        sub[sof + 11] = 0x22; // 2:2 subsampling, not 1:1
        assert!(matches!(
            decode(&sub, 4, 4),
            Err(CodecError::Unsupported(_))
        ));
        let mut tq = buf.clone();
        tq[sof + 12] = 1; // per-component mapping table
        assert!(matches!(decode(&tq, 4, 4), Err(CodecError::Unsupported(_))));
    }

    /// Assemble a 3-component, planar (ILV 0) JPEG-LS stream: one SOS + scan
    /// per plane, sharing one SOF55 (Nf = 3). Lossless, default presets.
    fn build_planar_rgb(planes: &[Vec<u16>], w: usize, h: usize, precision: u8) -> Vec<u8> {
        let nf = planes.len();
        let max_val = (1i32 << precision) - 1;
        let mut out: Vec<u8> = vec![0xFF, 0xD8]; // SOI
                                                 // SOF55: Ls = 8 + 3*Nf.
        out.extend_from_slice(&[0xFF, 0xF7]);
        out.extend_from_slice(&((8 + 3 * nf) as u16).to_be_bytes());
        out.push(precision);
        out.extend_from_slice(&(h as u16).to_be_bytes());
        out.extend_from_slice(&(w as u16).to_be_bytes());
        out.push(nf as u8);
        for c in 0..nf {
            out.extend_from_slice(&[(c as u8) + 1, 0x11, 0x00]); // id, sampling 1:1, Tq 0
        }
        // One SOS + entropy scan per component (ILV 0).
        for (c, plane) in planes.iter().enumerate() {
            out.extend_from_slice(&[0xFF, 0xDA]);
            out.extend_from_slice(&8u16.to_be_bytes()); // Ls = 8 for Ns = 1
            out.push(1); // Ns = 1
            out.extend_from_slice(&[(c as u8) + 1, 0x00]); // Csj, mapping table 0
            out.extend_from_slice(&[0x00, 0x00, 0x00]); // NEAR = 0, ILV = 0, point transform 0
            let ctx = ContextModel::new(max_val, 0, DEFAULT_RESET);
            encode_scan_t87(&mut out, plane, w, h, ctx).unwrap();
        }
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        out
    }

    #[test]
    fn decode_is_grayscale_only() {
        // A well-formed 3-component stream is rejected by the grayscale-only
        // decode() entry; decode_image() handles it (next test).
        let plane: Vec<u16> = (0..16).collect();
        let s = build_planar_rgb(&[plane.clone(), plane.clone(), plane], 4, 4, 8);
        assert!(matches!(decode(&s, 4, 4), Err(CodecError::Unsupported(_))));
    }

    #[test]
    fn decode_image_rgb_planar_roundtrip() {
        let (w, h) = (5usize, 3usize);
        let n = (w * h) as u16;
        let r: Vec<u16> = (0..n).map(|i| i * 3 % 256).collect();
        let g: Vec<u16> = (0..n).map(|i| (i * 7 + 1) % 256).collect();
        let b: Vec<u16> = (0..n).map(|i| 200u16.wrapping_sub(i * 5) % 256).collect();
        let s = build_planar_rgb(&[r.clone(), g.clone(), b.clone()], w, h, 8);
        let img = decode_image(&s).expect("decode_image");
        assert_eq!((img.width, img.height), (w as u32, h as u32));
        assert_eq!(img.components, 3);
        assert_eq!(img.interleave_mode, 0);
        assert_eq!((img.near, img.bits_per_sample), (0, 8));
        // Component-planar layout: R plane, then G, then B.
        let mut expected = r;
        expected.extend_from_slice(&g);
        expected.extend_from_slice(&b);
        assert_eq!(img.samples, expected);
    }

    #[test]
    fn decode_image_mono_matches_source() {
        let (w, h) = (7u32, 5u32);
        let pixels: Vec<u16> = (0..w * h).map(|i| (i * 3 % 256) as u16).collect();
        let mut buf = Vec::new();
        encode_with_options(&pixels, w, h, &EncodeOptions::default(), &mut buf).unwrap();
        let img = decode_image(&buf).expect("decode_image mono");
        assert_eq!(img.components, 1);
        assert_eq!((img.width, img.height), (w, h));
        assert_eq!(img.samples, pixels);
    }

    #[test]
    fn encode_image_roundtrips_through_decode_image() {
        // Mono.
        let mono: Vec<u16> = (0..35u16).map(|i| i * 3 % 256).collect();
        let mut buf = Vec::new();
        encode_image(&mono, 7, 5, 1, &EncodeOptions::default(), &mut buf).unwrap();
        let img = decode_image(&buf).unwrap();
        assert_eq!((img.components, img.width, img.height), (1, 7, 5));
        assert_eq!(img.samples, mono);

        // 3-component planar (ILV 0).
        let (w, h) = (5u32, 3u32);
        let n = (w * h) as u16;
        let mut planar: Vec<u16> = Vec::new();
        planar.extend((0..n).map(|i| i * 3 % 256));
        planar.extend((0..n).map(|i| (i * 7 + 1) % 256));
        planar.extend((0..n).map(|i| 200u16.wrapping_sub(i * 5) % 256));
        let mut buf = Vec::new();
        encode_image(&planar, w, h, 3, &EncodeOptions::default(), &mut buf).unwrap();
        let img = decode_image(&buf).unwrap();
        assert_eq!((img.components, img.interleave_mode), (3, 0));
        assert_eq!(img.samples, planar);
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
