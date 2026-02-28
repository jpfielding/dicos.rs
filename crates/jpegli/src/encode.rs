//! Full JPEG Lossless encoding pipeline.
//!
//! Writes a complete JPEG Lossless bitstream with SOI, APP0, SOF3, DHT, SOS,
//! entropy-coded scan data, and EOI markers.

use std::io::Write;

use crate::error::CodecError;

use crate::huffman::{build_default_table, HuffmanTable};
use crate::scan;

// ---------------------------------------------------------------------------
// JPEG markers
// ---------------------------------------------------------------------------

const MARKER_SOI: u16 = 0xFFD8;
const MARKER_EOI: u16 = 0xFFD9;
const MARKER_SOF3: u16 = 0xFFC3;
const MARKER_DHT: u16 = 0xFFC4;
const MARKER_SOS: u16 = 0xFFDA;
const MARKER_APP0: u16 = 0xFFE0;

/// Default predictor selection (predictor 1 = Ra, previous pixel in row).
const DEFAULT_PREDICTOR: u8 = 1;

/// Default point transform (0 = no shift, full precision).
const DEFAULT_POINT_TRANSFORM: u8 = 0;

/// Precision for 16-bit images.
const PRECISION_16: u8 = 16;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into JPEG Lossless format.
///
/// `pixels` is a row-major pixel buffer of length `width * height`.
/// Writes a complete JPEG bitstream (SOI through EOI) to `w`.
/// Uses predictor 1 (left neighbor) and no point transform.
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
    if width_usize > 65535 || height_usize > 65535 {
        return Err(CodecError::InvalidData(
            "image dimensions exceed JPEG maximum (65535)".into(),
        ));
    }

    let ht = build_default_table();

    write_marker(w, MARKER_SOI)?;
    write_app0(w)?;
    write_sof3(w, width_usize, height_usize, PRECISION_16)?;
    write_dht(w, &ht)?;
    write_sos_and_scan(
        w,
        &ht,
        pixels,
        width_usize,
        height_usize,
        PRECISION_16,
        DEFAULT_PREDICTOR,
    )?;
    write_marker(w, MARKER_EOI)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Marker writers
// ---------------------------------------------------------------------------

fn write_marker(w: &mut dyn Write, marker: u16) -> Result<(), CodecError> {
    w.write_all(&marker.to_be_bytes())?;
    Ok(())
}

/// Write JFIF APP0 marker segment.
fn write_app0(w: &mut dyn Write) -> Result<(), CodecError> {
    write_marker(w, MARKER_APP0)?;
    let data: [u8; 16] = [
        0x00, 0x10, // Length = 16
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF\0"
        0x01, 0x01, // Version 1.1
        0x00, // Units: no units
        0x00, 0x01, // X density = 1
        0x00, 0x01, // Y density = 1
        0x00, 0x00, // No thumbnail
    ];
    w.write_all(&data)?;
    Ok(())
}

/// Write SOF3 (Start of Frame -- Lossless Huffman) marker segment.
fn write_sof3(
    w: &mut dyn Write,
    width: usize,
    height: usize,
    precision: u8,
) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOF3)?;
    // Length = 2(len) + 1(prec) + 2(height) + 2(width) + 1(ncomp) + 3(comp) = 11
    let length: u16 = 11;
    let mut data = [0u8; 11];
    data[0..2].copy_from_slice(&length.to_be_bytes());
    data[2] = precision;
    data[3] = (height >> 8) as u8;
    data[4] = height as u8;
    data[5] = (width >> 8) as u8;
    data[6] = width as u8;
    data[7] = 1; // 1 component
    data[8] = 1; // Component ID = 1
    data[9] = 0x11; // H=1, V=1 sampling
    data[10] = 0; // Quantization table (unused in lossless)
    w.write_all(&data)?;
    Ok(())
}

/// Write DHT (Define Huffman Table) marker segment.
fn write_dht(w: &mut dyn Write, ht: &HuffmanTable) -> Result<(), CodecError> {
    write_marker(w, MARKER_DHT)?;
    // Length = 2(len) + 1(class|id) + 16(bits) + N(values)
    let length: u16 = 2 + 1 + 16 + ht.values.len() as u16;
    w.write_all(&length.to_be_bytes())?;
    w.write_all(&[0x00])?; // Table class 0 (DC), Table ID 0
                           // Write BITS[1..=16]
    w.write_all(&ht.bits[1..=16])?;
    // Write HUFFVAL
    w.write_all(&ht.values)?;
    Ok(())
}

/// Write SOS header and entropy-coded scan data.
fn write_sos_and_scan(
    w: &mut dyn Write,
    ht: &HuffmanTable,
    pixels: &[u16],
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOS)?;
    // Length = 2(len) + 1(ncomp) + 2(comp spec) + 3(Ss,Se,Ah|Al) = 8
    let length: u16 = 8;
    let header: [u8; 8] = [
        (length >> 8) as u8,
        length as u8,
        1,                       // 1 component
        1,                       // Component ID = 1
        0x00,                    // DC table 0, AC table 0
        predictor,               // Ss = predictor selection
        0,                       // Se = 0 (lossless)
        DEFAULT_POINT_TRANSFORM, // Ah=0, Al=point transform
    ];
    w.write_all(&header)?;

    // Write entropy-coded scan data
    scan::encode_scan(w, ht, pixels, width, height, precision, predictor)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_writes_soi_and_eoi() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        // Starts with SOI
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0xD8);
        // Ends with EOI
        let len = buf.len();
        assert_eq!(buf[len - 2], 0xFF);
        assert_eq!(buf[len - 1], 0xD9);
    }

    #[test]
    fn encode_contains_sof3() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        // Search for SOF3 marker 0xFFC3
        let has_sof3 = buf.windows(2).any(|w| w[0] == 0xFF && w[1] == 0xC3);
        assert!(has_sof3, "output should contain SOF3 marker");
    }

    #[test]
    fn encode_contains_dht() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        let has_dht = buf.windows(2).any(|w| w[0] == 0xFF && w[1] == 0xC4);
        assert!(has_dht, "output should contain DHT marker");
    }

    #[test]
    fn encode_contains_sos() {
        let pixels = vec![100u16, 200, 300, 400];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        let has_sos = buf.windows(2).any(|w| w[0] == 0xFF && w[1] == 0xDA);
        assert!(has_sos, "output should contain SOS marker");
    }

    #[test]
    fn encode_zero_dimension_fails() {
        let pixels: Vec<u16> = vec![];
        let mut buf = Vec::new();
        let result = encode(&pixels, 0, 10, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn encode_nonempty_output() {
        let pixels = vec![0u16; 16];
        let mut buf = Vec::new();
        encode(&pixels, 4, 4, &mut buf).unwrap();
        // A valid JPEG must be longer than just SOI+EOI (4 bytes)
        assert!(buf.len() > 4);
    }

    #[test]
    fn sof3_encodes_dimensions() {
        let mut buf = Vec::new();
        write_sof3(&mut buf, 320, 240, 16).unwrap();
        // write_sof3 writes: marker(2) + data(11) = 13 bytes
        // Marker: FF C3 at [0..2]
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0xC3);
        // Length bytes at [2..4] = 0x000B = 11
        assert_eq!(buf[2], 0x00);
        assert_eq!(buf[3], 0x0B);
        // Precision at [4]
        assert_eq!(buf[4], 16);
        // Height = 240 = 0x00F0 at [5..7]
        assert_eq!(buf[5], 0x00);
        assert_eq!(buf[6], 0xF0);
        // Width = 320 = 0x0140 at [7..9]
        assert_eq!(buf[7], 0x01);
        assert_eq!(buf[8], 0x40);
    }
}
