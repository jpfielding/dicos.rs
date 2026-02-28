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
    #[allow(dead_code)]
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

    if offset >= data.len() {
        return Err(CodecError::InvalidData(
            "SOS payload too short for spectral selection".into(),
        ));
    }

    // Ss = predictor selection
    dec.predictor = data[offset];
    offset += 1;

    // Se = always 0 for lossless
    offset += 1;

    // Ah (high nibble) | Al (low nibble) = point transform
    if offset < data.len() {
        dec.point_transform = data[offset] & 0x0F;
    }

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
}
