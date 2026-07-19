//! Bit-level reader and writer for T.87 (ITU-T T.87 / ISO 14495-1) JPEG-LS
//! entropy-coded segments.
//!
//! # Single-bit stuffing (T.87 A.1)
//!
//! Whenever an output byte equals `0xFF`, the *next* byte carries only 7 data
//! bits and its most-significant bit is forced to `0` (the "stuffed" bit).
//! On read, a byte `< 0x80` following a `0xFF` contributes 7 data bits (its
//! MSB is the stuffed zero); a byte `>= 0x80` following `0xFF` is a **marker**
//! and is surfaced as a structured signal ([`BitReader::pending_marker`] plus
//! [`CodecError::Marker`]) rather than a string error.
//!
//! # Limited-length Golomb (T.87 A.5.3)
//!
//! [`BitWriter::write_limited_golomb`] / [`BitReader::read_limited_golomb`]
//! implement the length-limited mapped-error code, replacing the pre-spec
//! uncapped unary coder (which lives on, frozen, in `legacy.rs`).

use std::io::{self, Write};

use crate::error::CodecError;

// ---------------------------------------------------------------------------
// BitReader
// ---------------------------------------------------------------------------

/// Reads bits from a byte slice, handling T.87 single-bit stuffing.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bit accumulator (MSB-first, valid bits are the low `n_bits`).
    bits: u64,
    /// Number of valid bits in `bits`.
    n_bits: i32,
    /// Whether the most recently consumed byte was `0xFF` (so the next byte is
    /// a stuffed 7-bit byte or a marker).
    last_ff: bool,
    /// Set when a marker (0xFFxx, xx >= 0x80) was encountered mid-scan.
    pending_marker: Option<u16>,
}

impl<'a> BitReader<'a> {
    /// Create a new `BitReader` over the given byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bits: 0,
            n_bits: 0,
            last_ff: false,
            pending_marker: None,
        }
    }

    /// If a marker terminated the entropy-coded segment, return it.
    ///
    /// The value is the full 16-bit marker code (e.g. `0xFFD9` for EOI).
    pub fn pending_marker(&self) -> Option<u16> {
        self.pending_marker
    }

    /// Fill the accumulator so it contains at least `n` bits.
    fn fill(&mut self, n: i32) -> Result<(), CodecError> {
        while self.n_bits < n {
            if let Some(m) = self.pending_marker {
                return Err(CodecError::Marker(m));
            }
            if self.pos >= self.data.len() {
                return Err(CodecError::InvalidData(
                    "unexpected end of JPEG-LS data".into(),
                ));
            }
            let b = self.data[self.pos];

            if self.last_ff {
                // The previous byte was a `0xFF` data byte. By construction (we
                // only treat `0xFF` as data when the following byte is < 0x80)
                // this byte is a stuffed 7-bit byte: its MSB is the stuffed 0.
                self.pos += 1;
                self.bits = (self.bits << 7) | (u64::from(b) & 0x7F);
                self.n_bits += 7;
                self.last_ff = false; // b < 0x80, cannot be 0xFF
            } else if b == 0xFF {
                // Look ahead: a following byte >= 0x80 makes `0xFF` the first
                // half of a marker (0xFF is NOT data); a following byte < 0x80
                // makes `0xFF` an 8-bit data byte with a stuffed byte after it.
                match self.data.get(self.pos + 1).copied() {
                    Some(next) if next >= 0x80 => {
                        let marker = 0xFF00u16 | u16::from(next);
                        self.pending_marker = Some(marker);
                        return Err(CodecError::Marker(marker));
                    }
                    _ => {
                        self.pos += 1;
                        self.bits = (self.bits << 8) | 0xFF;
                        self.n_bits += 8;
                        self.last_ff = true;
                    }
                }
            } else {
                self.pos += 1;
                self.bits = (self.bits << 8) | u64::from(b);
                self.n_bits += 8;
                self.last_ff = false;
            }
        }
        Ok(())
    }

    /// Read `n` bits (0..=32) and return them right-justified.
    pub fn read_bits(&mut self, n: i32) -> Result<u32, CodecError> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n)?;
        let shift = self.n_bits - n;
        let mask = (1u64 << n) - 1;
        let val = (self.bits >> shift) & mask;
        self.n_bits -= n;
        Ok(val as u32)
    }

    /// Read a single bit.
    #[inline]
    pub fn read_bit(&mut self) -> Result<u32, CodecError> {
        self.read_bits(1)
    }

    /// Read a length-limited Golomb code (T.87 A.5.3).
    ///
    /// Counts the zeros preceding the terminating `1`:
    /// - `count < glimit - qbpp - 1` → `(count << k) | read_bits(k)`;
    /// - `count == glimit - qbpp - 1` (escape) → `read_bits(qbpp) + 1`;
    /// - anything longer → [`CodecError::InvalidData`].
    pub fn read_limited_golomb(
        &mut self,
        k: i32,
        glimit: i32,
        qbpp: i32,
    ) -> Result<u32, CodecError> {
        let escape = glimit - qbpp - 1;
        let mut count: i32 = 0;
        while self.read_bit()? == 0 {
            count += 1;
            if count > escape {
                return Err(CodecError::InvalidData(
                    "limited-Golomb unary run exceeds escape length".into(),
                ));
            }
        }

        if count < escape {
            let r = if k > 0 { self.read_bits(k)? } else { 0 };
            Ok(((count as u32) << k) | r)
        } else {
            // count == escape: the escape carries (val - 1) in qbpp bits.
            Ok(self.read_bits(qbpp)?.wrapping_add(1))
        }
    }
}

// ---------------------------------------------------------------------------
// BitWriter
// ---------------------------------------------------------------------------

/// Writes bits to an underlying `Write` sink, applying T.87 single-bit
/// stuffing.
pub(crate) struct BitWriter<W: Write> {
    inner: W,
    /// Bit accumulator (MSB-first, valid bits are the low `n_bits`).
    bits: u64,
    /// Number of valid bits in `bits`.
    n_bits: i32,
    /// Whether the most recently emitted byte was `0xFF` (so the next byte
    /// must carry only 7 data bits with its MSB forced to 0).
    last_ff: bool,
}

impl<W: Write> BitWriter<W> {
    /// Create a new `BitWriter` wrapping the given writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            bits: 0,
            n_bits: 0,
            last_ff: false,
        }
    }

    /// Emit whole output bytes while enough bits are buffered. After a `0xFF`
    /// byte the next byte carries only 7 bits (MSB forced 0) per T.87 A.1.
    fn drain(&mut self) -> Result<(), CodecError> {
        loop {
            let width = if self.last_ff { 7 } else { 8 };
            if self.n_bits < width {
                break;
            }
            let shift = self.n_bits - width;
            let mask = (1u64 << width) - 1;
            let b = ((self.bits >> shift) & mask) as u8;
            self.inner.write_all(&[b])?;
            self.n_bits -= width;
            self.last_ff = b == 0xFF;
        }
        Ok(())
    }

    /// Write `n` bits (0..=32) from `val` (MSB-first).
    pub fn write_bits(&mut self, val: u32, n: i32) -> Result<(), CodecError> {
        if n == 0 {
            return Ok(());
        }
        self.bits = (self.bits << n) | (u64::from(val) & ((1u64 << n) - 1));
        self.n_bits += n;
        self.drain()
    }

    /// Write a single bit.
    #[inline]
    pub fn write_bit(&mut self, bit: u32) -> Result<(), CodecError> {
        self.write_bits(bit, 1)
    }

    /// Write a length-limited Golomb code (T.87 A.5.3).
    ///
    /// With `q = val >> k` and `escape = glimit - qbpp - 1`:
    /// - `q < escape` → `q` zeros, a `1`, then the low `k` bits of `val`;
    /// - otherwise → `escape` zeros, a `1`, then `val - 1` in `qbpp` bits.
    pub fn write_limited_golomb(
        &mut self,
        k: i32,
        val: u32,
        glimit: i32,
        qbpp: i32,
    ) -> Result<(), CodecError> {
        let q = (val >> k) as i32;
        let escape = glimit - qbpp - 1;

        if q < escape {
            for _ in 0..q {
                self.write_bit(0)?;
            }
            self.write_bit(1)?;
            if k > 0 {
                self.write_bits(val & ((1u32 << k) - 1), k)?;
            }
        } else {
            for _ in 0..escape {
                self.write_bit(0)?;
            }
            self.write_bit(1)?;
            self.write_bits(val.wrapping_sub(1), qbpp)?;
        }
        Ok(())
    }

    /// Flush remaining bits (zero-padded to a byte boundary) and the
    /// underlying writer.
    ///
    /// Padding respects the stuffing state: if the previous emitted byte was
    /// `0xFF`, the final byte carries 7 bits (MSB forced 0); otherwise 8. The
    /// zero padding can never itself produce a `0xFF`, so no trailing stuff
    /// byte is required (T.87 A.1).
    pub fn flush(&mut self) -> Result<(), CodecError> {
        if self.n_bits > 0 {
            let width = if self.last_ff { 7 } else { 8 };
            debug_assert!(self.n_bits < width, "drain should leave < width bits");
            let pad = width - self.n_bits;
            self.bits <<= pad;
            self.n_bits += pad;
            let mask = (1u64 << width) - 1;
            let b = (self.bits & mask) as u8;
            self.inner.write_all(&[b])?;
            self.bits = 0;
            self.n_bits = 0;
            self.last_ff = b == 0xFF;
        }
        self.inner.flush()?;
        Ok(())
    }

    /// Write a raw byte directly (used for markers, not bit-coded data).
    ///
    /// Must be byte-aligned; markers are exempt from stuffing.
    pub fn write_byte(&mut self, b: u8) -> Result<(), CodecError> {
        debug_assert_eq!(self.n_bits, 0, "write_byte called mid-byte");
        self.inner.write_all(&[b]).map_err(CodecError::from)
    }

    /// Write a big-endian 16-bit word directly (marker payloads).
    pub fn write_u16be(&mut self, v: u16) -> Result<(), CodecError> {
        debug_assert_eq!(self.n_bits, 0, "write_u16be called mid-byte");
        self.inner
            .write_all(&v.to_be_bytes())
            .map_err(CodecError::from)
    }

    /// Borrow the inner writer.
    pub fn inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Consume this writer and return the underlying writer.
    pub fn into_inner(self) -> W {
        self.inner
    }

    /// Write raw bytes. Must only be called when byte-aligned.
    pub fn write_bytes(&mut self, buf: &[u8]) -> io::Result<()> {
        debug_assert_eq!(
            self.n_bits, 0,
            "write_bytes called while bit buffer is non-empty"
        );
        self.inner.write_all(buf)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bytes_via_bits(values: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut bw = BitWriter::new(&mut buf);
        for &v in values {
            bw.write_bits(u32::from(v), 8).unwrap();
        }
        bw.flush().unwrap();
        buf
    }

    #[test]
    fn plain_bits_roundtrip() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0b101, 3).unwrap();
            bw.write_bits(0b1100, 4).unwrap();
            bw.write_bits(0b1, 1).unwrap();
            bw.flush().unwrap();
        }
        assert_eq!(buf, vec![0xB9]);

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(3).unwrap(), 0b101);
        assert_eq!(br.read_bits(4).unwrap(), 0b1100);
        assert_eq!(br.read_bits(1).unwrap(), 0b1);
    }

    #[test]
    fn single_bit_stuffing_after_ff() {
        // A single 0xFF data byte followed by more bits. Under T.87 single-bit
        // stuffing the byte after 0xFF carries only 7 data bits (MSB = 0), so
        // the encoded stream is NOT `FF 00` (that is the legacy behavior).
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0xFF, 8).unwrap();
            bw.write_bits(0b1, 1).unwrap();
            bw.flush().unwrap();
        }
        // 0xFF, then a stuffed byte: MSB 0, next bit 1, padded => 0b0100_0000.
        assert_eq!(buf, vec![0xFF, 0x40]);
        // Every byte after a 0xFF is < 0x80.
        for w in buf.windows(2) {
            if w[0] == 0xFF {
                assert!(w[1] < 0x80, "byte after 0xFF must be stuffed (< 0x80)");
            }
        }
    }

    #[test]
    fn stuffing_roundtrip_many_ff_boundaries() {
        // Patterns that force runs of 0xFF data bytes and 0xFF-adjacent bits.
        let patterns: &[&[u8]] = &[
            &[0xFF],
            &[0xFF, 0xFF, 0xFF, 0xFF],
            &[0xFF, 0x00, 0xFF, 0x00],
            &[0x00, 0xFF, 0xFF, 0x01, 0xFF, 0xFF, 0xFF, 0x80, 0x7F],
            &[0xFF; 16],
            &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x12, 0x34],
        ];
        for pat in patterns {
            let encoded = write_bytes_via_bits(pat);
            // Invariant: no byte following 0xFF is >= 0x80 (no accidental
            // markers), i.e. the stuffing held across every boundary.
            for w in encoded.windows(2) {
                if w[0] == 0xFF {
                    assert!(w[1] < 0x80, "pattern {pat:?} produced FF {:02X}", w[1]);
                }
            }
            let mut br = BitReader::new(&encoded);
            for &v in pat.iter() {
                assert_eq!(br.read_bits(8).unwrap(), u32::from(v), "pattern {pat:?}");
            }
        }
    }

    #[test]
    fn limited_golomb_roundtrip_all_k_non_escape() {
        let glimit = 32;
        let qbpp = 16;
        let escape = glimit - qbpp - 1; // = 15
        for k in 0..=15 {
            let kbits = 1u32 << k;
            // Build values whose quotient q = val >> k stays strictly below the
            // escape so the non-escape branch is exercised for every k.
            let mut values: Vec<u32> = Vec::new();
            for q in [0u32, 1, 7, (escape as u32) - 1] {
                for r in [0u32, kbits / 2, kbits - 1] {
                    values.push((q << k) | (r & (kbits - 1)));
                }
            }
            let mut buf = Vec::new();
            {
                let mut bw = BitWriter::new(&mut buf);
                for &v in &values {
                    assert!((v >> k) < escape as u32);
                    bw.write_limited_golomb(k, v, glimit, qbpp).unwrap();
                }
                bw.flush().unwrap();
            }
            let mut br = BitReader::new(&buf);
            for &v in &values {
                let got = br.read_limited_golomb(k, glimit, qbpp).unwrap();
                assert_eq!(got, v, "k={k}, v={v}");
            }
        }
    }

    #[test]
    fn limited_golomb_escape_path() {
        let glimit = 32;
        let qbpp = 16;
        // For k=0 the escape triggers at q = val >= glimit - qbpp - 1 = 15.
        let escape = glimit - qbpp - 1;
        let values: Vec<u32> = vec![15, 16, 100, 1000, 65535, (1 << qbpp)];
        for k in 0..=15 {
            let mut buf = Vec::new();
            {
                let mut bw = BitWriter::new(&mut buf);
                for &v in &values {
                    // Confirm at least some of these take the escape branch.
                    bw.write_limited_golomb(k, v, glimit, qbpp).unwrap();
                }
                bw.flush().unwrap();
            }
            let mut br = BitReader::new(&buf);
            for &v in &values {
                let got = br.read_limited_golomb(k, glimit, qbpp).unwrap();
                assert_eq!(got, v, "escape k={k}, v={v}");
            }
        }
        assert!(escape > 0);
    }

    #[test]
    fn marker_detection_mid_stream() {
        // 0xFF followed by 0xD9 (EOI) is a marker, surfaced structurally.
        let data = [0xFFu8, 0xD9];
        let mut br = BitReader::new(&data);
        // Reading any bit must fail with a structured marker signal.
        let err = br.read_bit().unwrap_err();
        assert!(matches!(err, CodecError::Marker(0xFFD9)));
        assert_eq!(br.pending_marker(), Some(0xFFD9));
        // Subsequent reads keep reporting the marker, not garbage.
        assert!(matches!(
            br.read_bit().unwrap_err(),
            CodecError::Marker(0xFFD9)
        ));
    }

    #[test]
    fn marker_after_valid_data() {
        // Some real bits, then a 0xFF-prefixed marker.
        let data = [0b1010_0000u8, 0xFF, 0xD9];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits(3).unwrap(), 0b101);
        // Continue reading until we cross into the marker.
        let mut hit_marker = false;
        for _ in 0..16 {
            match br.read_bit() {
                Ok(_) => {}
                Err(CodecError::Marker(0xFFD9)) => {
                    hit_marker = true;
                    break;
                }
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
        assert!(hit_marker, "expected to reach the EOI marker");
    }

    #[test]
    fn truncation_is_error_not_panic() {
        let data = [0b1010_1010u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits(8).unwrap(), 0b1010_1010);
        // Nothing left: further reads error cleanly.
        assert!(matches!(br.read_bit(), Err(CodecError::InvalidData(_))));
    }

    #[test]
    fn read_zero_bits_returns_zero() {
        let buf = [0xAB];
        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(0).unwrap(), 0);
    }

    #[test]
    fn write_zero_bits_is_noop() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0, 0).unwrap();
            bw.flush().unwrap();
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn limited_golomb_rejects_overlong_unary() {
        // A stream of all-zero bits never terminates the unary run within the
        // escape budget -> InvalidData, not an infinite loop or panic.
        let data = [0x00u8; 8];
        let mut br = BitReader::new(&data);
        assert!(matches!(
            br.read_limited_golomb(0, 32, 16),
            Err(CodecError::InvalidData(_))
        ));
    }
}
