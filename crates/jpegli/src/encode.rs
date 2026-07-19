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
const MARKER_DRI: u16 = 0xFFDD;

// ---------------------------------------------------------------------------
// Encode options
// ---------------------------------------------------------------------------

/// Options controlling JPEG Lossless (T.81 Annex H) encoding.
///
/// Construct via [`EncodeOptions::default`] and override fields as needed; the
/// struct is `#[non_exhaustive]` so new fields can be added without a breaking
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EncodeOptions {
    /// DPCM predictor selector (SOS `Ss`), 1..=7. Default 1 (Ra, left neighbor).
    pub predictor: u8,
    /// Point transform (SOS `Al` / `Pt`), 0..=precision-1. Default 0.
    ///
    /// The codec operates in the reduced domain P' = precision - point_transform:
    /// every input sample is shifted right by `point_transform` before coding.
    pub point_transform: u8,
    /// Restart every N MCU rows; 0 disables restart intervals. The emitted DRI
    /// value is `restart_interval_rows * width` (validated to fit in a `u16`).
    /// Default 0.
    pub restart_interval_rows: u16,
    /// Sample precision P (SOF3), 2..=16. Default 16.
    pub precision: u8,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            predictor: 1,
            point_transform: 0,
            restart_interval_rows: 0,
            precision: 16,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encode a 16-bit grayscale image into JPEG Lossless format using default
/// options (predictor 1, no point transform, 16-bit precision, no restarts).
///
/// `pixels` is a row-major pixel buffer of length `width * height`.
/// Writes a complete JPEG bitstream (SOI through EOI) to `w`.
pub fn encode(
    pixels: &[u16],
    width: u32,
    height: u32,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    encode_with_options(pixels, width, height, &EncodeOptions::default(), w)
}

/// Encode a 16-bit grayscale image into JPEG Lossless format with explicit
/// [`EncodeOptions`].
///
/// `pixels` is a row-major pixel buffer of length `width * height`. Writes a
/// complete JPEG bitstream (SOI through EOI) to `w`.
pub fn encode_with_options(
    pixels: &[u16],
    width: u32,
    height: u32,
    opts: &EncodeOptions,
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

    // --- Option validation (T.81 Table B.3 / B.2.2) --------------------------
    if !(2..=16).contains(&opts.precision) {
        return Err(CodecError::InvalidData(format!(
            "invalid precision {} (must be 2..=16)",
            opts.precision
        )));
    }
    if !(1..=7).contains(&opts.predictor) {
        return Err(CodecError::InvalidData(format!(
            "invalid predictor {} (must be 1..=7)",
            opts.predictor
        )));
    }
    if opts.point_transform >= opts.precision {
        return Err(CodecError::InvalidData(format!(
            "invalid point transform {} (must be < precision {})",
            opts.point_transform, opts.precision
        )));
    }

    // Every sample must fit in `precision` bits.
    let sample_limit: u32 = 1u32 << opts.precision;
    if let Some(&max) = pixels.iter().max() {
        if (max as u32) >= sample_limit {
            return Err(CodecError::InvalidData(format!(
                "sample value {max} exceeds precision {} (max {})",
                opts.precision,
                sample_limit - 1
            )));
        }
    }

    // Restart-interval math must fit in the 16-bit DRI field (T.81 H.1.1: the
    // DRI value is `restart_interval_rows * width` MCUs, one MCU per sample).
    let restart_interval: u16 = if opts.restart_interval_rows > 0 {
        let dri = (opts.restart_interval_rows as u32) * (width_usize as u32);
        if dri == 0 || dri > 65535 {
            return Err(CodecError::InvalidData(format!(
                "restart interval {} rows x {} width = {} does not fit in the 16-bit DRI field",
                opts.restart_interval_rows, width_usize, dri
            )));
        }
        dri as u16
    } else {
        0
    };

    let ht = build_default_table();

    write_marker(w, MARKER_SOI)?;
    write_app0(w)?;
    write_sof3(w, width_usize, height_usize, opts.precision)?;
    write_dht(w, &ht)?;
    if restart_interval > 0 {
        write_dri(w, restart_interval)?;
    }
    write_sos_and_scan(
        w,
        &ht,
        pixels,
        width_usize,
        height_usize,
        opts.precision,
        opts.predictor,
        opts.point_transform,
        opts.restart_interval_rows as usize,
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

/// Write DRI (Define Restart Interval) marker segment (T.81 B.2.4).
fn write_dri(w: &mut dyn Write, restart_interval: u16) -> Result<(), CodecError> {
    write_marker(w, MARKER_DRI)?;
    // Length = 4 (2 length bytes + 2 interval bytes), then Ri.
    let mut data = [0u8; 4];
    data[0..2].copy_from_slice(&4u16.to_be_bytes());
    data[2..4].copy_from_slice(&restart_interval.to_be_bytes());
    w.write_all(&data)?;
    Ok(())
}

/// Write SOS header and entropy-coded scan data.
#[allow(clippy::too_many_arguments)]
fn write_sos_and_scan(
    w: &mut dyn Write,
    ht: &HuffmanTable,
    pixels: &[u16],
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
    point_transform: u8,
    restart_interval_rows: usize,
) -> Result<(), CodecError> {
    write_marker(w, MARKER_SOS)?;
    // Length = 2(len) + 1(ncomp) + 2(comp spec) + 3(Ss,Se,Ah|Al) = 8
    let length: u16 = 8;
    let header: [u8; 8] = [
        (length >> 8) as u8,
        length as u8,
        1,               // 1 component
        1,               // Component ID = 1
        0x00,            // DC table 0, AC table 0
        predictor,       // Ss = predictor selection
        0,               // Se = 0 (lossless)
        point_transform, // Ah=0, Al=point transform (Pt)
    ];
    w.write_all(&header)?;

    // Write entropy-coded scan data
    scan::encode_scan(
        w,
        ht,
        pixels,
        width,
        height,
        precision,
        predictor,
        point_transform,
        restart_interval_rows,
    )?;

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

    // -----------------------------------------------------------------------
    // EncodeOptions (J2)
    // -----------------------------------------------------------------------

    #[test]
    fn default_options_values() {
        let opts = EncodeOptions::default();
        assert_eq!(opts.predictor, 1);
        assert_eq!(opts.point_transform, 0);
        assert_eq!(opts.restart_interval_rows, 0);
        assert_eq!(opts.precision, 16);
    }

    fn opts(predictor: u8, point_transform: u8, precision: u8) -> EncodeOptions {
        EncodeOptions {
            predictor,
            point_transform,
            restart_interval_rows: 0,
            precision,
        }
    }

    #[test]
    fn options_write_ss_and_al() {
        // Encode with predictor 5 and point transform 2; confirm the SOS bytes.
        let pixels = vec![0u16, 4, 8, 12];
        let mut buf = Vec::new();
        encode_with_options(&pixels, 2, 2, &opts(5, 2, 12), &mut buf).unwrap();

        // Find SOS marker (0xFFDA); the 8-byte header follows.
        let pos = buf
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] == 0xDA)
            .expect("SOS present");
        // header layout: FF DA, len(2)=0008, ncomp=1, id=1, table=00, Ss, Se, Ah|Al
        assert_eq!(buf[pos + 7], 5, "Ss should equal predictor");
        assert_eq!(buf[pos + 8], 0, "Se should be 0");
        assert_eq!(
            buf[pos + 9],
            2,
            "Ah|Al low nibble should equal point transform"
        );

        // SOF3 precision byte.
        let sof = buf
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] == 0xC3)
            .expect("SOF3 present");
        assert_eq!(
            buf[sof + 4],
            12,
            "SOF3 precision should equal opts.precision"
        );
    }

    #[test]
    fn reject_predictor_out_of_range() {
        let pixels = vec![0u16; 4];
        let mut buf = Vec::new();
        assert!(matches!(
            encode_with_options(&pixels, 2, 2, &opts(0, 0, 16), &mut buf),
            Err(CodecError::InvalidData(_))
        ));
        buf.clear();
        assert!(matches!(
            encode_with_options(&pixels, 2, 2, &opts(8, 0, 16), &mut buf),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_precision_out_of_range() {
        let pixels = vec![0u16; 4];
        for p in [0u8, 1, 17] {
            let mut buf = Vec::new();
            assert!(
                matches!(
                    encode_with_options(&pixels, 2, 2, &opts(1, 0, p), &mut buf),
                    Err(CodecError::InvalidData(_))
                ),
                "precision {p} should be rejected"
            );
        }
    }

    #[test]
    fn reject_point_transform_ge_precision() {
        let pixels = vec![0u16; 4];
        let mut buf = Vec::new();
        assert!(matches!(
            encode_with_options(&pixels, 2, 2, &opts(1, 8, 8), &mut buf),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn reject_sample_exceeding_precision() {
        // precision 8 => samples must be < 256.
        let pixels = vec![10u16, 20, 300, 40];
        let mut buf = Vec::new();
        assert!(matches!(
            encode_with_options(&pixels, 2, 2, &opts(1, 0, 8), &mut buf),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn restart_interval_emits_dri_marker() {
        // Valid restart math (1 row x 4 width = 4 <= 65535); J4 emits DRI + RSTn.
        let pixels = vec![0u16; 16];
        let mut buf = Vec::new();
        let o = EncodeOptions {
            predictor: 1,
            point_transform: 0,
            restart_interval_rows: 1,
            precision: 16,
        };
        encode_with_options(&pixels, 4, 4, &o, &mut buf).unwrap();

        // DRI marker 0xFFDD carrying Ri = rows * width = 1 * 4 = 4, positioned
        // before SOS.
        let dri = buf
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] == 0xDD)
            .expect("DRI present");
        let sos = buf
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] == 0xDA)
            .expect("SOS present");
        assert!(dri < sos, "DRI must precede SOS");
        // Layout: FF DD, len(2)=0004, Ri(2).
        assert_eq!(
            &buf[dri + 2..dri + 4],
            &[0x00, 0x04],
            "DRI length must be 4"
        );
        assert_eq!(
            &buf[dri + 4..dri + 6],
            &[0x00, 0x04],
            "Ri must be rows*width"
        );
    }

    #[test]
    fn restart_interval_overflow_is_invalid() {
        // 100 rows x 1000 width = 100_000 does not fit the 16-bit DRI field.
        let pixels = vec![0u16; 1000];
        let mut buf = Vec::new();
        let o = EncodeOptions {
            predictor: 1,
            point_transform: 0,
            restart_interval_rows: 100,
            precision: 16,
        };
        assert!(matches!(
            encode_with_options(&pixels, 1000, 1, &o, &mut buf),
            Err(CodecError::InvalidData(_))
        ));
    }
}
