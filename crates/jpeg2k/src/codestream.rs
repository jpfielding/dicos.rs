//! JPEG 2000 codestream reader / writer and public encode / decode functions.
//!
//! Generates and parses the conformant (ITU-T T.800) codestream structure:
//! `SOC → SIZ → COD → QCD → SOT → SOD → [LRCP packets] → EOC`.
//!
//! Supported profile (everything else is rejected loudly by the decode support
//! matrix): one tile, one component (unsigned 16-bit), reversible 5/3 DWT, LRCP
//! progression, one quality layer, `cb_style = 0`, zero grid origins.
//!
//! The public [`encode`] emits this conformant stream. [`decode`] parses it and,
//! by default ([`LegacyPolicy::Auto`]), also transparently decodes the
//! non-conformant v1.0.0 / Go "legacy" raw-DWT format (gated behind the
//! `legacy-decode` feature).

use std::io::Write;

use crate::bitstream::{ByteReader, ByteWriter};
use crate::error::CodecError;
use crate::geometry::BandKind;
use crate::markers::*;
use crate::tile::{decode_tile, encode_tile};
use crate::Jpeg2kOptions;

// ---------------------------------------------------------------------------
// Profile constants and the single-source-of-truth quantization helpers
// ---------------------------------------------------------------------------

/// Component bit depth `RI` (unsigned 16-bit). Used as the reversible dynamic
/// range in the E-10 exponent formula.
const BIT_DEPTH: u8 = 16;
/// Guard bits `G` (T.800 E.1) for our profile.
const GUARD_BITS: u8 = 2;
/// DC level shift for unsigned 16-bit samples: `2^(BIT_DEPTH-1) = 32768`.
const DC_SHIFT: i32 = 1 << (BIT_DEPTH - 1);
/// Allocation cap on `Xsiz · Ysiz` (samples), guarding hostile SIZ dimensions.
const MAX_IMAGE_AREA: u64 = 1 << 28;

/// `log2` of the nominal sub-band gain (T.800 E.1 / Table E.1): 0/1/1/2.
fn gain_log2(kind: BandKind) -> u8 {
    match kind {
        BandKind::LL => 0,
        BandKind::HL | BandKind::LH => 1,
        BandKind::HH => 2,
    }
}

/// QCD exponent `εb` for the reversible, no-quantization case (T.800 E-10):
/// `εb = RI + log2(gain_b)`. Level-independent for the reversible transform.
///
/// This is the single source of truth shared by the QCD marker builder and the
/// `mb_for` closure handed to the tile pipeline.
fn qcd_epsilon(kind: BandKind) -> u8 {
    BIT_DEPTH + gain_log2(kind)
}

/// `Mb = εb + G − 1`, the maximum magnitude bit-plane count for a band.
fn mb_from_epsilon(epsilon: u8, guard_bits: u8) -> u32 {
    epsilon as u32 + guard_bits as u32 - 1
}

/// QCD `SPqcd` exponents in T.800 spec order for `num_levels` decompositions:
/// `LL_NL` first, then for `n = NL … 1` the triple `HL_n, LH_n, HH_n`
/// (just `[LL]` when `NL = 0`). Length `3·NL + 1`.
fn qcd_epsilons(num_levels: u8) -> Vec<i16> {
    let mut v = Vec::with_capacity(3 * num_levels as usize + 1);
    v.push(qcd_epsilon(BandKind::LL) as i16);
    for _ in 0..num_levels {
        v.push(qcd_epsilon(BandKind::HL) as i16);
        v.push(qcd_epsilon(BandKind::LH) as i16);
        v.push(qcd_epsilon(BandKind::HH) as i16);
    }
    v
}

/// Index into the spec-order QCD exponent list for band `kind` at decomposition
/// `level` (`1..=NL` for detail bands, `NL` for LL).
fn qcd_band_index(kind: BandKind, level: u8, num_levels: u8) -> usize {
    match kind {
        BandKind::LL => 0,
        BandKind::HL => 1 + 3 * (num_levels - level) as usize,
        BandKind::LH => 2 + 3 * (num_levels - level) as usize,
        BandKind::HH => 3 + 3 * (num_levels - level) as usize,
    }
}

// ---------------------------------------------------------------------------
// Codestream writer (encoder side)
// ---------------------------------------------------------------------------

struct CodestreamWriter {
    bw: ByteWriter,
}

impl CodestreamWriter {
    fn new() -> Self {
        Self {
            bw: ByteWriter::new(),
        }
    }

    fn write_soc(&mut self) {
        self.bw.write_u16(MARKER_SOC);
    }

    fn write_siz(&mut self, siz: &SizMarker) {
        self.bw.write_u16(MARKER_SIZ);
        // Lsiz = 38 + 3 · Csiz.
        let length = 38 + 3 * siz.components.len() as u16;
        self.bw.write_u16(length);
        self.bw.write_u16(siz.rsiz);
        self.bw.write_u32(siz.x_siz);
        self.bw.write_u32(siz.y_siz);
        self.bw.write_u32(siz.x_osiz);
        self.bw.write_u32(siz.y_osiz);
        self.bw.write_u32(siz.x_tsiz);
        self.bw.write_u32(siz.y_tsiz);
        self.bw.write_u32(siz.x_tosiz);
        self.bw.write_u32(siz.y_tosiz);
        self.bw.write_u16(siz.components.len() as u16);
        for comp in &siz.components {
            let mut ssiz = comp.precision - 1;
            if comp.signed {
                ssiz |= 0x80;
            }
            self.bw.write_u8(ssiz);
            self.bw.write_u8(comp.x_rsiz);
            self.bw.write_u8(comp.y_rsiz);
        }
    }

    fn write_cod(&mut self, cod: &CodMarker) {
        self.bw.write_u16(MARKER_COD);
        let mut length = 12u16;
        if cod.scod & CODING_STYLE_PRECINCTS_USER != 0 {
            length += cod.precinct_sizes.len() as u16;
        }
        self.bw.write_u16(length);
        self.bw.write_u8(cod.scod);
        self.bw.write_u8(cod.progression as u8);
        self.bw.write_u16(cod.num_layers);
        self.bw.write_u8(cod.mct);
        self.bw.write_u8(cod.decomp_levels);
        self.bw.write_u8(cod.cb_width_exp);
        self.bw.write_u8(cod.cb_height_exp);
        self.bw.write_u8(cod.cb_style);
        self.bw.write_u8(cod.transform as u8);
        if cod.scod & CODING_STYLE_PRECINCTS_USER != 0 {
            self.bw.write_bytes(&cod.precinct_sizes);
        }
    }

    fn write_qcd(&mut self, qcd: &QcdMarker) {
        self.bw.write_u16(MARKER_QCD);
        let length = 3 + qcd.step_sizes.len() as u16;
        self.bw.write_u16(length);
        let sqcd = (qcd.guard_bits << 5) | (qcd.sqcd & 0x1F);
        self.bw.write_u8(sqcd);
        // Reversible (no-quantization) style: each SPqcd entry is one byte,
        // carrying `εb` in its high 5 bits (`εb << 3`).
        for &eps in &qcd.step_sizes {
            self.bw.write_u8((eps << 3) as u8);
        }
    }

    fn write_sot(&mut self, sot: &SotMarker) {
        self.bw.write_u16(MARKER_SOT);
        self.bw.write_u16(10); // Lsot (fixed for one tile-part)
        self.bw.write_u16(sot.tile_index);
        self.bw.write_u32(sot.tile_part_len);
        self.bw.write_u8(sot.tile_part_idx);
        self.bw.write_u8(sot.num_tile_parts);
    }

    fn write_sod(&mut self) {
        self.bw.write_u16(MARKER_SOD);
    }

    fn write_eoc(&mut self) {
        self.bw.write_u16(MARKER_EOC);
    }

    fn write_bytes(&mut self, data: &[u8]) {
        self.bw.write_bytes(data);
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bw.into_bytes()
    }
}

fn io_err(e: std::io::Error) -> CodecError {
    CodecError::Io(e)
}

// ---------------------------------------------------------------------------
// Public API: encode
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into a conformant JPEG 2000 (T.800)
/// codestream.
///
/// `pixels` is a row-major buffer of length `img_width · img_height`. The
/// resulting codestream is single-tile, single-component, reversible 5/3, LRCP,
/// one quality layer.
pub fn encode(
    pixels: &[u16],
    img_width: u32,
    img_height: u32,
    options: &Jpeg2kOptions,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    options.validate()?;

    let width = img_width as usize;
    let height = img_height as usize;
    let expected = width
        .checked_mul(height)
        .ok_or_else(|| CodecError::InvalidData("image dimensions overflow usize".into()))?;

    if pixels.len() != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixels.len(),
        });
    }
    if width == 0 || height == 0 {
        return Err(CodecError::InvalidData("image has zero dimension".into()));
    }

    let num_levels = options.num_decomp_levels;
    let xcb = options.cb_width_exp; // actual exponents
    let ycb = options.cb_height_exp;

    // DC level shift: unsigned samples → signed, centred about zero.
    let component: Vec<i32> = pixels.iter().map(|&v| v as i32 - DC_SHIFT).collect();

    // SIZ: single unsigned 16-bit component, zero origins, one tile = image.
    let comp_info = vec![ComponentInfo {
        precision: BIT_DEPTH,
        signed: false,
        x_rsiz: 1,
        y_rsiz: 1,
    }];
    let siz = build_siz(img_width, img_height, comp_info, 0, 0);

    // COD from options (stores cb exponents offset by −2 per A.6.1).
    let cod = build_cod(num_levels, xcb, ycb);

    // QCD: real E-10 exponents in spec order, guard bits G = 2.
    let qcd = QcdMarker {
        sqcd: 0,
        guard_bits: GUARD_BITS,
        step_sizes: qcd_epsilons(num_levels),
    };

    // Encode the single tile-component. `mb_for` derives from the same
    // `qcd_epsilon` the QCD marker used — single source of truth.
    let tile_bitstream = encode_tile(
        &component,
        img_width,
        img_height,
        num_levels,
        xcb,
        ycb,
        |kind, _level| mb_from_epsilon(qcd_epsilon(kind), GUARD_BITS),
    )?;

    let mut cw = CodestreamWriter::new();
    cw.write_soc();
    cw.write_siz(&siz);
    cw.write_cod(&cod);
    cw.write_qcd(&qcd);

    // Psot (T.800 A.4.2): length from the first byte of the SOT marker to the
    // end of the tile-part data = SOT segment (12) + SOD marker (2) + data.
    let psot = 12u32
        .checked_add(2)
        .and_then(|v| v.checked_add(tile_bitstream.len() as u32))
        .ok_or_else(|| CodecError::InvalidData("tile-part length overflow".into()))?;
    let sot = SotMarker {
        tile_index: 0,
        tile_part_len: psot,
        tile_part_idx: 0,
        num_tile_parts: 1,
    };
    cw.write_sot(&sot);
    cw.write_sod();
    cw.write_bytes(&tile_bitstream);
    cw.write_eoc();

    let bytes = cw.into_bytes();
    w.write_all(&bytes).map_err(CodecError::Io)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Decode options / legacy policy
// ---------------------------------------------------------------------------

/// How [`decode_with_options`] treats the non-conformant v1.0.0 / Go "legacy"
/// raw-DWT format.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegacyPolicy {
    /// Decode conformant streams; transparently fall back to the legacy format
    /// when its fingerprint is present. The default for [`decode`].
    #[default]
    Auto,
    /// Only decode conformant T.800 streams; reject legacy input.
    StandardOnly,
    /// Only decode legacy streams; reject conformant input.
    LegacyOnly,
}

/// Options controlling [`decode_with_options`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeOptions {
    /// Legacy-format handling policy.
    pub legacy: LegacyPolicy,
}

/// Whether the `legacy-decode` feature (and therefore the legacy decode path) is
/// compiled in.
const LEGACY_AVAILABLE: bool = cfg!(feature = "legacy-decode");

// ---------------------------------------------------------------------------
// Structured main-header + tile-part parse (no marker rescans)
// ---------------------------------------------------------------------------

/// Everything the decoder needs, recovered from a single bounds-checked walk.
struct Parsed {
    cod: CodMarker,
    qcd: QcdMarker,
    img_w: u32,
    img_h: u32,
    /// Byte range of the tile-part data (the LRCP packets / legacy payload),
    /// i.e. the bytes between the SOD marker and the end of the tile-part.
    payload: std::ops::Range<usize>,
}

/// Read one marker segment's length and return the absolute index one past its
/// content, validating the length is at least 2 (it counts its own field).
fn seg_content_end(br: &ByteReader, length: u16) -> Result<usize, CodecError> {
    let content = (length as usize)
        .checked_sub(2)
        .ok_or_else(|| CodecError::InvalidData("marker segment length < 2".into()))?;
    br.position()
        .checked_add(content)
        .ok_or_else(|| CodecError::InvalidData("marker segment length overflow".into()))
}

/// Advance `br` to `end`, erroring if the fields already overran it.
fn seek_to(br: &mut ByteReader, end: usize) -> Result<(), CodecError> {
    if br.position() > end {
        return Err(CodecError::InvalidData(
            "marker segment fields exceed its declared length".into(),
        ));
    }
    br.skip(end - br.position()).map_err(io_err)
}

fn read_siz(br: &mut ByteReader) -> Result<(SizMarker, u32, u32), CodecError> {
    let length = br.read_u16().map_err(io_err)?;
    let end = seg_content_end(br, length)?;
    let rsiz = br.read_u16().map_err(io_err)?;
    let x_siz = br.read_u32().map_err(io_err)?;
    let y_siz = br.read_u32().map_err(io_err)?;
    let x_osiz = br.read_u32().map_err(io_err)?;
    let y_osiz = br.read_u32().map_err(io_err)?;
    let x_tsiz = br.read_u32().map_err(io_err)?;
    let y_tsiz = br.read_u32().map_err(io_err)?;
    let x_tosiz = br.read_u32().map_err(io_err)?;
    let y_tosiz = br.read_u32().map_err(io_err)?;
    let num_comps = br.read_u16().map_err(io_err)?;

    // Support matrix: single component only.
    if num_comps != 1 {
        return Err(CodecError::Unsupported(format!(
            "only single-component images supported (Csiz = {num_comps})"
        )));
    }

    let ssiz = br.read_u8().map_err(io_err)?;
    let x_rsiz = br.read_u8().map_err(io_err)?;
    let y_rsiz = br.read_u8().map_err(io_err)?;
    let signed = (ssiz & 0x80) != 0;
    let precision = (ssiz & 0x7F) + 1;
    // Support matrix: exactly unsigned 16-bit (Ssiz raw value 15).
    if signed || precision != BIT_DEPTH {
        return Err(CodecError::Unsupported(format!(
            "only unsigned 16-bit samples supported (Ssiz = 0x{ssiz:02X})"
        )));
    }

    // Grid-origin support matrix (the whole pipeline pins origins at zero).
    if x_osiz != 0 || y_osiz != 0 || x_tosiz != 0 || y_tosiz != 0 {
        return Err(CodecError::Unsupported(
            "non-zero grid origins not supported".into(),
        ));
    }
    if x_osiz > x_siz || y_osiz > y_siz {
        return Err(CodecError::InvalidData(
            "SIZ origin exceeds image size".into(),
        ));
    }
    if x_tsiz == 0 || y_tsiz == 0 {
        return Err(CodecError::InvalidData(
            "SIZ tile size has a zero dimension".into(),
        ));
    }
    if x_siz == 0 || y_siz == 0 {
        return Err(CodecError::InvalidData(
            "SIZ image has a zero dimension".into(),
        ));
    }
    // Single tile must cover the whole image.
    if x_tsiz < x_siz || y_tsiz < y_siz {
        return Err(CodecError::Unsupported(
            "multi-tile images not supported".into(),
        ));
    }
    // Allocation cap.
    let area = (x_siz as u64)
        .checked_mul(y_siz as u64)
        .ok_or_else(|| CodecError::InvalidData("image area overflow".into()))?;
    if area > MAX_IMAGE_AREA {
        return Err(CodecError::InvalidData(format!(
            "image area {area} exceeds the {MAX_IMAGE_AREA} sample cap"
        )));
    }

    let siz = SizMarker {
        rsiz,
        x_siz,
        y_siz,
        x_osiz,
        y_osiz,
        x_tsiz,
        y_tsiz,
        x_tosiz,
        y_tosiz,
        components: vec![ComponentInfo {
            precision,
            signed,
            x_rsiz,
            y_rsiz,
        }],
    };
    seek_to(br, end)?;
    Ok((siz, x_siz - x_osiz, y_siz - y_osiz))
}

fn read_cod(br: &mut ByteReader) -> Result<CodMarker, CodecError> {
    let length = br.read_u16().map_err(io_err)?;
    let end = seg_content_end(br, length)?;
    let scod = br.read_u8().map_err(io_err)?;
    let prog_byte = br.read_u8().map_err(io_err)?;
    let num_layers = br.read_u16().map_err(io_err)?;
    let mct = br.read_u8().map_err(io_err)?;
    let decomp_levels = br.read_u8().map_err(io_err)?;
    let cb_width_exp = br.read_u8().map_err(io_err)?;
    let cb_height_exp = br.read_u8().map_err(io_err)?;
    let cb_style = br.read_u8().map_err(io_err)?;
    let transform_byte = br.read_u8().map_err(io_err)?;

    // Support matrix (each with a specific message).
    if scod & (CODING_STYLE_PRECINCTS_USER | CODING_STYLE_SOP | CODING_STYLE_EPH) != 0 {
        return Err(CodecError::Unsupported(
            "COD Scod precinct/SOP/EPH flags not supported".into(),
        ));
    }
    let progression = ProgressionOrder::from_byte(prog_byte)
        .ok_or_else(|| CodecError::InvalidData(format!("unknown progression order {prog_byte}")))?;
    if progression != ProgressionOrder::Lrcp {
        return Err(CodecError::Unsupported(
            "only LRCP progression supported".into(),
        ));
    }
    if num_layers != 1 {
        return Err(CodecError::Unsupported(format!(
            "only a single quality layer supported (layers = {num_layers})"
        )));
    }
    if mct != 0 {
        return Err(CodecError::Unsupported(
            "multiple-component transform not supported (MCT != 0)".into(),
        ));
    }
    let transform = TransformType::from_byte(transform_byte).ok_or_else(|| {
        CodecError::InvalidData(format!("unknown transform type {transform_byte}"))
    })?;
    if transform != TransformType::Reversible53 {
        return Err(CodecError::Unsupported(
            "only reversible 5/3 transform supported".into(),
        ));
    }
    if cb_style != 0 {
        return Err(CodecError::Unsupported(format!(
            "code-block style flags not supported (cb_style = 0x{cb_style:02X})"
        )));
    }
    if decomp_levels > 32 {
        return Err(CodecError::InvalidData(format!(
            "decomposition levels {decomp_levels} exceed 32"
        )));
    }
    // Code-block exponent legality (stored value is exp − 2; exp in 2..=10).
    if cb_width_exp > 8 || cb_height_exp > 8 || cb_width_exp + cb_height_exp > 8 {
        return Err(CodecError::InvalidData(format!(
            "illegal code-block exponents (stored {cb_width_exp},{cb_height_exp})"
        )));
    }

    let cod = CodMarker {
        scod,
        progression,
        num_layers,
        mct,
        decomp_levels,
        cb_width_exp,
        cb_height_exp,
        cb_style,
        transform,
        precinct_sizes: Vec::new(),
    };
    seek_to(br, end)?;
    Ok(cod)
}

fn read_qcd(br: &mut ByteReader) -> Result<QcdMarker, CodecError> {
    let length = br.read_u16().map_err(io_err)?;
    let end = seg_content_end(br, length)?;
    let sqcd_byte = br.read_u8().map_err(io_err)?;
    let guard_bits = (sqcd_byte >> 5) & 0x07;
    let q_style = sqcd_byte & 0x1F;
    if q_style != 0 {
        return Err(CodecError::Unsupported(format!(
            "only no-quantization (reversible) QCD supported (style = {q_style})"
        )));
    }
    let remaining = end.saturating_sub(br.position());
    let mut step_sizes = Vec::with_capacity(remaining);
    for _ in 0..remaining {
        let exp = br.read_u8().map_err(io_err)?;
        step_sizes.push((exp >> 3) as i16);
    }
    let qcd = QcdMarker {
        sqcd: q_style,
        guard_bits,
        step_sizes,
    };
    seek_to(br, end)?;
    Ok(qcd)
}

/// Skip an informational (CRG/COM) marker segment.
fn skip_segment(br: &mut ByteReader) -> Result<(), CodecError> {
    let length = br.read_u16().map_err(io_err)?;
    let content = (length as usize)
        .checked_sub(2)
        .ok_or_else(|| CodecError::InvalidData("marker segment length < 2".into()))?;
    br.skip(content).map_err(io_err)
}

/// Reject a legal-but-unsupported marker with a specific message.
fn reject(name: &str) -> CodecError {
    CodecError::Unsupported(format!("{name} marker not supported"))
}

fn parse_codestream(data: &[u8]) -> Result<Parsed, CodecError> {
    let mut br = ByteReader::new(data);

    if br.read_u16().map_err(io_err)? != MARKER_SOC {
        return Err(CodecError::InvalidData("missing SOC marker".into()));
    }

    let mut siz: Option<SizMarker> = None;
    let mut cod: Option<CodMarker> = None;
    let mut qcd: Option<QcdMarker> = None;
    let mut img_w = 0u32;
    let mut img_h = 0u32;

    // Main header up to (but not consuming past) the first SOT.
    let sot_marker_off = loop {
        let marker_off = br.position();
        let marker = br.read_u16().map_err(io_err)?;
        match marker {
            MARKER_SIZ => {
                let (s, w, h) = read_siz(&mut br)?;
                img_w = w;
                img_h = h;
                siz = Some(s);
            }
            MARKER_COD => cod = Some(read_cod(&mut br)?),
            MARKER_QCD => qcd = Some(read_qcd(&mut br)?),
            MARKER_SOT => break marker_off,
            // Skip-with-rejection: identify by marker code.
            MARKER_COC => return Err(reject("COC")),
            MARKER_QCC => return Err(reject("QCC")),
            MARKER_RGN => return Err(reject("RGN")),
            MARKER_POC => return Err(reject("POC")),
            MARKER_PPM => return Err(reject("PPM")),
            MARKER_PLM => return Err(reject("PLM")),
            // Informational: skip silently.
            MARKER_CRG | MARKER_COM | MARKER_TLM => skip_segment(&mut br)?,
            other => {
                return Err(CodecError::InvalidData(format!(
                    "unexpected marker 0x{other:04X} in main header"
                )))
            }
        }
    };

    let siz = siz.ok_or_else(|| CodecError::InvalidData("missing SIZ marker".into()))?;
    let cod = cod.ok_or_else(|| CodecError::InvalidData("missing COD marker".into()))?;
    let qcd = qcd.ok_or_else(|| CodecError::InvalidData("missing QCD marker".into()))?;
    let _ = &siz;

    // SOT segment (marker already consumed).
    let lsot = br.read_u16().map_err(io_err)?;
    if lsot != 10 {
        return Err(CodecError::InvalidData(format!("unexpected Lsot {lsot}")));
    }
    let isot = br.read_u16().map_err(io_err)?;
    if isot != 0 {
        return Err(CodecError::Unsupported(format!(
            "only tile index 0 supported (Isot = {isot})"
        )));
    }
    let psot = br.read_u32().map_err(io_err)?;
    let tpsot = br.read_u8().map_err(io_err)?;
    let tnsot = br.read_u8().map_err(io_err)?;
    if tpsot != 0 {
        return Err(CodecError::Unsupported(format!(
            "only the first tile-part supported (TPsot = {tpsot})"
        )));
    }
    if tnsot > 1 {
        return Err(CodecError::Unsupported(format!(
            "multiple tile-parts not supported (TNsot = {tnsot})"
        )));
    }

    // Require SOD, walking any informational markers between SOT and SOD.
    // Accept the legacy (0xFFD3) SOD code as well as the conformant 0xFF93.
    let payload_start = loop {
        let marker = br.read_u16().map_err(io_err)?;
        match marker {
            MARKER_SOD | MARKER_SOD_LEGACY => break br.position(),
            MARKER_CRG | MARKER_COM => skip_segment(&mut br)?,
            MARKER_PPT => return Err(reject("PPT")),
            MARKER_PLT => return Err(reject("PLT")),
            other => {
                return Err(CodecError::InvalidData(format!(
                    "expected SOD after SOT, got 0x{other:04X}"
                )))
            }
        }
    };

    // Tile-part data bounds (T.800 A.4.2). Psot spans from the SOT marker's
    // first byte to the end of the tile-part data.
    let payload_end = if psot == 0 {
        // Psot == 0 ⇒ data extends to EOC. Require the stream to end with EOC.
        data.len()
            .checked_sub(2)
            .filter(|&e| {
                e >= payload_start
                    && data[e] == (MARKER_EOC >> 8) as u8
                    && data[e + 1] == MARKER_EOC as u8
            })
            .ok_or_else(|| CodecError::InvalidData("Psot = 0 but no trailing EOC".into()))?
    } else {
        let tile_part_end = (sot_marker_off as u64)
            .checked_add(psot as u64)
            .ok_or_else(|| CodecError::InvalidData("Psot overflow".into()))?;
        if tile_part_end > data.len() as u64 {
            return Err(CodecError::InvalidData("Psot exceeds buffer".into()));
        }
        let end = tile_part_end as usize;
        if end < payload_start {
            return Err(CodecError::InvalidData(
                "Psot shorter than tile-part header".into(),
            ));
        }
        // The tile-part must be followed immediately by EOC and nothing else.
        let after = &data[end..];
        if after.len() != 2 || after[0] != (MARKER_EOC >> 8) as u8 || after[1] != MARKER_EOC as u8 {
            return Err(CodecError::InvalidData(
                "expected EOC immediately after the tile-part (trailing garbage?)".into(),
            ));
        }
        end
    };

    Ok(Parsed {
        cod,
        qcd,
        img_w,
        img_h,
        payload: payload_start..payload_end,
    })
}

// ---------------------------------------------------------------------------
// Public API: decode
// ---------------------------------------------------------------------------

/// Decode a JPEG 2000 codestream into a 16-bit grayscale pixel buffer, using
/// [`LegacyPolicy::Auto`]. See [`decode_with_options`].
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    decode_with_options(data, width, height, DecodeOptions::default())
}

/// Decode a JPEG 2000 codestream into a 16-bit grayscale pixel buffer.
///
/// The SIZ marker is authoritative for the image dimensions. If `width` or
/// `height` is non-zero it is validated against SIZ and a mismatch yields
/// [`CodecError::DimensionMismatch`]; passing `0` for either means "trust SIZ".
///
/// Returns `(pixels, width, height)` where `pixels` is a row-major `Vec<u16>`.
///
/// ## Legacy detection ([`LegacyPolicy::Auto`])
///
/// v1.0.0 / Go "legacy" streams carry the same SOC/SIZ/COD/QCD/SOT markers but
/// their tile payload is `[Xsiz_u16be, Ysiz_u16be, then Xsiz·Ysiz i32be
/// coefficients]`. Their QCD, however, was written with **all-zero SPqcd
/// exponents** — impossible for any conformant 16-bit stream, whose exponents
/// are always `≥ RI = 16`. That anomaly is used as the primary discriminator
/// (checked *before* attempting a packet decode, which is cleaner than
/// failure-then-retry): a stream whose exponents are all zero *and* whose
/// payload matches the legacy layout is decoded via the frozen legacy path.
pub fn decode_with_options(
    data: &[u8],
    width: u32,
    height: u32,
    opts: DecodeOptions,
) -> Result<(Vec<u16>, u32, u32), CodecError> {
    let parsed = parse_codestream(data)?;
    let img_w = parsed.img_w;
    let img_h = parsed.img_h;

    // Validate caller dimensions against the authoritative SIZ (0 = wildcard).
    if (width != 0 && width != img_w) || (height != 0 && height != img_h) {
        return Err(CodecError::DimensionMismatch {
            expected: (width as usize) * (height as usize),
            actual: (img_w as usize) * (img_h as usize),
        });
    }

    let payload = &data[parsed.payload.clone()];
    let all_zero_exps = parsed.qcd.step_sizes.iter().all(|&e| e == 0);
    let is_legacy_fp = all_zero_exps && legacy_payload_matches(payload, img_w, img_h);

    let pixels = match opts.legacy {
        LegacyPolicy::StandardOnly => decode_standard(&parsed, payload)?,
        LegacyPolicy::LegacyOnly => {
            if !LEGACY_AVAILABLE {
                return Err(CodecError::Unsupported(
                    "legacy-decode feature disabled".into(),
                ));
            }
            if !is_legacy_fp {
                return Err(CodecError::InvalidData(
                    "LegacyOnly: input is not a legacy raw-DWT stream".into(),
                ));
            }
            decode_legacy(payload, img_w, img_h, parsed.cod.decomp_levels)?
        }
        LegacyPolicy::Auto => {
            if LEGACY_AVAILABLE && is_legacy_fp {
                decode_legacy(payload, img_w, img_h, parsed.cod.decomp_levels)?
            } else {
                decode_standard(&parsed, payload)?
            }
        }
    };

    Ok((pixels, img_w, img_h))
}

/// Conformant T.800 tile decode (with DC un-shift). Rejects the legacy all-zero
/// QCD anomaly, which cannot arise from any conformant 16-bit stream.
fn decode_standard(parsed: &Parsed, payload: &[u8]) -> Result<Vec<u16>, CodecError> {
    let nl = parsed.cod.decomp_levels;
    let exps = &parsed.qcd.step_sizes;
    if exps.iter().all(|&e| e == 0) {
        return Err(CodecError::InvalidData(
            "all-zero QCD exponents: not a conformant 16-bit stream \
             (a legacy v1.0.0 file needs LegacyPolicy::Auto/LegacyOnly)"
                .into(),
        ));
    }
    // Enough exponents for every band at NL levels (LL + 3·NL detail bands).
    if exps.len() < 3 * nl as usize + 1 {
        return Err(CodecError::InvalidData(format!(
            "QCD has {} exponents, need {} for {nl} levels",
            exps.len(),
            3 * nl as usize + 1
        )));
    }

    let guard = parsed.qcd.guard_bits;

    // Robustness guard (adversarial QCD): the tier-1 decoder shifts `1 << p`
    // for p in `0..num_bitplanes`, and `num_bitplanes <= Mb = εb + G − 1`. A
    // crafted QCD can push εb to 31 and G to 7, giving Mb ≈ 37 and shifts of
    // `1 << 36` — a debug panic / release corruption, and magnitudes past the
    // i32 range. Reject any band whose Mb would permit `num_bitplanes >= 32`
    // before it can reach the shift. Our conformant encoder uses εb=16+gain,
    // G=2 → Mb ≈ 17-18, comfortably under the cap.
    for &eps in exps.iter().take(3 * nl as usize + 1) {
        let mb = mb_from_epsilon(eps as u8, guard);
        if mb >= 32 {
            return Err(CodecError::Unsupported(format!(
                "QCD band Mb {mb} (εb={eps}, G={guard}) allows num_bitplanes >= 32; \
                 magnitude would exceed the i32 coefficient range"
            )));
        }
    }

    let mb_for = |kind: BandKind, level: u8| -> u32 {
        let idx = qcd_band_index(kind, level, nl);
        mb_from_epsilon(exps[idx] as u8, guard)
    };

    let xcb = parsed.cod.cb_width_exp + 2; // actual exponent
    let ycb = parsed.cod.cb_height_exp + 2;
    let coeffs = decode_tile(payload, parsed.img_w, parsed.img_h, nl, xcb, ycb, mb_for)?;

    // DC un-shift and clamp to the unsigned 16-bit range.
    Ok(coeffs
        .iter()
        .map(|&v| (v + DC_SHIFT).clamp(0, u16::MAX as i32) as u16)
        .collect())
}

/// Does `payload` match the legacy tile layout `[w_u16be, h_u16be, w·h i32be]`?
fn legacy_payload_matches(payload: &[u8], w: u32, h: u32) -> bool {
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return false;
    }
    let wh = match (w as usize).checked_mul(h as usize) {
        Some(v) => v,
        None => return false,
    };
    let expected = match wh.checked_mul(4).and_then(|v| v.checked_add(4)) {
        Some(v) => v,
        None => return false,
    };
    payload.len() == expected
        && payload[0] == (w >> 8) as u8
        && payload[1] == w as u8
        && payload[2] == (h >> 8) as u8
        && payload[3] == h as u8
}

#[cfg(feature = "legacy-decode")]
fn decode_legacy(payload: &[u8], w: u32, h: u32, nl: u8) -> Result<Vec<u16>, CodecError> {
    let td = crate::legacy::TileDecoder::new(nl as usize);
    let (dw, dh, coeffs) = td.decode_tile(payload)?;
    if dw != w as usize || dh != h as usize {
        return Err(CodecError::DimensionMismatch {
            expected: (w as usize) * (h as usize),
            actual: dw * dh,
        });
    }
    // Legacy files stored the raw (un-shifted) pixel DWT, so no DC un-shift.
    Ok(coeffs
        .iter()
        .map(|&v| v.clamp(0, u16::MAX as i32) as u16)
        .collect())
}

#[cfg(not(feature = "legacy-decode"))]
fn decode_legacy(_payload: &[u8], _w: u32, _h: u32, _nl: u8) -> Result<Vec<u16>, CodecError> {
    Err(CodecError::Unsupported(
        "legacy-decode feature disabled".into(),
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_pixels(width: u32, height: u32, pattern: impl Fn(u32, u32) -> u16) -> Vec<u16> {
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(pattern(x, y));
            }
        }
        data
    }

    fn enc(pixels: &[u16], w: u32, h: u32, opts: &Jpeg2kOptions) -> Vec<u8> {
        let mut buf = Vec::new();
        encode(pixels, w, h, opts, &mut buf).expect("encode");
        buf
    }

    // -- round-trip matrix --------------------------------------------------

    fn pattern_pixels(kind: usize, w: u32, h: u32) -> Vec<u16> {
        let n = (w * h) as usize;
        match kind {
            0 => vec![40000u16; n], // constant (exercises high half of the range)
            1 => (0..n as u32)
                .map(|i| (((i % w) * 250 + (i / w) * 70) % 65536) as u16)
                .collect(),
            2 => {
                let mut s = 0x9E37_79B9_7F4A_7C15u64 ^ ((w as u64) << 20) ^ (h as u64);
                (0..n)
                    .map(|_| {
                        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                        (s >> 40) as u16
                    })
                    .collect()
            }
            _ => {
                let mut v = vec![0u16; n];
                if n > 0 {
                    v[0] = 65535;
                    v[n / 2] = 32768;
                    v[n - 1] = 1;
                }
                v
            }
        }
    }

    #[test]
    fn roundtrip_matrix_auto_and_standard_only() {
        let dims = [1u32, 2, 5, 13, 64, 130, 200];
        let levels = [0u8, 1, 5];
        let cbs = [(2u8, 2u8), (6, 6)];
        for &w in &dims {
            for &h in &dims {
                for &nl in &levels {
                    for &(cbw, cbh) in &cbs {
                        for kind in 0..4 {
                            let pixels = pattern_pixels(kind, w, h);
                            let opts = Jpeg2kOptions {
                                num_decomp_levels: nl,
                                cb_width_exp: cbw,
                                cb_height_exp: cbh,
                                ..Jpeg2kOptions::default()
                            };
                            let buf = enc(&pixels, w, h, &opts);
                            // Auto.
                            let (d, dw, dh) = decode(&buf, w, h).unwrap_or_else(|e| {
                                panic!("Auto decode {w}x{h} nl={nl} cb=({cbw},{cbh}) k{kind}: {e}")
                            });
                            assert_eq!((dw, dh), (w, h));
                            assert_eq!(d, pixels, "Auto {w}x{h} nl={nl} cb=({cbw},{cbh}) k{kind}");
                            // StandardOnly must also decode a conformant stream.
                            let (d2, _, _) = decode_with_options(
                                &buf,
                                w,
                                h,
                                DecodeOptions {
                                    legacy: LegacyPolicy::StandardOnly,
                                },
                            )
                            .unwrap();
                            assert_eq!(d2, pixels, "StandardOnly {w}x{h} nl={nl}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn decode_trusts_siz_when_dims_zero() {
        let pixels = make_test_pixels(20, 12, |x, y| (x * 300 + y) as u16);
        let buf = enc(&pixels, 20, 12, &Jpeg2kOptions::default());
        let (d, w, h) = decode(&buf, 0, 0).unwrap();
        assert_eq!((w, h), (20, 12));
        assert_eq!(d, pixels);
    }

    #[test]
    fn codestream_starts_with_soc_and_ends_with_eoc() {
        let pixels = make_test_pixels(8, 8, |_, _| 42);
        let buf = enc(&pixels, 8, 8, &Jpeg2kOptions::default());
        assert_eq!(&buf[..2], &[0xFF, 0x4F]);
        let n = buf.len();
        assert_eq!(&buf[n - 2..], &[0xFF, 0xD9]);
    }

    #[test]
    fn conformant_sod_marker_is_ff93() {
        let pixels = make_test_pixels(8, 8, |_, _| 7);
        let buf = enc(&pixels, 8, 8, &Jpeg2kOptions::default());
        // SOD (0xFF93) must appear (conformant code, not the legacy 0xFFD3).
        assert!(buf.windows(2).any(|w| w == [0xFF, 0x93]));
        assert!(!buf.windows(2).any(|w| w == [0xFF, 0xD3]));
    }

    #[test]
    fn qcd_exponents_are_conformant_nonzero() {
        // εb = 16/17/17/18 for LL/HL/LH/HH → SPqcd bytes 0x80/0x88/0x88/0x90.
        let pixels = make_test_pixels(4, 4, |_, _| 100);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 1,
            ..Jpeg2kOptions::default()
        };
        let buf = enc(&pixels, 4, 4, &opts);
        // Find the QCD marker and inspect its SPqcd bytes.
        let qcd_off = buf.windows(2).position(|w| w == [0xFF, 0x5C]).unwrap();
        // FF5C, Lqcd(2)=0x0007, Sqcd(1)=0x40, then 4 exponents.
        assert_eq!(&buf[qcd_off + 2..qcd_off + 4], &[0x00, 0x07]);
        assert_eq!(buf[qcd_off + 4], 0x40); // guard bits 2, style 0
        assert_eq!(
            &buf[qcd_off + 5..qcd_off + 9],
            &[16 << 3, 17 << 3, 17 << 3, 18 << 3]
        );
    }

    // -- dimension / basic errors ------------------------------------------

    #[test]
    fn decode_dimension_mismatch() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        let buf = enc(&pixels, 8, 8, &Jpeg2kOptions::default());
        assert!(matches!(
            decode(&buf, 16, 16),
            Err(CodecError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn decode_invalid_data() {
        assert!(decode(&[], 8, 8).is_err());
        assert!(decode(&[0x00, 0x00], 8, 8).is_err());
        assert!(decode(&[0xFF, 0x4F], 8, 8).is_err());
    }

    #[test]
    fn encode_rejects_multi_tile_options() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        let opts = Jpeg2kOptions {
            tile_width: 4,
            tile_height: 4,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        assert!(matches!(
            encode(&pixels, 8, 8, &opts, &mut buf),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn encode_rejects_bad_cb_exp() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        for opts in [
            Jpeg2kOptions {
                cb_width_exp: 11,
                ..Jpeg2kOptions::default()
            },
            Jpeg2kOptions {
                cb_width_exp: 8,
                cb_height_exp: 8,
                ..Jpeg2kOptions::default()
            },
        ] {
            let mut buf = Vec::new();
            assert!(encode(&pixels, 8, 8, &opts, &mut buf).is_err());
        }
    }

    // -- support matrix rejection ------------------------------------------

    /// Byte offset of the first occurrence of a 2-byte marker.
    fn marker_off(buf: &[u8], marker: u16) -> usize {
        let hi = (marker >> 8) as u8;
        let lo = marker as u8;
        buf.windows(2)
            .position(|w| w[0] == hi && w[1] == lo)
            .unwrap()
    }

    fn valid_stream() -> Vec<u8> {
        let pixels = make_test_pixels(16, 16, |x, y| (x * 400 + y * 40) as u16);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 2,
            ..Jpeg2kOptions::default()
        };
        enc(&pixels, 16, 16, &opts)
    }

    fn assert_unsupported(buf: &[u8], ctx: &str) {
        match decode(buf, 16, 16) {
            Err(CodecError::Unsupported(_)) => {}
            other => panic!("{ctx}: expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn reject_8bit_ssiz() {
        let mut buf = valid_stream();
        let siz = marker_off(&buf, MARKER_SIZ);
        // Ssiz is the byte after the 36-byte fixed SIZ body: FF51 + Lsiz(2) +
        // Rsiz(2) + 8×4 dims + Csiz(2) = 2+2+2+32+2 = 40 → Ssiz at siz+40.
        buf[siz + 40] = 7; // precision 8, unsigned
        assert_unsupported(&buf, "8-bit Ssiz");
    }

    #[test]
    fn reject_signed_ssiz() {
        let mut buf = valid_stream();
        let siz = marker_off(&buf, MARKER_SIZ);
        buf[siz + 40] = 0x8F; // signed 16-bit
        assert_unsupported(&buf, "signed Ssiz");
    }

    #[test]
    fn reject_multi_component() {
        let mut buf = valid_stream();
        let siz = marker_off(&buf, MARKER_SIZ);
        // Csiz is the 2-byte field right before Ssiz (siz+38, siz+39).
        buf[siz + 38] = 0x00;
        buf[siz + 39] = 0x03; // Csiz = 3
        assert_unsupported(&buf, "Csiz=3");
    }

    #[test]
    fn reject_multi_tile() {
        let mut buf = valid_stream();
        let siz = marker_off(&buf, MARKER_SIZ);
        // XTsiz is at siz + 2(marker) + 2(Lsiz) + 2(Rsiz) + 16 = siz+22.
        buf[siz + 22] = 0x00;
        buf[siz + 23] = 0x00;
        buf[siz + 24] = 0x00;
        buf[siz + 25] = 0x08; // XTsiz = 8 < Xsiz(16)
        assert_unsupported(&buf, "multi-tile");
    }

    #[test]
    fn reject_non_lrcp_progression() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        // SPcod: scod(1) then progression(1). COD = FF52 + Lcod(2) + scod + prog…
        buf[cod + 5] = 1; // RLCP
        assert_unsupported(&buf, "RLCP progression");
    }

    #[test]
    fn reject_multiple_layers() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        // num_layers is the u16 after scod+prog: cod + 4 + 1 + 1 = cod+6.
        buf[cod + 6] = 0x00;
        buf[cod + 7] = 0x02; // layers = 2
        assert_unsupported(&buf, "layers=2");
    }

    #[test]
    fn reject_mct() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        // mct byte: cod + 8 (after scod, prog, layers u16).
        buf[cod + 8] = 1;
        assert_unsupported(&buf, "MCT=1");
    }

    #[test]
    fn reject_cb_style() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        // cb_style byte: scod,prog,layers(2),mct,levels,cbw,cbh,cb_style →
        // cod+4 + 1+1+2+1+1+1+1 = cod+12.
        buf[cod + 12] = 0x01;
        assert_unsupported(&buf, "cb_style!=0");
    }

    #[test]
    fn reject_transform_97() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        // transform byte: cod+13.
        buf[cod + 13] = 0; // 9/7 irreversible
        assert_unsupported(&buf, "9/7 transform");
    }

    #[test]
    fn reject_precinct_flag() {
        let mut buf = valid_stream();
        let cod = marker_off(&buf, MARKER_COD);
        buf[cod + 4] = 0x01; // scod precinct flag
        assert_unsupported(&buf, "precinct flag");
    }

    #[test]
    fn reject_coc_marker() {
        // Inject a COC segment before SOT.
        let buf = valid_stream();
        let sot = marker_off(&buf, MARKER_SOT);
        let mut out = buf[..sot].to_vec();
        out.extend_from_slice(&[0xFF, 0x53, 0x00, 0x03, 0x00]); // COC, Lcoc=3, 1 byte
        out.extend_from_slice(&buf[sot..]);
        assert_unsupported(&out, "COC marker");
    }

    #[test]
    fn reject_qcd_mb_overflow_shift() {
        // Adversarial QCD: εb = 31 (SPqcd byte 0xF8) and guard bits = 7 (Sqcd
        // byte 0xE0) drive a band's Mb to ~37. Unguarded, the tier-1 decoder
        // would shift `1 << p` for p up to 36 — a debug panic / release
        // corruption, and magnitudes past the i32 range. Must be a clean Err
        // (this test runs in debug, where the bad shift would panic).
        let mut buf = valid_stream();
        let qcd = marker_off(&buf, MARKER_QCD);
        buf[qcd + 4] = 0xE0; // Sqcd: guard bits 7, no-quantization style 0
        buf[qcd + 5] = 0xF8; // first SPqcd (LL): εb = 31
        match decode(&buf, 16, 16) {
            Err(CodecError::Unsupported(_)) | Err(CodecError::InvalidData(_)) => {}
            other => panic!("expected clean Err for over-range QCD Mb, got {other:?}"),
        }
    }

    #[test]
    fn encode_rejects_multi_precinct() {
        // 32769 > 2^15 (the default PPx=15 precinct span) → resolution NL spans
        // two precincts horizontally. The single-packet-per-resolution pipeline
        // must reject rather than silently mispack.
        let w = 32769u32;
        let h = 1u32;
        let pixels = vec![0u16; (w * h) as usize];
        let mut buf = Vec::new();
        assert!(matches!(
            encode(&pixels, w, h, &Jpeg2kOptions::default(), &mut buf),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn decode_rejects_multi_precinct() {
        // Enlarge Xsiz and XTsiz to 32769 so the parsed geometry crosses the
        // default precinct span; decode must reject with Unsupported.
        let mut buf = valid_stream();
        let siz = marker_off(&buf, MARKER_SIZ);
        let big = 32769u32.to_be_bytes();
        buf[siz + 6..siz + 10].copy_from_slice(&big); // Xsiz
        buf[siz + 22..siz + 26].copy_from_slice(&big); // XTsiz
                                                       // Trust SIZ (0,0) so we reach the geometry check, not a dim mismatch.
        assert!(matches!(
            decode(&buf, 0, 0),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn reject_tnsot_multiple_tile_parts() {
        let mut buf = valid_stream();
        let sot = marker_off(&buf, MARKER_SOT);
        // TNsot is the last byte of the 12-byte SOT segment: sot+11.
        buf[sot + 11] = 2;
        assert_unsupported(&buf, "TNsot=2");
    }

    #[test]
    fn reject_trailing_garbage() {
        let mut buf = valid_stream();
        buf.push(0x00);
        assert!(matches!(
            decode(&buf, 16, 16),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn truncation_never_panics() {
        let buf = valid_stream();
        for cut in 0..buf.len() {
            // Must be Ok or Err, never panic.
            let _ = decode(&buf[..cut], 16, 16);
        }
        assert!(decode(&buf, 16, 16).is_ok());
    }
}
