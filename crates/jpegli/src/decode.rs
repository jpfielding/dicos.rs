//! Full JPEG Lossless decoding pipeline.
//!
//! Parses a complete JPEG Lossless bitstream (SOI through EOI), extracting
//! the frame header (SOF3), Huffman table (DHT), and scan parameters (SOS),
//! then decodes the entropy-coded scan data using DPCM prediction.

use std::io::{self, Cursor, Read};

use crate::error::CodecError;

use crate::huffman::HuffmanTable;
use crate::scan;

// ---------------------------------------------------------------------------
// JPEG markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOF3: u16 = 0xFFC3;
const MARKER_DHT: u16 = 0xFFC4;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_DRI: u16 = 0xFFDD;

// ---------------------------------------------------------------------------
// Decoder state
// ---------------------------------------------------------------------------

/// Internal decoder state accumulated while parsing JPEG markers.
struct Decoder {
    precision: u8,
    height: u16,
    width: u16,
    components: u8,
    comp_info: Vec<ComponentInfo>,
    dc_tables: [Option<HuffmanTable>; 4],
    predictor: u8,
    point_transform: u8,
    restart_interval: u16,
}

#[derive(Clone)]
struct ComponentInfo {
    id: u8,
    #[allow(dead_code)]
    h_sampling: u8,
    #[allow(dead_code)]
    v_sampling: u8,
    table_index: u8,
}

impl Decoder {
    fn new() -> Self {
        Self {
            precision: 0,
            height: 0,
            width: 0,
            components: 0,
            comp_info: Vec::new(),
            dc_tables: [None, None, None, None],
            predictor: 1,
            point_transform: 0,
            restart_interval: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Decode a JPEG Lossless compressed bitstream into a 16-bit grayscale pixel buffer.
///
/// The `width` and `height` parameters are used for validation against the
/// dimensions encoded in the SOF3 frame header. The actual dimensions come
/// from the JPEG data itself.
///
/// Returns `(pixels, width, height)` where pixels is a row-major `Vec<u16>`.
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    let mut cursor = Cursor::new(data);
    let mut dec = Decoder::new();

    // Read SOI
    let marker = read_marker(&mut cursor)?;
    if marker != MARKER_SOI {
        return Err(CodecError::InvalidData(format!(
            "expected SOI marker 0xFFD8, got 0x{marker:04X}"
        )));
    }

    // Parse markers until SOS
    loop {
        let marker = read_marker(&mut cursor)?;
        match marker {
            MARKER_SOF3 => parse_sof3(&mut cursor, &mut dec)?,
            MARKER_DHT => parse_dht(&mut cursor, &mut dec)?,
            MARKER_SOS => {
                parse_sos(&mut cursor, &mut dec)?;
                break;
            }
            MARKER_DRI => parse_dri(&mut cursor, &mut dec)?,
            MARKER_EOI => {
                return Err(CodecError::InvalidData(
                    "unexpected EOI before scan data".into(),
                ));
            }
            m if (0xFFE0..=0xFFEF).contains(&m) => {
                // APP markers: skip
                skip_marker_data(&mut cursor)?;
            }
            m if (0xFFC0..=0xFFCF).contains(&m) => {
                return Err(CodecError::Unsupported(format!(
                    "unsupported SOF marker: 0x{m:04X}"
                )));
            }
            0xFFFE => {
                // COM marker: skip
                skip_marker_data(&mut cursor)?;
            }
            _ => {
                // Unknown marker: skip its data
                skip_marker_data(&mut cursor)?;
            }
        }
    }

    // Validate dimensions
    let jpeg_w = dec.width as u32;
    let jpeg_h = dec.height as u32;
    if jpeg_w != width || jpeg_h != height {
        return Err(CodecError::DimensionMismatch {
            expected: (width as usize) * (height as usize),
            actual: (jpeg_w as usize) * (jpeg_h as usize),
        });
    }

    // Get Huffman table
    let table_idx = if !dec.comp_info.is_empty() {
        dec.comp_info[0].table_index as usize
    } else {
        0
    };
    let ht = dec.dc_tables[table_idx]
        .as_ref()
        .ok_or_else(|| CodecError::InvalidData("missing Huffman table".into()))?;

    // Restart interval (DRI) validation.  T.81 H.1.1: for a non-interleaved
    // single-component lossless scan 1 MCU = 1 sample, and Ri must be an
    // integer multiple of the samples-per-row (== width), so restarts fall on
    // row boundaries only.
    let restart_interval = dec.restart_interval as usize;
    if restart_interval != 0 && restart_interval % (jpeg_w as usize) != 0 {
        return Err(CodecError::InvalidData(format!(
            "restart interval {restart_interval} is not a multiple of width {jpeg_w} (H.1.1)"
        )));
    }

    // Decode the scan data (remaining bytes after SOS header)
    let remaining = &data[cursor.position() as usize..];
    let pixels = scan::decode_scan(
        remaining,
        ht,
        jpeg_w as usize,
        jpeg_h as usize,
        dec.precision,
        dec.predictor,
        dec.point_transform,
        restart_interval,
    )
    .map_err(|e| CodecError::InvalidData(format!("scan decode error: {e}")))?;

    let pixel_count = pixels.len();
    let expected = (jpeg_w as usize) * (jpeg_h as usize);
    if pixel_count != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixel_count,
        });
    }
    Ok((pixels, jpeg_w, jpeg_h))
}

// ---------------------------------------------------------------------------
// Marker parsing
// ---------------------------------------------------------------------------

/// Read a 2-byte JPEG marker, skipping any fill bytes (0xFF).
fn read_marker<R: Read>(r: &mut R) -> Result<u16, CodecError> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    if buf[0] != 0xFF {
        return Err(CodecError::InvalidData(format!(
            "expected marker prefix 0xFF, got 0x{:02X}",
            buf[0]
        )));
    }
    // Skip fill bytes
    while buf[1] == 0xFF {
        r.read_exact(&mut buf[1..2])?;
    }
    Ok(u16::from_be_bytes(buf))
}

/// Read the 2-byte marker data length and skip that many bytes.
fn skip_marker_data<R: Read>(r: &mut R) -> Result<(), CodecError> {
    let mut len_buf = [0u8; 2];
    r.read_exact(&mut len_buf)?;
    let length = u16::from_be_bytes(len_buf) as usize;
    if length < 2 {
        return Ok(());
    }
    let skip = length - 2;
    io::copy(&mut r.take(skip as u64), &mut io::sink())?;
    Ok(())
}

/// Parse SOF3 (Start of Frame - Lossless Huffman).
fn parse_sof3<R: Read>(r: &mut R, dec: &mut Decoder) -> Result<(), CodecError> {
    let mut len_buf = [0u8; 2];
    r.read_exact(&mut len_buf)?;
    let length = u16::from_be_bytes(len_buf) as usize;
    if length < 2 {
        return Err(CodecError::InvalidData("SOF3 length too short".into()));
    }
    let payload_len = length - 2;
    let mut data = vec![0u8; payload_len];
    r.read_exact(&mut data)?;

    if data.len() < 6 {
        return Err(CodecError::InvalidData(
            "SOF3 payload too short for header".into(),
        ));
    }

    dec.precision = data[0];
    dec.height = u16::from_be_bytes([data[1], data[2]]);
    dec.width = u16::from_be_bytes([data[3], data[4]]);
    dec.components = data[5];

    // T.81 B.2.2: sample precision P for lossless is 2..=16.  Rejecting the
    // out-of-range values here prevents the `1 << (precision - 1)` underflow
    // (P == 0/1) and out-of-bounds shifts (P > 16) in the DPCM loop.
    if !(2..=16).contains(&dec.precision) {
        return Err(CodecError::InvalidData(format!(
            "invalid SOF3 sample precision: {} (must be 2..=16)",
            dec.precision
        )));
    }

    // This codec supports single-component (grayscale) frames only.
    if dec.components != 1 {
        return Err(CodecError::Unsupported(format!(
            "SOF3 with {} components; only single-component frames are supported",
            dec.components
        )));
    }

    dec.comp_info.clear();
    for i in 0..dec.components as usize {
        let offset = 6 + i * 3;
        if offset + 2 >= data.len() {
            return Err(CodecError::InvalidData(
                "SOF3 payload too short for components".into(),
            ));
        }
        dec.comp_info.push(ComponentInfo {
            id: data[offset],
            h_sampling: data[offset + 1] >> 4,
            v_sampling: data[offset + 1] & 0x0F,
            table_index: data[offset + 2],
        });
    }

    Ok(())
}

/// Parse DHT (Define Huffman Table).
fn parse_dht<R: Read>(r: &mut R, dec: &mut Decoder) -> Result<(), CodecError> {
    let mut len_buf = [0u8; 2];
    r.read_exact(&mut len_buf)?;
    let length = u16::from_be_bytes(len_buf) as usize;
    if length < 2 {
        return Err(CodecError::InvalidData("DHT length too short".into()));
    }
    let payload_len = length - 2;
    let mut data = vec![0u8; payload_len];
    r.read_exact(&mut data)?;

    let mut offset = 0;
    while offset < data.len() {
        if offset >= data.len() {
            break;
        }
        let table_info = data[offset];
        let table_class = table_info >> 4; // 0 = DC
        let table_id = (table_info & 0x0F) as usize;
        offset += 1;

        if table_class != 0 {
            // Lossless only uses DC tables; skip AC table definition
            let mut count = 0usize;
            for i in 0..16 {
                if offset + i >= data.len() {
                    break;
                }
                count += data[offset + i] as usize;
            }
            offset += 16 + count;
            continue;
        }

        if table_id >= 4 {
            return Err(CodecError::InvalidData(format!(
                "invalid Huffman table ID: {table_id}"
            )));
        }

        // Read BITS[1..=16]
        let mut bits = [0u8; 17];
        let mut total_codes = 0usize;
        for i in 0..16 {
            if offset + i >= data.len() {
                return Err(CodecError::InvalidData("DHT data truncated".into()));
            }
            bits[i + 1] = data[offset + i];
            total_codes += data[offset + i] as usize;
        }
        offset += 16;

        // Read HUFFVAL
        if offset + total_codes > data.len() {
            return Err(CodecError::InvalidData("DHT values truncated".into()));
        }
        let values = data[offset..offset + total_codes].to_vec();
        offset += total_codes;

        dec.dc_tables[table_id] = Some(HuffmanTable::from_bits_values(bits, values));
    }

    Ok(())
}

/// Parse SOS (Start of Scan).
fn parse_sos<R: Read>(r: &mut R, dec: &mut Decoder) -> Result<(), CodecError> {
    let mut len_buf = [0u8; 2];
    r.read_exact(&mut len_buf)?;
    let length = u16::from_be_bytes(len_buf) as usize;
    if length < 2 {
        return Err(CodecError::InvalidData("SOS length too short".into()));
    }
    let payload_len = length - 2;
    let mut data = vec![0u8; payload_len];
    r.read_exact(&mut data)?;

    if data.is_empty() {
        return Err(CodecError::InvalidData("SOS payload empty".into()));
    }

    let num_components = data[0] as usize;
    let mut offset = 1;

    for _ in 0..num_components {
        if offset + 1 >= data.len() {
            return Err(CodecError::InvalidData(
                "SOS payload too short for component specs".into(),
            ));
        }
        let selector = data[offset];
        let table_mapping = data[offset + 1];
        offset += 2;

        // Update table index for the matching component
        for ci in &mut dec.comp_info {
            if ci.id == selector {
                ci.table_index = table_mapping >> 4;
                break;
            }
        }
    }

    // Need three more bytes: Ss, Se, Ah|Al (T.81 B.2.3).
    if offset + 2 >= data.len() {
        return Err(CodecError::InvalidData(
            "SOS payload too short for spectral selection".into(),
        ));
    }

    // Ss = predictor selection.  Table B.3 restricts lossless Ss to 1..=7:
    // 0 selects the hierarchical-only "no prediction" mode and > 7 is undefined.
    dec.predictor = data[offset];
    offset += 1;
    if !(1..=7).contains(&dec.predictor) {
        return Err(CodecError::InvalidData(format!(
            "invalid SOS predictor selector Ss={} (must be 1..=7)",
            dec.predictor
        )));
    }

    // Se must be 0 for lossless.
    let se = data[offset];
    offset += 1;
    if se != 0 {
        return Err(CodecError::InvalidData(format!(
            "invalid SOS Se={se} (must be 0 for lossless)"
        )));
    }

    // Ah (high nibble) must be 0; Al (low nibble) = Pt = point transform.
    let ah = data[offset] >> 4;
    let al = data[offset] & 0x0F;
    if ah != 0 {
        return Err(CodecError::InvalidData(format!(
            "invalid SOS Ah={ah} (must be 0 for lossless)"
        )));
    }
    // Pt must be strictly less than the sample precision (T.81 Table B.3).
    if al >= dec.precision {
        return Err(CodecError::InvalidData(format!(
            "invalid SOS point transform Al={al} (must be < precision {})",
            dec.precision
        )));
    }
    dec.point_transform = al;

    Ok(())
}

/// Parse DRI (Define Restart Interval).
fn parse_dri<R: Read>(r: &mut R, dec: &mut Decoder) -> Result<(), CodecError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    // buf[0..2] = length (always 4), buf[2..4] = restart interval
    dec.restart_interval = u16::from_be_bytes([buf[2], buf[3]]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid JPEG Lossless bitstream for a 2x2 image
    /// and verify it can be decoded.
    #[test]
    fn decode_minimal_roundtrip() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 2, 2, &mut encoded).unwrap();

        let (decoded, w, h) = decode(&encoded, 2, 2).unwrap();
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(decoded, vec![100, 200, 300, 400]);
    }

    #[test]
    fn decode_dimension_mismatch() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 2, 2, &mut encoded).unwrap();

        // Ask for wrong dimensions
        let result = decode(&encoded, 3, 3);
        assert!(result.is_err());
    }

    #[test]
    fn decode_missing_soi() {
        let data = [0x00, 0x00, 0xFF, 0xD9];
        let result = decode(&data, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn decode_truncated() {
        // Just SOI, nothing else
        let data = [0xFF, 0xD8];
        let result = decode(&data, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn decode_unsupported_sof() {
        // SOI + SOF0 (baseline DCT, not lossless)
        let data = [0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x02];
        let result = decode(&data, 1, 1);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Validation (J1): SOF3 / SOS contract per T.81 Table B.3.
    // -----------------------------------------------------------------------

    /// Build a SOI + SOF3 (only) stream with the given precision / component count.
    fn sof3_stream(precision: u8, ncomp: u8) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8]; // SOI
                                      // payload: P, H(2), W(2), Nf, then Nf * (Ci, Hi|Vi, Tqi)
        let mut payload = vec![precision, 0x00, 0x01, 0x00, 0x01, ncomp];
        for i in 0..ncomp {
            payload.extend_from_slice(&[i + 1, 0x11, 0x00]);
        }
        let len = (payload.len() + 2) as u16;
        v.extend_from_slice(&[0xFF, 0xC3]);
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&payload);
        v
    }

    /// Build a SOI + valid-SOF3 (P=8, 1 comp) + SOS stream with the given
    /// Ss / Se / (Ah|Al) bytes so SOS validation can be exercised in isolation.
    fn sos_stream(ss: u8, se: u8, ah_al: u8) -> Vec<u8> {
        let mut v = sof3_stream(8, 1);
        let payload = vec![1u8, 1, 0x00, ss, se, ah_al];
        let len = (payload.len() + 2) as u16;
        v.extend_from_slice(&[0xFF, 0xDA]);
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&payload);
        v
    }

    #[test]
    fn reject_precision_zero() {
        assert!(matches!(
            decode(&sof3_stream(0, 1), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_precision_one() {
        assert!(matches!(
            decode(&sof3_stream(1, 1), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_precision_seventeen() {
        assert!(matches!(
            decode(&sof3_stream(17, 1), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_multi_component() {
        // Nf = 3 must be rejected as Unsupported (single-component only).
        assert!(matches!(
            decode(&sof3_stream(8, 3), 1, 1),
            Err(CodecError::Unsupported(_))
        ));
    }

    #[test]
    fn reject_predictor_zero() {
        // Ss = 0 (hierarchical-only) is illegal for lossless.
        assert!(matches!(
            decode(&sos_stream(0, 0, 0), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_predictor_eight() {
        assert!(matches!(
            decode(&sos_stream(8, 0, 0), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_nonzero_se() {
        assert!(matches!(
            decode(&sos_stream(1, 5, 0), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_nonzero_ah() {
        // Ah = high nibble = 1.
        assert!(matches!(
            decode(&sos_stream(1, 0, 0x10), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_point_transform_ge_precision() {
        // precision = 8, Al = 8 must be rejected.
        assert!(matches!(
            decode(&sos_stream(1, 0, 0x08), 1, 1),
            Err(CodecError::InvalidData(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Full-pipeline round-trips: predictor x point-transform x precision.
    // -----------------------------------------------------------------------

    #[test]
    fn roundtrip_matrix_predictor_pt_precision() {
        use crate::encode::{encode_with_options, EncodeOptions};
        let (w, h) = (5usize, 4usize);
        for &precision in &[2u8, 8, 12, 16] {
            let modulus: u64 = 1u64 << precision;
            // Deterministic image whose samples all fit in `precision` bits.
            let pixels: Vec<u16> = (0..(w * h))
                .map(|i| ((i as u64 * 2_654_435_761) % modulus) as u16)
                .collect();
            for predictor in 1..=7u8 {
                for &pt in &[0u8, 2u8] {
                    if pt >= precision {
                        continue;
                    }
                    let opts = EncodeOptions {
                        predictor,
                        point_transform: pt,
                        restart_interval_rows: 0,
                        precision,
                    };

                    let mut buf = Vec::new();
                    encode_with_options(&pixels, w as u32, h as u32, &opts, &mut buf).unwrap();
                    let (decoded, dw, dh) = decode(&buf, w as u32, h as u32).unwrap();
                    assert_eq!((dw, dh), (w as u32, h as u32));
                    let expected: Vec<u16> = pixels.iter().map(|&p| (p >> pt) << pt).collect();
                    assert_eq!(
                        decoded, expected,
                        "roundtrip mismatch predictor={predictor} pt={pt} precision={precision}"
                    );
                }
            }
        }
    }

    #[test]
    fn roundtrip_constant_image() {
        let val = 12345u16;
        let pixels = vec![val; 64];
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 8, 8, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 8, 8).unwrap();
        assert_eq!(decoded, vec![val; 64]);
    }

    #[test]
    fn roundtrip_gradient() {
        let pixels: Vec<u16> = (0..256).collect();
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 16, 16, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 16, 16).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn roundtrip_16bit_full_range() {
        // Exercise full 16-bit range
        let pixels: Vec<u16> = vec![0, 1, 65534, 65535, 32768, 32767, 100, 60000];
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 4, 2, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 4, 2).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn roundtrip_large_image() {
        // A larger image to stress-test the codec
        let width = 64u32;
        let height = 64u32;
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // A pattern that exercises the predictor
                pixels.push(((x * 100 + y * 200) % 65536) as u16);
            }
        }
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, width, height, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, width, height).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn roundtrip_single_pixel() {
        let pixels = vec![42000u16];
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 1, 1, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 1, 1).unwrap();
        assert_eq!(decoded, vec![42000]);
    }

    #[test]
    fn roundtrip_single_row() {
        let pixels: Vec<u16> = (0..128).map(|i| i * 512).collect();
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 128, 1, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 128, 1).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn roundtrip_single_column() {
        let pixels: Vec<u16> = (0..128).map(|i| i * 512).collect();
        let mut encoded = Vec::new();
        crate::encode::encode(&pixels, 1, 128, &mut encoded).unwrap();

        let (decoded, _, _) = decode(&encoded, 1, 128).unwrap();
        assert_eq!(decoded, pixels);
    }

    // -----------------------------------------------------------------------
    // Restart intervals (J4): T.81 Annex H.1.1 row-aligned restarts.
    // -----------------------------------------------------------------------

    use crate::encode::{encode_with_options, EncodeOptions};

    /// Deterministic image whose samples all fit in `precision` bits.
    fn det_image(w: usize, h: usize, precision: u8) -> Vec<u16> {
        let modulus: u64 = 1u64 << precision;
        (0..(w * h))
            .map(|i| ((i as u64 * 2_654_435_761 + 12345) % modulus) as u16)
            .collect()
    }

    fn restart_opts(predictor: u8, pt: u8, precision: u8, rows: u16) -> EncodeOptions {
        EncodeOptions {
            predictor,
            point_transform: pt,
            restart_interval_rows: rows,
            precision,
        }
    }

    /// Collect the restart-marker indices (RST0..RST7 -> 0..7) appearing in the
    /// stream, in order.  Any `0xFF Dn` with n in 0xD0..=0xD7 is unambiguously a
    /// restart marker: entropy `0xFF` bytes are always stuffed as `0xFF 0x00`.
    fn restart_markers(buf: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < buf.len() {
            if buf[i] == 0xFF && (0xD0..=0xD7).contains(&buf[i + 1]) {
                out.push(buf[i + 1] - 0xD0);
                i += 2;
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn restart_roundtrip_matrix() {
        // rows x predictor x pt x precision on several deterministic images.
        for &(w, h) in &[(8usize, 6usize), (5, 5), (1, 4)] {
            for &precision in &[8u8, 16u8] {
                let pixels = det_image(w, h, precision);
                for &rows in &[1u16, 2, 3, h as u16] {
                    for &predictor in &[1u8, 4, 7] {
                        for &pt in &[0u8, 2u8] {
                            if pt >= precision {
                                continue;
                            }
                            let opts = restart_opts(predictor, pt, precision, rows);
                            let mut buf = Vec::new();
                            encode_with_options(&pixels, w as u32, h as u32, &opts, &mut buf)
                                .unwrap();
                            let (decoded, dw, dh) = decode(&buf, w as u32, h as u32).unwrap();
                            assert_eq!((dw, dh), (w as u32, h as u32));
                            let expected: Vec<u16> =
                                pixels.iter().map(|&p| (p >> pt) << pt).collect();
                            assert_eq!(
                                decoded, expected,
                                "restart roundtrip mismatch w={w} h={h} rows={rows} \
                                 predictor={predictor} pt={pt} precision={precision}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn restart_markers_cycle_mod_8() {
        // A tall 1-wide image with rows=1 forces one restart per row after the
        // first: 10 rows -> 9 restarts, indices cycling 0..7 then wrapping to 0.
        let (w, h) = (1usize, 10usize);
        let pixels = det_image(w, h, 8);
        let opts = restart_opts(1, 0, 8, 1);
        let mut buf = Vec::new();
        encode_with_options(&pixels, w as u32, h as u32, &opts, &mut buf).unwrap();

        assert_eq!(
            restart_markers(&buf),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 0],
            "restart indices must cycle mod 8"
        );

        // And it still round-trips exactly.
        let (decoded, _, _) = decode(&buf, w as u32, h as u32).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn restart_dri_value_is_rows_times_width() {
        let (w, h) = (6usize, 4usize);
        let pixels = det_image(w, h, 8);
        let opts = restart_opts(1, 0, 8, 2); // 2 rows x 6 width = 12
        let mut buf = Vec::new();
        encode_with_options(&pixels, w as u32, h as u32, &opts, &mut buf).unwrap();

        let dri = buf
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xDD)
            .expect("DRI present");
        assert_eq!(&buf[dri + 2..dri + 4], &[0x00, 0x04]);
        assert_eq!(
            u16::from_be_bytes([buf[dri + 4], buf[dri + 5]]),
            12,
            "Ri must equal rows * width"
        );
    }

    /// Restart resets prediction (H.1.2.1): the segment after a restart is coded
    /// independently of the rows before it, so two images that share row 1 but
    /// differ in row 0 must produce byte-identical entropy after the RST0 marker.
    /// A decoder that failed to reset would make row 1 depend on row 0.
    #[test]
    fn restart_resets_predictor_row_independence() {
        let (w, h) = (3usize, 2usize);
        let row1 = [100u16, 110, 120];
        let img_a: Vec<u16> = [10, 20, 30].into_iter().chain(row1).collect();
        let img_b: Vec<u16> = [200, 150, 90].into_iter().chain(row1).collect();
        let opts = restart_opts(4, 0, 8, 1); // restart after every row

        let mut a = Vec::new();
        let mut b = Vec::new();
        encode_with_options(&img_a, w as u32, h as u32, &opts, &mut a).unwrap();
        encode_with_options(&img_b, w as u32, h as u32, &opts, &mut b).unwrap();

        // Slice from the RST0 marker (0xFF 0xD0) to just before EOI (0xFF 0xD9).
        let tail = |buf: &[u8]| -> Vec<u8> {
            let start = buf
                .windows(2)
                .position(|x| x[0] == 0xFF && x[1] == 0xD0)
                .expect("RST0 present");
            let end = buf.len() - 2; // strip trailing EOI
            buf[start..end].to_vec()
        };
        assert_eq!(
            tail(&a),
            tail(&b),
            "row-1 entropy must be independent of row 0 across a restart"
        );

        // Both must also decode exactly.
        assert_eq!(decode(&a, w as u32, h as u32).unwrap().0, img_a);
        assert_eq!(decode(&b, w as u32, h as u32).unwrap().0, img_b);
    }

    // --- Error cases -------------------------------------------------------

    /// Build a valid restart-bearing stream for corruption tests.
    fn restart_stream(w: usize, h: usize, rows: u16) -> Vec<u8> {
        let pixels = det_image(w, h, 8);
        let opts = restart_opts(1, 0, 8, rows);
        let mut buf = Vec::new();
        encode_with_options(&pixels, w as u32, h as u32, &opts, &mut buf).unwrap();
        buf
    }

    #[test]
    fn reject_ri_not_multiple_of_width() {
        // width 4, valid Ri = 4; corrupt the DRI value to 3 (not a multiple).
        let mut buf = restart_stream(4, 4, 1);
        let dri = buf
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xDD)
            .unwrap();
        buf[dri + 4] = 0x00;
        buf[dri + 5] = 0x03;
        assert!(matches!(
            decode(&buf, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_restart_out_of_order() {
        // Corrupt the first RST0 (0xD0) into RST3 (0xD3): expected index mismatch.
        let mut buf = restart_stream(4, 4, 1);
        let rst = buf
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xD0)
            .unwrap();
        buf[rst + 1] = 0xD3;
        assert!(matches!(
            decode(&buf, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_restart_without_dri() {
        // Remove the DRI segment (6 bytes: marker + len + Ri) but keep the RSTn
        // markers in the entropy: an unexpected restart must be rejected.
        let mut buf = restart_stream(4, 4, 1);
        let dri = buf
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xDD)
            .unwrap();
        buf.drain(dri..dri + 6);
        assert!(matches!(
            decode(&buf, 4, 4),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_missing_restart_marker() {
        // Splice out the first RST marker bytes so the expected restart is gone.
        let mut buf = restart_stream(4, 6, 1);
        let rst = buf
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xD0)
            .unwrap();
        buf.drain(rst..rst + 2);
        assert!(decode(&buf, 4, 6).is_err());
    }

    #[test]
    fn truncation_inside_restart_interval_errors_not_panics() {
        // Truncating mid-scan (before later restarts / EOI) must error, not panic.
        let full = restart_stream(4, 8, 1);
        let sos = full
            .windows(2)
            .position(|x| x[0] == 0xFF && x[1] == 0xDA)
            .unwrap();
        // Keep the SOS header (8 bytes) plus a few entropy bytes, drop the rest.
        let cut = sos + 8 + 3;
        let truncated = &full[..cut.min(full.len())];
        assert!(decode(truncated, 4, 8).is_err());
    }
}
