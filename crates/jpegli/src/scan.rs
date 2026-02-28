//! Scan data parsing and writing for JPEG Lossless.
//!
//! Handles bit-level I/O with JPEG byte stuffing (0xFF -> 0xFF 0x00),
//! Huffman symbol decoding, DPCM predictor selection (1-7), and the
//! overall scan encode/decode loops.

use std::io::{self, Read, Write};

use crate::huffman::{categorize, extend, HuffmanTable};

// ---------------------------------------------------------------------------
// Bit reader (decoding)
// ---------------------------------------------------------------------------

/// Reads bits from a JPEG entropy-coded segment, handling byte stuffing.
pub(crate) struct BitReader<R> {
    reader: R,
    buf: u32,
    bits: u8,
    eof: bool,
}

impl<R: Read> BitReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: 0,
            bits: 0,
            eof: false,
        }
    }

    /// Fill the internal buffer up to at least 16 bits (or until EOF / marker).
    fn fill(&mut self) -> io::Result<()> {
        let mut byte_buf = [0u8; 1];
        while self.bits < 24 && !self.eof {
            match self.reader.read(&mut byte_buf) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(());
                }
                Ok(_) => {
                    let c = byte_buf[0];
                    if c == 0xFF {
                        // Read the next byte to check for stuffing vs marker
                        match self.reader.read(&mut byte_buf) {
                            Ok(0) => {
                                // 0xFF at end of data
                                self.eof = true;
                                return Ok(());
                            }
                            Ok(_) => {
                                let next = byte_buf[0];
                                if next == 0x00 {
                                    // Stuffed byte: emit 0xFF
                                    self.buf = (self.buf << 8) | 0xFF;
                                    self.bits += 8;
                                } else if (0xD0..=0xD7).contains(&next) {
                                    // Restart marker: skip it
                                    continue;
                                } else {
                                    // Another marker (e.g. EOI): stop reading
                                    self.eof = true;
                                    return Ok(());
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        self.buf = (self.buf << 8) | (c as u32);
                        self.bits += 8;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Read exactly `n` bits (n <= 24).
    pub fn read_bits(&mut self, n: u8) -> io::Result<u32> {
        debug_assert!(n <= 24);
        if n == 0 {
            return Ok(0);
        }
        while self.bits < n {
            self.fill()?;
            if self.eof && self.bits < n {
                // Pad with zeros on EOF (match Go behavior)
                let have = self.buf & ((1 << self.bits) - 1);
                let missing = n - self.bits;
                let result = have << missing;
                self.bits = 0;
                return Ok(result);
            }
        }
        self.bits -= n;
        let mask = (1u32 << n) - 1;
        Ok((self.buf >> self.bits) & mask)
    }

    /// Peek at the top `n` bits without consuming them.
    pub fn peek_bits(&mut self, n: u8) -> io::Result<u32> {
        debug_assert!(n <= 24);
        while self.bits < n {
            self.fill()?;
            if self.eof && self.bits < n {
                let have = self.buf & ((1 << self.bits) - 1);
                let missing = n - self.bits;
                return Ok(have << missing);
            }
        }
        let mask = (1u32 << n) - 1;
        Ok((self.buf >> (self.bits - n)) & mask)
    }

    /// Consume `n` bits that were previously peeked.
    pub fn consume_bits(&mut self, n: u8) {
        if self.bits >= n {
            self.bits -= n;
        } else {
            self.bits = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Bit writer (encoding)
// ---------------------------------------------------------------------------

/// Writes bits to a JPEG entropy-coded segment with byte stuffing.
pub(crate) struct BitWriter<W> {
    writer: W,
    buf: u32,
    bits: u8,
}

impl<W: Write> BitWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            buf: 0,
            bits: 0,
        }
    }

    /// Write `n` bits (MSB-first) from the low `n` bits of `val`.
    pub fn write_bits(&mut self, val: u32, n: u8) -> io::Result<()> {
        let mask = if n >= 32 { u32::MAX } else { (1u32 << n) - 1 };
        self.buf = (self.buf << n) | (val & mask);
        self.bits += n;

        while self.bits >= 8 {
            self.bits -= 8;
            let byte_val = (self.buf >> self.bits) as u8;
            self.writer.write_all(&[byte_val])?;
            if byte_val == 0xFF {
                self.writer.write_all(&[0x00])?; // byte stuffing
            }
        }
        Ok(())
    }

    /// Flush any remaining bits, padding with 1-bits to a byte boundary.
    pub fn flush(mut self) -> io::Result<W> {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            let padded = (self.buf << pad) | ((1u32 << pad) - 1);
            let byte_val = padded as u8;
            self.writer.write_all(&[byte_val])?;
            if byte_val == 0xFF {
                self.writer.write_all(&[0x00])?;
            }
        }
        Ok(self.writer)
    }
}

// ---------------------------------------------------------------------------
// Huffman decode helper
// ---------------------------------------------------------------------------

/// Decode one Huffman symbol from the bit reader using `ht`.
pub(crate) fn decode_huffman<R: Read>(br: &mut BitReader<R>, ht: &HuffmanTable) -> io::Result<u8> {
    // Fast path: 8-bit lookup
    let peek = br.peek_bits(8)? as u8;
    if let Some((size, value)) = ht.fast_lookup(peek) {
        br.consume_bits(size);
        return Ok(value);
    }

    // At EOF with only padding bits remaining: treat as SSSS=0 (no difference).
    // JPEG encoders pad the final byte with 1-bits, which may not form a valid
    // Huffman code. When the stream is exhausted, remaining bits are padding.
    if br.eof {
        br.consume_bits(br.bits);
        return Ok(0);
    }

    // Slow path: decode bit by bit
    let mut code: u16 = 0;
    for size in 1..=16u8 {
        let bit = br.read_bits(1)?;
        code = (code << 1) | (bit as u16);
        if let Some(value) = ht.decode_slow(code, size) {
            return Ok(value);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Huffman code: 16 bits without match",
    ))
}

// ---------------------------------------------------------------------------
// Predictor
// ---------------------------------------------------------------------------

/// Compute the predicted pixel value using one of the 7 DPCM predictors.
///
/// Arguments:
/// - `curr_row`: current row decoded so far (index < x are valid)
/// - `prev_row`: previous row (fully decoded)
/// - `x`, `y`: pixel coordinates
/// - `predictor`: predictor selection (1-7)
/// - `precision`: bits per sample
///
/// Special cases:
/// - (0,0): predicts 2^(precision-1)
/// - first row (y==0): always uses Ra (left neighbor)
/// - first column (x==0): always uses Rb (above neighbor)
pub(crate) fn predict(
    curr_row: &[i32],
    prev_row: &[i32],
    x: usize,
    y: usize,
    predictor: u8,
    precision: u8,
) -> i32 {
    let ra = if x > 0 { curr_row[x - 1] } else { 0 };
    let rb = if y > 0 { prev_row[x] } else { 0 };
    let rc = if x > 0 && y > 0 { prev_row[x - 1] } else { 0 };

    if y == 0 && x == 0 {
        return 1i32 << (precision - 1);
    }
    if y == 0 {
        return ra;
    }
    if x == 0 {
        return rb;
    }

    match predictor {
        0 => 0,
        1 => ra,
        2 => rb,
        3 => rc,
        4 => ra + rb - rc,
        5 => ra + (rb - rc) / 2,
        6 => rb + (ra - rc) / 2,
        7 => (ra + rb) / 2,
        _ => ra,
    }
}

// ---------------------------------------------------------------------------
// Scan decode
// ---------------------------------------------------------------------------

/// Decode a full scan of JPEG Lossless data into a flat pixel buffer.
///
/// Returns a `Vec<u16>` of length `width * height`.
pub(crate) fn decode_scan<R: Read>(
    reader: R,
    ht: &HuffmanTable,
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
    point_transform: u8,
) -> io::Result<Vec<u16>> {
    let max_val: i32 = (1i32 << precision) - 1;
    let mut br = BitReader::new(reader);

    let mut pixels = vec![0u16; width * height];
    let mut prev_row = vec![0i32; width];
    let mut curr_row = vec![0i32; width];

    for y in 0..height {
        for x in 0..width {
            // Decode Huffman symbol (SSSS = number of additional bits)
            let ssss = decode_huffman(&mut br, ht)?;

            // Read additional bits and sign-extend
            let diff = if ssss > 0 {
                let bits = br.read_bits(ssss)?;
                extend(bits, ssss)
            } else {
                0
            };

            // Apply point transform
            let diff = if point_transform > 0 {
                diff << point_transform
            } else {
                diff
            };

            // Predict and reconstruct
            let pred = predict(&curr_row, &prev_row, x, y, predictor, precision);
            let val = (pred + diff) & max_val;

            curr_row[x] = val;
            pixels[y * width + x] = val as u16;
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
        curr_row.fill(0);
    }

    Ok(pixels)
}

// ---------------------------------------------------------------------------
// Scan encode
// ---------------------------------------------------------------------------

/// Encode a full scan of pixel data into JPEG Lossless format.
///
/// Writes the entropy-coded segment (no markers) to `writer`.
pub(crate) fn encode_scan<W: Write>(
    writer: W,
    ht: &HuffmanTable,
    pixels: &[u16],
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
) -> io::Result<()> {
    let max_val: i32 = (1i32 << precision) - 1;
    let mut bw = BitWriter::new(writer);

    let mut prev_row = vec![0i32; width];
    let mut curr_row = vec![0i32; width];

    for y in 0..height {
        for x in 0..width {
            let val = pixels[y * width + x] as i32;
            curr_row[x] = val;

            let pred = predict(&curr_row, &prev_row, x, y, predictor, precision);

            // Compute modular difference
            let mut diff = (val - pred) & max_val;
            if diff > max_val / 2 {
                diff -= max_val + 1;
            }

            let ssss = categorize(diff);

            // Write Huffman code for SSSS
            if let Some((code, size)) = ht.encode_symbol(ssss) {
                bw.write_bits(code as u32, size)?;
            }

            // Write additional bits
            if ssss > 0 {
                let additional = if diff < 0 {
                    (diff + (1 << ssss) - 1) as u32
                } else {
                    diff as u32
                };
                bw.write_bits(additional, ssss)?;
            }
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
        curr_row.fill(0);
    }

    bw.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Predictor tests
    // -----------------------------------------------------------------------

    #[test]
    fn predict_first_pixel() {
        let curr = vec![0i32; 4];
        let prev = vec![0i32; 4];
        // First pixel predicts 2^(p-1)
        assert_eq!(predict(&curr, &prev, 0, 0, 1, 8), 128);
        assert_eq!(predict(&curr, &prev, 0, 0, 1, 16), 32768);
    }

    #[test]
    fn predict_first_row_uses_left() {
        let curr = vec![100i32, 0, 0, 0];
        let prev = vec![0i32; 4];
        // Regardless of predictor, first row uses Ra (left)
        for predictor in 1..=7u8 {
            assert_eq!(predict(&curr, &prev, 1, 0, predictor, 8), 100);
        }
    }

    #[test]
    fn predict_first_col_uses_above() {
        let curr = vec![0i32; 4];
        let prev = vec![200i32, 0, 0, 0];
        // Regardless of predictor, first column uses Rb (above)
        for predictor in 1..=7u8 {
            assert_eq!(predict(&curr, &prev, 0, 1, predictor, 8), 200);
        }
    }

    #[test]
    fn predict_all_seven() {
        // Interior pixel: Ra=10, Rb=20, Rc=5
        let curr = vec![10i32, 0];
        let prev = vec![5i32, 20];
        let (x, y) = (1, 1);

        assert_eq!(predict(&curr, &prev, x, y, 1, 8), 10); // Ra
        assert_eq!(predict(&curr, &prev, x, y, 2, 8), 20); // Rb
        assert_eq!(predict(&curr, &prev, x, y, 3, 8), 5); // Rc
        assert_eq!(predict(&curr, &prev, x, y, 4, 8), 25); // Ra+Rb-Rc
        assert_eq!(predict(&curr, &prev, x, y, 5, 8), 17); // Ra+(Rb-Rc)/2 = 10+7 (int div)
        assert_eq!(predict(&curr, &prev, x, y, 6, 8), 22); // Rb+(Ra-Rc)/2 = 20+2 (int div)
        assert_eq!(predict(&curr, &prev, x, y, 7, 8), 15); // (Ra+Rb)/2
    }

    // -----------------------------------------------------------------------
    // BitWriter / BitReader roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn bit_writer_reader_roundtrip() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0b101, 3).unwrap();
            bw.write_bits(0b1100, 4).unwrap();
            bw.write_bits(0b1, 1).unwrap(); // byte boundary: 0b10111001 = 0xB9
            bw.write_bits(0xFF, 8).unwrap(); // should be byte-stuffed
            bw.flush().unwrap();
        }

        let mut br = BitReader::new(&buf[..]);
        assert_eq!(br.read_bits(3).unwrap(), 0b101);
        assert_eq!(br.read_bits(4).unwrap(), 0b1100);
        assert_eq!(br.read_bits(1).unwrap(), 0b1);
        assert_eq!(br.read_bits(8).unwrap(), 0xFF);
    }

    #[test]
    fn bit_writer_byte_stuffing() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0xFF, 8).unwrap();
            bw.flush().unwrap();
        }
        // Should contain 0xFF 0x00 (stuffed) then padding byte
        assert!(buf.len() >= 2);
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0x00);
    }

    // -----------------------------------------------------------------------
    // Scan encode/decode roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn scan_roundtrip_small_8bit() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![100, 102, 104, 103, 101, 105, 108, 107, 106];
        let (w, h) = (3, 3);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 8, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 8, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_16bit() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![1000, 1002, 1005, 1003, 1001, 1006, 1010, 1008, 1007];
        let (w, h) = (3, 3);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_all_predictors() {
        let ht = crate::huffman::build_default_table();
        // A 4x4 image with varying values to exercise all neighbors
        let pixels: Vec<u16> = vec![
            100, 105, 110, 108, 102, 107, 112, 109, 104, 108, 115, 111, 103, 106, 113, 110,
        ];
        let (w, h) = (4, 4);

        for predictor in 1..=7u8 {
            let mut encoded = Vec::new();
            encode_scan(&mut encoded, &ht, &pixels, w, h, 16, predictor).unwrap();

            let decoded = decode_scan(&encoded[..], &ht, w, h, 16, predictor, 0).unwrap();
            assert_eq!(
                decoded, pixels,
                "roundtrip failed for predictor {predictor}"
            );
        }
    }

    #[test]
    fn scan_roundtrip_constant_image() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![42; 16];
        let (w, h) = (4, 4);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_max_values() {
        let ht = crate::huffman::build_default_table();
        // 16-bit max values
        let pixels: Vec<u16> = vec![65535, 0, 65535, 0];
        let (w, h) = (2, 2);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_pixel() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![12345];
        let (w, h) = (1, 1);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_row() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = (0..100).collect();
        let (w, h) = (100, 1);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_column() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = (0..100).collect();
        let (w, h) = (1, 100);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0).unwrap();
        assert_eq!(decoded, pixels);
    }
}
