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
    /// The most recent marker byte (the second byte of an `0xFF Mn` pair) that
    /// [`BitReader::fill`] stopped at without consuming as entropy data.  `fill`
    /// never silently skips a marker; it records it here and stops.  Restart
    /// handling ([`BitReader::read_restart`]) inspects and clears this.
    pending_marker: Option<u8>,
}

impl<R: Read> BitReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: 0,
            bits: 0,
            eof: false,
            pending_marker: None,
        }
    }

    /// Fill the internal buffer up to at least 24 bits (or until EOF / marker).
    ///
    /// On encountering ANY marker (`0xFF` followed by a non-`0x00`, non-fill
    /// byte -- including the restart markers RST0..RST7) the fill stops, records
    /// the marker byte in [`BitReader::pending_marker`], and sets `eof` so the
    /// bit-consuming paths zero-pad the tail.  It never continues past a marker.
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
                                } else {
                                    // Any marker (RSTn, EOI, ...): stop reading
                                    // and record it for the restart machinery.
                                    self.pending_marker = Some(next);
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

    /// Consume a restart marker at an interval boundary (T.81 H.1.1 / B.2.1).
    ///
    /// Discards any buffered bits back to a byte boundary, then requires the
    /// next marker in the stream to be `RSTn` with `n == expected_index`
    /// (`0xD0 + expected_index`).  The marker may already have been seen by
    /// [`BitReader::fill`] (in `pending_marker`) or still be unread in the
    /// underlying reader.  On success the reader is reset so decoding resumes
    /// with the following entropy segment.
    fn read_restart(&mut self, expected_index: u8) -> io::Result<()> {
        // Drop the partial byte's padding bits: restarts are always byte-aligned.
        self.buf = 0;
        self.bits = 0;

        let marker = match self.pending_marker.take() {
            Some(m) => m,
            None => {
                // fill() never reached the marker; read it directly.  Expect
                // 0xFF (skipping any 0xFF fill bytes) then the marker byte.
                let mut b = [0u8; 1];
                if self.reader.read(&mut b)? == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "expected restart marker (H.1.1), got end of scan data",
                    ));
                }
                if b[0] != 0xFF {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "expected restart marker prefix 0xFF (H.1.1)",
                    ));
                }
                // Skip 0xFF fill bytes, then take the marker code.
                loop {
                    if self.reader.read(&mut b)? == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "truncated restart marker (H.1.1)",
                        ));
                    }
                    if b[0] != 0xFF {
                        break;
                    }
                }
                b[0]
            }
        };

        if marker != 0xD0 + expected_index {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "restart marker out of order (H.1.1): expected 0x{:02X}, got 0x{:02X}",
                    0xD0 + expected_index,
                    marker
                ),
            ));
        }

        // Resume: the next entropy segment follows immediately.
        self.eof = false;
        Ok(())
    }

    /// The marker byte `fill` last stopped at, if any (`0xFF Mn` -> `Mn`).
    fn pending_marker(&self) -> Option<u8> {
        self.pending_marker
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

    /// Pad the current partial byte with 1-bits to the next byte boundary and
    /// emit it (with byte stuffing), leaving the writer byte-aligned.
    ///
    /// T.81 B.1.1.5: entropy segments are padded with 1-bits.  A padded byte
    /// equal to `0xFF` is still followed by the `0x00` stuffing byte so a later
    /// marker cannot be misread out of the padding.  A no-op when already
    /// aligned, so it is safe to call before every restart marker.
    pub fn pad_and_align(&mut self) -> io::Result<()> {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            let padded = (self.buf << pad) | ((1u32 << pad) - 1);
            let byte_val = padded as u8;
            self.writer.write_all(&[byte_val])?;
            if byte_val == 0xFF {
                self.writer.write_all(&[0x00])?;
            }
            self.bits = 0;
            self.buf = 0;
        }
        Ok(())
    }

    /// Write a restart marker `RSTn` (`0xFF 0xD0+index`) at an interval
    /// boundary.  Pads/aligns first; the marker bytes are written raw (never
    /// byte-stuffed), matching [`BitReader::read_restart`].
    pub fn write_restart(&mut self, index: u8) -> io::Result<()> {
        self.pad_and_align()?;
        self.writer.write_all(&[0xFF, 0xD0 + index])?;
        Ok(())
    }

    /// Flush any remaining bits, padding with 1-bits to a byte boundary, and
    /// return the underlying writer.  Equivalent to [`BitWriter::pad_and_align`]
    /// followed by unwrapping the writer; preserves the end-of-scan bytes for
    /// streams without restarts.
    pub fn flush(mut self) -> io::Result<W> {
        self.pad_and_align()?;
        Ok(self.writer)
    }
}

// ---------------------------------------------------------------------------
// Huffman decode helper
// ---------------------------------------------------------------------------

/// Decode one Huffman symbol from the bit reader using `ht`.
pub(crate) fn decode_huffman<R: Read>(br: &mut BitReader<R>, ht: &HuffmanTable) -> io::Result<u8> {
    // Fast path: 8-bit lookup. This can resolve any code of length <= 8 even
    // when the buffer is short (peek zero-pads beyond EOF, and codes that
    // resolve within 8 bits are unaffected by the padding).
    let peek = br.peek_bits(8)? as u8;
    if let Some((size, value)) = ht.fast_lookup(peek) {
        br.consume_bits(size);
        return Ok(value);
    }

    // Slow path: decode bit by bit (codes 9..=16 bits, e.g. the SSSS=16 code).
    //
    // A JPEG encoder pads the final byte with 1-bits, which may not form a
    // valid Huffman code. Only when the buffer is genuinely exhausted at EOF
    // do we treat the remainder as padding and report SSSS=0 (no difference).
    let mut code: u16 = 0;
    for size in 1..=16u8 {
        if br.eof && br.bits == 0 {
            // Nothing left but end-of-stream: remaining bits (if any were
            // consumed) were padding.
            return Ok(0);
        }
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
/// - `x`: column coordinate
/// - `first_line`: whether this row is a "first line" for prediction -- true for
///   the top row of the image (y == 0) AND for the first row after a restart
///   marker (T.81 H.1.2.1: prediction restarts exactly as at scan start)
/// - `predictor`: predictor selection (1-7)
/// - `precision`: bits per sample
///
/// Special cases:
/// - first line, first column: predicts 2^(precision-1)
/// - first line: always uses Ra (left neighbor)
/// - first column: always uses Rb (above neighbor)
pub(crate) fn predict(
    curr_row: &[i32],
    prev_row: &[i32],
    x: usize,
    first_line: bool,
    predictor: u8,
    precision: u8,
) -> i32 {
    let ra = if x > 0 { curr_row[x - 1] } else { 0 };
    let rb = if !first_line { prev_row[x] } else { 0 };
    let rc = if x > 0 && !first_line {
        prev_row[x - 1]
    } else {
        0
    };

    if first_line && x == 0 {
        return 1i32 << (precision - 1);
    }
    if first_line {
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
/// `restart_interval` is the DRI value in MCUs (== samples, non-interleaved
/// single-component lossless); `0` disables restarts.  Per T.81 H.1.1 it must
/// be an integer multiple of `width` (validated by the caller), so restarts
/// always fall on row boundaries.
///
/// Returns a `Vec<u16>` of length `width * height`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_scan<R: Read>(
    reader: R,
    ht: &HuffmanTable,
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
    point_transform: u8,
    restart_interval: usize,
) -> io::Result<Vec<u16>> {
    // The codec operates entirely in the reduced domain P' = P - Pt.  The DPCM
    // loop reconstructs P'-bit samples; each is shifted left by Pt on store.
    let p_prime = precision - point_transform;
    let max_val: i32 = (1i32 << p_prime) - 1;
    let mut br = BitReader::new(reader);

    let mut pixels = vec![0u16; width * height];
    let mut prev_row = vec![0i32; width];
    let mut curr_row = vec![0i32; width];
    let mut restart_index: u8 = 0;

    for y in 0..height {
        // Row-aligned restart boundary (H.1.1): after every `restart_interval`
        // samples.  With `restart_interval % width == 0` this is `y` a nonzero
        // multiple of `restart_interval / width`.
        let restart_boundary = restart_interval > 0 && (y * width) % restart_interval == 0;
        if restart_boundary && y > 0 {
            br.read_restart(restart_index)?;
            restart_index = (restart_index + 1) % 8;
        }
        // H.1.2.1: prediction restarts as at scan start, so the boundary row is
        // a "first line" (as is the true top row).
        let first_line = y == 0 || restart_boundary;

        for x in 0..width {
            // Decode Huffman symbol (SSSS = number of additional bits)
            let ssss = decode_huffman(&mut br, ht)?;

            // T.81 permits SSSS only in 0..=16; a corrupted DHT can map a
            // valid code to a larger symbol, which must be rejected before
            // read_bits (a count > 24 is unrepresentable in the bit buffer).
            if ssss > 16 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("SSSS category {ssss} out of range (T.81 allows 0..=16)"),
                ));
            }

            // Read additional bits and sign-extend.  SSSS==16 is the T.81
            // H.1.2.2 / Table H.2 special case: the modular difference 32768
            // carries NO appended bits (it can only occur at P'==16).
            let diff = if ssss == 16 {
                32768
            } else if ssss > 0 {
                let bits = br.read_bits(ssss)?;
                extend(bits, ssss)
            } else {
                0
            };

            // Predict and reconstruct in the reduced domain.
            let pred = predict(&curr_row, &prev_row, x, first_line, predictor, p_prime);
            let val = (pred + diff) & max_val;

            curr_row[x] = val;
            // Store back at full precision by re-applying the point transform.
            pixels[y * width + x] = (val << point_transform) as u16;
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
        curr_row.fill(0);
    }

    // Without a DRI a restart marker is illegal (H.1.1): if the entropy reader
    // stopped on an RSTn while none was expected, reject the stream.
    if restart_interval == 0 {
        if let Some(m) = br.pending_marker() {
            if (0xD0..=0xD7).contains(&m) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected restart marker without DRI (H.1.1)",
                ));
            }
        }
    }

    Ok(pixels)
}

// ---------------------------------------------------------------------------
// Scan encode
// ---------------------------------------------------------------------------

/// Encode a full scan of pixel data into JPEG Lossless format.
///
/// Writes the entropy-coded segment (no markers) to `writer`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_scan<W: Write>(
    writer: W,
    ht: &HuffmanTable,
    pixels: &[u16],
    width: usize,
    height: usize,
    precision: u8,
    predictor: u8,
    point_transform: u8,
    restart_interval_rows: usize,
) -> io::Result<()> {
    // Operate entirely in the reduced domain P' = P - Pt: every input sample is
    // shifted right by Pt before entering the DPCM loop.
    let p_prime = precision - point_transform;
    let max_val: i32 = (1i32 << p_prime) - 1;
    let mut bw = BitWriter::new(writer);

    let mut prev_row = vec![0i32; width];
    let mut curr_row = vec![0i32; width];
    let mut restart_index: u8 = 0;

    for y in 0..height {
        // Row-aligned restart boundary (H.1.1): emit RSTn before every
        // `restart_interval_rows` rows, but never after the final row.
        let restart_boundary = restart_interval_rows > 0 && y % restart_interval_rows == 0;
        if restart_boundary && y > 0 {
            bw.write_restart(restart_index)?;
            restart_index = (restart_index + 1) % 8;
        }
        // H.1.2.1: the boundary row (and the true top row) restarts prediction.
        let first_line = y == 0 || restart_boundary;

        for x in 0..width {
            let val = (pixels[y * width + x] >> point_transform) as i32;
            curr_row[x] = val;

            let pred = predict(&curr_row, &prev_row, x, first_line, predictor, p_prime);

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

            // Write additional bits.  SSSS==16 is the T.81 H.1.2.2 / Table H.2
            // special case for the modular difference 32768: NO appended bits.
            if ssss > 0 && ssss < 16 {
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
        assert_eq!(predict(&curr, &prev, 0, true, 1, 8), 128);
        assert_eq!(predict(&curr, &prev, 0, true, 1, 16), 32768);
    }

    #[test]
    fn predict_first_row_uses_left() {
        let curr = vec![100i32, 0, 0, 0];
        let prev = vec![0i32; 4];
        // Regardless of predictor, first row uses Ra (left)
        for predictor in 1..=7u8 {
            assert_eq!(predict(&curr, &prev, 1, true, predictor, 8), 100);
        }
    }

    #[test]
    fn predict_first_col_uses_above() {
        let curr = vec![0i32; 4];
        let prev = vec![200i32, 0, 0, 0];
        // Regardless of predictor, first column uses Rb (above)
        for predictor in 1..=7u8 {
            assert_eq!(predict(&curr, &prev, 0, false, predictor, 8), 200);
        }
    }

    #[test]
    fn predict_all_seven() {
        // Interior pixel: Ra=10, Rb=20, Rc=5
        let curr = vec![10i32, 0];
        let prev = vec![5i32, 20];
        let x = 1;
        let first_line = false;

        assert_eq!(predict(&curr, &prev, x, first_line, 1, 8), 10); // Ra
        assert_eq!(predict(&curr, &prev, x, first_line, 2, 8), 20); // Rb
        assert_eq!(predict(&curr, &prev, x, first_line, 3, 8), 5); // Rc
        assert_eq!(predict(&curr, &prev, x, first_line, 4, 8), 25); // Ra+Rb-Rc
        assert_eq!(predict(&curr, &prev, x, first_line, 5, 8), 17); // Ra+(Rb-Rc)/2 = 10+7 (int div)
        assert_eq!(predict(&curr, &prev, x, first_line, 6, 8), 22); // Rb+(Ra-Rc)/2 = 20+2 (int div)
        assert_eq!(predict(&curr, &prev, x, first_line, 7, 8), 15); // (Ra+Rb)/2
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
        encode_scan(&mut encoded, &ht, &pixels, w, h, 8, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 8, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_16bit() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![1000, 1002, 1005, 1003, 1001, 1006, 1010, 1008, 1007];
        let (w, h) = (3, 3);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
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
            encode_scan(&mut encoded, &ht, &pixels, w, h, 16, predictor, 0, 0).unwrap();

            let decoded = decode_scan(&encoded[..], &ht, w, h, 16, predictor, 0, 0).unwrap();
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
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_max_values() {
        let ht = crate::huffman::build_default_table();
        // 16-bit max values
        let pixels: Vec<u16> = vec![65535, 0, 65535, 0];
        let (w, h) = (2, 2);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_pixel() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![12345];
        let (w, h) = (1, 1);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_row() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = (0..100).collect();
        let (w, h) = (100, 1);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_roundtrip_single_column() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = (0..100).collect();
        let (w, h) = (1, 100);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    // -----------------------------------------------------------------------
    // SSSS = 16 special case (T.81 H.1.2.2 / Table H.2): the modular
    // difference 32768 is coded as category 16 with NO appended bits.
    // -----------------------------------------------------------------------

    #[test]
    fn ssss16_emits_no_appended_bits_exact_vector() {
        // A 1x1, 16-bit image whose only sample is 0.  The first-pixel
        // prediction is 2^15 = 32768, so DIFF = (0 - 32768) mod 65536 = 32768,
        // mapped to the signed value -32768 => category 16.
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![0];

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, 1, 1, 16, 1, 0, 0).unwrap();

        // The default table's category-16 code is 14 bits (0b11111111111110).
        // With no appended bits, flushing pads the trailing 6 bits with 1s.
        // The first byte is 0xFF, which the byte-stuffer follows with 0x00,
        // then the padded final byte 0xFB.  Had the (buggy) 16 appended bits
        // been written, the payload would have been 30 bits => 4 bytes.
        assert_eq!(
            encoded,
            vec![0xFF, 0x00, 0xFB],
            "category-16 code must carry no appended bits"
        );

        // And it must decode back to the original sample.
        let decoded = decode_scan(&encoded[..], &ht, 1, 1, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn ssss16_roundtrip_boundary_values() {
        // Values that force the -32768 modular difference at various positions.
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![0, 32768, 65535, 32767, 0, 32768];
        let (w, h) = (3, 2);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 16, 1, 0, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 16, 1, 0, 0).unwrap();
        assert_eq!(decoded, pixels);
    }

    // -----------------------------------------------------------------------
    // Point transform: reduced-domain (P' = P - Pt) round-trips.
    // -----------------------------------------------------------------------

    #[test]
    fn scan_point_transform_reduced_domain() {
        let ht = crate::huffman::build_default_table();
        // 12-bit-ish values; Pt=2 keeps only the top 10 bits (P' = 10).
        let pixels: Vec<u16> = vec![0, 4, 8, 12, 2044, 4092, 100, 2048, 4000];
        let (w, h) = (3, 3);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 12, 1, 2, 0).unwrap();

        let decoded = decode_scan(&encoded[..], &ht, w, h, 12, 1, 2, 0).unwrap();
        // Decoded values equal (orig >> 2) << 2.
        let expected: Vec<u16> = pixels.iter().map(|&p| (p >> 2) << 2).collect();
        assert_eq!(decoded, expected);
    }

    // -----------------------------------------------------------------------
    // Restart intervals (J4): low-level encode_scan / decode_scan symmetry.
    // -----------------------------------------------------------------------

    #[test]
    fn scan_restart_roundtrip_and_reset() {
        let ht = crate::huffman::build_default_table();
        // 2 columns, 4 rows; predictor 2 (Rb, above) makes vertical prediction
        // load-bearing, so a failed restart reset would corrupt the output.
        let pixels: Vec<u16> = vec![10, 250, 240, 5, 128, 200, 7, 190];
        let (w, h) = (2usize, 4usize);

        for rows in 1..=h {
            let mut encoded = Vec::new();
            encode_scan(&mut encoded, &ht, &pixels, w, h, 8, 2, 0, rows).unwrap();
            let decoded = decode_scan(&encoded[..], &ht, w, h, 8, 2, 0, w * rows).unwrap();
            assert_eq!(decoded, pixels, "restart roundtrip failed for rows={rows}");
        }
    }

    #[test]
    fn scan_restart_first_sample_predicts_half_range() {
        // A 1x2 image with restart after row 0.  The restart row (row 1) is a
        // "first line": its first (only) sample is predicted 2^(P'-1)=128, NOT
        // from row 0.  Verify by decoding the restart-encoded stream (reset) and
        // confirming the reconstruction of an arbitrary row-1 value is exact.
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![250, 3]; // strong jump across the restart
        let (w, h) = (1usize, 2usize);

        let mut encoded = Vec::new();
        encode_scan(&mut encoded, &ht, &pixels, w, h, 8, 1, 0, 1).unwrap();
        // A RST0 marker must separate the two single-sample rows.
        assert!(
            encoded.windows(2).any(|x| x[0] == 0xFF && x[1] == 0xD0),
            "RST0 marker expected between rows"
        );
        let decoded = decode_scan(&encoded[..], &ht, w, h, 8, 1, 0, 1).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn scan_point_transform_property_all_predictors() {
        let ht = crate::huffman::build_default_table();
        let pixels: Vec<u16> = vec![
            1000, 1005, 1010, 1008, 1002, 1007, 1012, 1009, 1004, 1008, 1015, 1011, 1003, 1006,
            1013, 1010,
        ];
        let (w, h) = (4, 4);
        for predictor in 1..=7u8 {
            for &pt in &[0u8, 2u8] {
                let mut encoded = Vec::new();
                encode_scan(&mut encoded, &ht, &pixels, w, h, 12, predictor, pt, 0).unwrap();
                let decoded = decode_scan(&encoded[..], &ht, w, h, 12, predictor, pt, 0).unwrap();
                let expected: Vec<u16> = pixels.iter().map(|&p| (p >> pt) << pt).collect();
                assert_eq!(
                    decoded, expected,
                    "point-transform property failed for predictor {predictor}, pt {pt}"
                );
            }
        }
    }
}
