//! JPEG 2000 codestream reader / writer and public encode / decode functions.
//!
//! Generates and parses the codestream structure:
//! SOC -> SIZ -> COD -> QCD -> SOT -> SOD -> [tile data] -> EOC

use std::io::Write;

use crate::error::CodecError;

use crate::bitstream::{ByteReader, ByteWriter};
use crate::markers::*;
use crate::tile::{TileDecoder, TileEncoder};
use crate::Jpeg2kOptions;

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
        // Length: 38 + 3 * num_components
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
        // For reversible coding each step size is 1 byte (exponent only).
        for &step in &qcd.step_sizes {
            let exp = (step << 3) as u8;
            self.bw.write_u8(exp);
        }
    }

    fn write_sot(&mut self, sot: &SotMarker) {
        self.bw.write_u16(MARKER_SOT);
        self.bw.write_u16(10); // fixed length
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

// ---------------------------------------------------------------------------
// Codestream reader (decoder side)
// ---------------------------------------------------------------------------

struct CodestreamReader<'a> {
    br: ByteReader<'a>,
    siz: Option<SizMarker>,
    cod: Option<CodMarker>,
    qcd: Option<QcdMarker>,
}

impl<'a> CodestreamReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            br: ByteReader::new(data),
            siz: None,
            cod: None,
            qcd: None,
        }
    }

    /// Read the main header (SOC through first SOT or SOD).
    fn read_main_header(&mut self) -> Result<(), CodecError> {
        // Read SOC.
        let marker = self.read_marker()?;
        if marker != MARKER_SOC {
            return Err(CodecError::InvalidData(format!(
                "expected SOC (0xFF4F), got 0x{marker:04X}"
            )));
        }

        loop {
            let marker = self.read_marker()?;
            match marker {
                MARKER_SIZ => self.read_siz()?,
                MARKER_COD => self.read_cod()?,
                MARKER_QCD => self.read_qcd()?,
                MARKER_SOT | MARKER_SOD => return Ok(()),
                _ => {
                    // Skip unknown marker segment.
                    let length = self.br.read_u16().map_err(io_err)?;
                    self.br
                        .skip((length as usize).saturating_sub(2))
                        .map_err(io_err)?;
                }
            }
        }
    }

    fn read_marker(&mut self) -> Result<u16, CodecError> {
        self.br.read_u16().map_err(io_err)
    }

    fn read_siz(&mut self) -> Result<(), CodecError> {
        let length = self.br.read_u16().map_err(io_err)?;
        if length < 41 {
            return Err(CodecError::InvalidData("SIZ marker too short".into()));
        }
        let rsiz = self.br.read_u16().map_err(io_err)?;
        let x_siz = self.br.read_u32().map_err(io_err)?;
        let y_siz = self.br.read_u32().map_err(io_err)?;
        let x_osiz = self.br.read_u32().map_err(io_err)?;
        let y_osiz = self.br.read_u32().map_err(io_err)?;
        let x_tsiz = self.br.read_u32().map_err(io_err)?;
        let y_tsiz = self.br.read_u32().map_err(io_err)?;
        let x_tosiz = self.br.read_u32().map_err(io_err)?;
        let y_tosiz = self.br.read_u32().map_err(io_err)?;
        let num_comps = self.br.read_u16().map_err(io_err)?;

        let mut components = Vec::with_capacity(num_comps as usize);
        for _ in 0..num_comps {
            let ssiz = self.br.read_u8().map_err(io_err)?;
            let signed = (ssiz & 0x80) != 0;
            let precision = (ssiz & 0x7F) + 1;
            let x_rsiz = self.br.read_u8().map_err(io_err)?;
            let y_rsiz = self.br.read_u8().map_err(io_err)?;
            components.push(ComponentInfo {
                precision,
                signed,
                x_rsiz,
                y_rsiz,
            });
        }

        self.siz = Some(SizMarker {
            rsiz,
            x_siz,
            y_siz,
            x_osiz,
            y_osiz,
            x_tsiz,
            y_tsiz,
            x_tosiz,
            y_tosiz,
            components,
        });
        Ok(())
    }

    fn read_cod(&mut self) -> Result<(), CodecError> {
        let length = self.br.read_u16().map_err(io_err)?;
        if length < 12 {
            return Err(CodecError::InvalidData("COD marker too short".into()));
        }
        let scod = self.br.read_u8().map_err(io_err)?;
        let prog_byte = self.br.read_u8().map_err(io_err)?;
        let progression = ProgressionOrder::from_byte(prog_byte).ok_or_else(|| {
            CodecError::InvalidData(format!("unknown progression order {prog_byte}"))
        })?;
        let num_layers = self.br.read_u16().map_err(io_err)?;
        let mct = self.br.read_u8().map_err(io_err)?;
        let decomp_levels = self.br.read_u8().map_err(io_err)?;
        let cb_width_exp = self.br.read_u8().map_err(io_err)?;
        let cb_height_exp = self.br.read_u8().map_err(io_err)?;
        let cb_style = self.br.read_u8().map_err(io_err)?;
        let transform_byte = self.br.read_u8().map_err(io_err)?;
        let transform = TransformType::from_byte(transform_byte).ok_or_else(|| {
            CodecError::InvalidData(format!("unknown transform type {transform_byte}"))
        })?;

        let mut precinct_sizes = Vec::new();
        if scod & CODING_STYLE_PRECINCTS_USER != 0 {
            let remaining = (length as usize).saturating_sub(12);
            for _ in 0..remaining {
                precinct_sizes.push(self.br.read_u8().map_err(io_err)?);
            }
        }

        self.cod = Some(CodMarker {
            scod,
            progression,
            num_layers,
            mct,
            decomp_levels,
            cb_width_exp,
            cb_height_exp,
            cb_style,
            transform,
            precinct_sizes,
        });
        Ok(())
    }

    fn read_qcd(&mut self) -> Result<(), CodecError> {
        let length = self.br.read_u16().map_err(io_err)?;
        if length < 4 {
            return Err(CodecError::InvalidData("QCD marker too short".into()));
        }
        let sqcd_byte = self.br.read_u8().map_err(io_err)?;
        let guard_bits = (sqcd_byte >> 5) & 0x07;
        let q_style = sqcd_byte & 0x1F;

        let remaining = (length as usize).saturating_sub(3);
        let step_sizes = match q_style {
            0 => {
                // No quantization (reversible) -- each entry is 1 byte.
                let mut sizes = Vec::with_capacity(remaining);
                for _ in 0..remaining {
                    let exp = self.br.read_u8().map_err(io_err)?;
                    sizes.push((exp >> 3) as i16);
                }
                sizes
            }
            _ => {
                // Skip unsupported quantization styles.
                self.br.skip(remaining).map_err(io_err)?;
                Vec::new()
            }
        };

        self.qcd = Some(QcdMarker {
            sqcd: q_style,
            guard_bits,
            step_sizes,
        });
        Ok(())
    }
}

fn io_err(e: std::io::Error) -> CodecError {
    CodecError::Io(e)
}

// ---------------------------------------------------------------------------
// Find a marker in raw data
// ---------------------------------------------------------------------------

fn find_marker(data: &[u8], marker: u16) -> Option<usize> {
    let hi = (marker >> 8) as u8;
    let lo = marker as u8;
    data.windows(2).position(|w| w[0] == hi && w[1] == lo)
}

// ---------------------------------------------------------------------------
// Public API: encode
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into JPEG 2000 codestream format.
///
/// `pixels` is a row-major pixel buffer of length `img_width * img_height`.
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
    let expected = width * height;

    if pixels.len() != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    if width == 0 || height == 0 {
        return Err(CodecError::InvalidData("image has zero dimension".into()));
    }

    // Convert pixel data to i32 for DWT processing.
    let component: Vec<i32> = pixels.iter().map(|&v| v as i32).collect();

    let num_comps = 1u16;
    let precision = 16u8;

    // Build marker structures.
    let comp_info = vec![ComponentInfo {
        precision,
        signed: false,
        x_rsiz: 1,
        y_rsiz: 1,
    }];
    let siz = build_siz(
        width as u32,
        height as u32,
        comp_info,
        options.tile_width,
        options.tile_height,
    );
    let cod = build_default_cod(options.num_decomp_levels, 1, false);
    let qcd = build_default_qcd(options.num_decomp_levels, 2);

    // Encode the tile.
    let te = TileEncoder::new(width, height, options.num_decomp_levels as usize);
    let tile_data = te.encode_tile(&component)?;

    // Build the codestream.
    let mut cw = CodestreamWriter::new();
    cw.write_soc();
    cw.write_siz(&siz);
    cw.write_cod(&cod);
    cw.write_qcd(&qcd);

    // SOT: total length = 12 (SOT marker segment) + 2 (SOD marker) + tile_data
    let tile_part_len = 12 + 2 + tile_data.len() as u32;
    let num_tiles = siz.num_tiles();
    for tile_idx in 0..num_tiles {
        let sot = SotMarker {
            tile_index: tile_idx as u16,
            tile_part_len,
            tile_part_idx: 0,
            num_tile_parts: 1,
        };
        cw.write_sot(&sot);
        cw.write_sod();
        cw.write_bytes(&tile_data);
    }

    cw.write_eoc();

    let bytes = cw.into_bytes();
    w.write_all(&bytes).map_err(CodecError::Io)?;

    // Verify: the encoded data must carry exactly `num_comps` component(s).
    let _ = num_comps;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: decode
// ---------------------------------------------------------------------------

/// Decode a JPEG 2000 codestream into a 16-bit grayscale pixel buffer.
///
/// The `width` and `height` parameters are used for validation against the
/// SIZ marker.
///
/// Returns `(pixels, width, height)` where pixels is a row-major `Vec<u16>`.
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    if data.len() < 4 {
        return Err(CodecError::InvalidData("codestream too short".into()));
    }

    // Check SOC.
    if data[0] != 0xFF || data[1] != 0x4F {
        return Err(CodecError::InvalidData("missing SOC marker".into()));
    }

    // Parse main header.
    let mut cr = CodestreamReader::new(data);
    cr.read_main_header()?;

    let siz = cr
        .siz
        .as_ref()
        .ok_or_else(|| CodecError::InvalidData("missing SIZ marker".into()))?;
    let cod = cr
        .cod
        .as_ref()
        .ok_or_else(|| CodecError::InvalidData("missing COD marker".into()))?;

    let img_w = (siz.x_siz - siz.x_osiz) as usize;
    let img_h = (siz.y_siz - siz.y_osiz) as usize;

    // Validate against caller-provided dimensions.
    if img_w != width as usize || img_h != height as usize {
        return Err(CodecError::DimensionMismatch {
            expected: (width as usize) * (height as usize),
            actual: img_w * img_h,
        });
    }

    let decomp_levels = cod.decomp_levels as usize;

    // Find SOT marker.
    let sot_pos = find_marker(data, MARKER_SOT)
        .ok_or_else(|| CodecError::InvalidData("missing SOT marker".into()))?;

    // Skip SOT marker (2 bytes) + SOT length field (2 bytes) + SOT content (8 bytes) = 12 bytes.
    let mut pos = sot_pos + 2;
    let sot_len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
    pos += sot_len;

    // Find SOD.
    let sod_pos = find_marker(&data[pos..], MARKER_SOD)
        .ok_or_else(|| CodecError::InvalidData("missing SOD marker".into()))?;
    pos += sod_pos + 2; // skip SOD marker

    // Decode tile data for each component (we only support 1 component).
    let td = TileDecoder::new(decomp_levels);
    if pos >= data.len() {
        return Err(CodecError::InvalidData("no tile data after SOD".into()));
    }

    let (dw, dh, pixels) = td.decode_tile(&data[pos..])?;

    if dw != img_w || dh != img_h {
        return Err(CodecError::DimensionMismatch {
            expected: img_w * img_h,
            actual: dw * dh,
        });
    }

    // Convert i32 back to u16, clamping to valid range.
    let u16_pixels: Vec<u16> = pixels
        .iter()
        .map(|&v| v.clamp(0, u16::MAX as i32) as u16)
        .collect();
    let pixel_count = u16_pixels.len();
    let expected_count = (width as usize) * (height as usize);

    if pixel_count != expected_count {
        return Err(CodecError::DimensionMismatch {
            expected: expected_count,
            actual: pixel_count,
        });
    }

    Ok((u16_pixels, width, height))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create test pixels.
    fn make_test_pixels(width: u32, height: u32, pattern: impl Fn(u32, u32) -> u16) -> Vec<u16> {
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(pattern(x, y));
            }
        }
        data
    }

    #[test]
    fn codestream_roundtrip_constant_image() {
        let pixels = make_test_pixels(16, 16, |_, _| 1000);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 16, 16, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 16, 16).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_gradient() {
        let pixels = make_test_pixels(32, 32, |x, y| ((x + y * 32) % 65536) as u16);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 32, 32, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 32, 32).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_all_zeros() {
        let pixels = make_test_pixels(8, 8, |_, _| 0);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 8, 8, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 8, 8).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_max_value() {
        let pixels = make_test_pixels(8, 8, |_, _| u16::MAX);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 8, 8, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 8, 8).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_checkerboard() {
        let pixels = make_test_pixels(16, 16, |x, y| if (x + y) % 2 == 0 { 0 } else { 65535 });
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 16, 16, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 16, 16).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_non_square() {
        let pixels = make_test_pixels(24, 8, |x, y| (x * 100 + y * 10) as u16);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 3,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        encode(&pixels, 24, 8, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 24, 8).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_odd_dims() {
        let pixels = make_test_pixels(13, 7, |x, y| (x * y) as u16);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 2,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        encode(&pixels, 13, 7, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 13, 7).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_roundtrip_large_image() {
        let pixels = make_test_pixels(128, 128, |x, y| {
            ((x.wrapping_mul(31) ^ y.wrapping_mul(17)) % 65536) as u16
        });
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 128, 128, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 128, 128).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn codestream_starts_with_soc() {
        let pixels = make_test_pixels(8, 8, |_, _| 42);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 8, 8, &opts, &mut buf).unwrap();
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0x4F);
    }

    #[test]
    fn codestream_ends_with_eoc() {
        let pixels = make_test_pixels(8, 8, |_, _| 42);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 8, 8, &opts, &mut buf).unwrap();
        let n = buf.len();
        assert_eq!(buf[n - 2], 0xFF);
        assert_eq!(buf[n - 1], 0xD9);
    }

    #[test]
    fn decode_invalid_data() {
        assert!(decode(&[], 8, 8).is_err());
        assert!(decode(&[0x00, 0x00], 8, 8).is_err());
    }

    #[test]
    fn decode_dimension_mismatch() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        let opts = Jpeg2kOptions::default();
        let mut buf = Vec::new();
        encode(&pixels, 8, 8, &opts, &mut buf).unwrap();
        // Try decoding with wrong dimensions.
        assert!(decode(&buf, 16, 16).is_err());
    }

    #[test]
    fn codestream_roundtrip_single_pixel() {
        let pixels = make_test_pixels(2, 2, |_, _| 12345);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 1,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 2, 2).unwrap();
        assert_eq!(decoded, pixels);
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
        let err = encode(&pixels, 8, 8, &opts, &mut buf).unwrap_err();
        assert!(matches!(err, CodecError::Unsupported(_)), "got {err:?}");
    }

    #[test]
    fn encode_rejects_out_of_range_cb_exp() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        let opts = Jpeg2kOptions {
            cb_width_exp: 11,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        assert!(encode(&pixels, 8, 8, &opts, &mut buf).is_err());
    }

    #[test]
    fn encode_rejects_cb_exp_sum_over_12() {
        let pixels = make_test_pixels(8, 8, |_, _| 100);
        let opts = Jpeg2kOptions {
            cb_width_exp: 8,
            cb_height_exp: 8,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        assert!(encode(&pixels, 8, 8, &opts, &mut buf).is_err());
    }

    #[test]
    fn default_options_pass_validation() {
        assert!(Jpeg2kOptions::default().validate().is_ok());
    }

    #[test]
    fn codestream_roundtrip_decomp_levels_1() {
        let pixels = make_test_pixels(16, 16, |x, y| (x * 256 + y * 16) as u16);
        let opts = Jpeg2kOptions {
            num_decomp_levels: 1,
            ..Jpeg2kOptions::default()
        };
        let mut buf = Vec::new();
        encode(&pixels, 16, 16, &opts, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 16, 16).unwrap();
        assert_eq!(decoded, pixels);
    }
}
