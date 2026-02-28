//! Bit-level reader and writer for JPEG-LS Golomb-Rice coded bitstreams.
//!
//! Handles 0xFF byte-stuffing as required by the JPEG-LS standard:
//! - On write: after emitting a 0xFF byte, a 0x00 stuff byte is inserted.
//! - On read: 0xFF followed by 0x00 is consumed as a single 0xFF data byte;
//!   0xFF followed by anything else signals a marker.

use std::io::{self, Write};

use crate::error::CodecError;

// ---------------------------------------------------------------------------
// BitReader
// ---------------------------------------------------------------------------

/// Reads bits from a byte slice, handling JPEG byte-stuffing.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bit accumulator (MSB-first).
    bits: u64,
    /// Number of valid bits in `bits`.
    n_bits: i32,
}

impl<'a> BitReader<'a> {
    /// Create a new `BitReader` over the given byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bits: 0,
            n_bits: 0,
        }
    }

    /// Fill the accumulator so it contains at least `n` bits.
    fn fill(&mut self, n: i32) -> Result<(), CodecError> {
        while self.n_bits < n {
            if self.pos >= self.data.len() {
                return Err(CodecError::InvalidData(
                    "unexpected end of JPEG-LS data".into(),
                ));
            }
            let b = self.data[self.pos];
            self.pos += 1;

            if b == 0xFF {
                // Peek at the next byte.
                if self.pos >= self.data.len() {
                    return Err(CodecError::InvalidData(
                        "unexpected end of data after 0xFF".into(),
                    ));
                }
                let next = self.data[self.pos];
                if next == 0x00 {
                    // Byte stuffing -- consume the 0x00 and treat as 0xFF data.
                    self.pos += 1;
                } else {
                    // This is a marker (e.g. EOI). Stop reading.
                    // Back up so the marker can be parsed later.
                    self.pos -= 1;
                    return Err(CodecError::InvalidData("marker encountered".into()));
                }
            }

            self.bits = (self.bits << 8) | u64::from(b);
            self.n_bits += 8;
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

    /// Read a Golomb-Rice code with parameter `k`.
    ///
    /// The format is: unary-coded quotient (zeros followed by a 1-bit),
    /// then `k` bits for the remainder.
    pub fn read_golomb(&mut self, k: i32) -> Result<u32, CodecError> {
        // Count leading zeros (the quotient).
        let mut q: u32 = 0;
        loop {
            let b = self.read_bit()?;
            if b == 1 {
                break;
            }
            q += 1;
            if q > 65536 {
                return Err(CodecError::InvalidData("golomb q overflow".into()));
            }
        }

        if k == 0 {
            return Ok(q);
        }

        let r = self.read_bits(k)?;
        Ok(q.wrapping_shl(k as u32) | r)
    }
}

// ---------------------------------------------------------------------------
// BitWriter
// ---------------------------------------------------------------------------

/// Writes bits to an underlying `Write` sink, handling JPEG byte-stuffing.
pub(crate) struct BitWriter<W: Write> {
    inner: W,
    /// Bit accumulator (MSB-first).
    bits: u64,
    /// Number of valid bits in `bits`.
    n_bits: i32,
}

impl<W: Write> BitWriter<W> {
    /// Create a new `BitWriter` wrapping the given writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            bits: 0,
            n_bits: 0,
        }
    }

    /// Write `n` bits from `val` (MSB-first).
    pub fn write_bits(&mut self, val: u32, n: i32) -> Result<(), CodecError> {
        self.bits = (self.bits << n) | (u64::from(val) & ((1u64 << n) - 1));
        self.n_bits += n;

        while self.n_bits >= 8 {
            let shift = self.n_bits - 8;
            let b = (self.bits >> shift) as u8;
            self.inner.write_all(&[b])?;

            // Byte stuffing: after 0xFF, insert 0x00.
            if b == 0xFF {
                self.inner.write_all(&[0x00])?;
            }

            self.n_bits -= 8;
        }
        Ok(())
    }

    /// Write a single bit.
    #[inline]
    pub fn write_bit(&mut self, bit: u32) -> Result<(), CodecError> {
        self.write_bits(bit, 1)
    }

    /// Flush remaining bits (zero-padded to byte boundary) and the
    /// underlying writer.
    pub fn flush(&mut self) -> Result<(), CodecError> {
        if self.n_bits > 0 {
            let shift = 8 - self.n_bits;
            let b = (self.bits << shift) as u8;
            self.inner.write_all(&[b])?;
            if b == 0xFF {
                self.inner.write_all(&[0x00])?;
            }
            self.n_bits = 0;
            self.bits = 0;
        }
        self.inner.flush()?;
        Ok(())
    }

    /// Write a Golomb-Rice code for the non-negative mapped value `val`.
    ///
    /// Format: unary quotient (q zeros + one 1-bit), then k remainder bits.
    pub fn write_golomb(&mut self, k: i32, val: u32) -> Result<(), CodecError> {
        let q = val >> k;
        let r = val & ((1u32 << k) - 1);

        // Unary: q zeros then a 1.
        for _ in 0..q {
            self.write_bit(0)?;
        }
        self.write_bit(1)?;

        // Remainder.
        if k > 0 {
            self.write_bits(r, k)?;
        }
        Ok(())
    }

    /// Write a raw byte directly (used for markers, not bit-coded data).
    pub fn write_byte(&mut self, b: u8) -> Result<(), CodecError> {
        self.inner.write_all(&[b]).map_err(CodecError::from)
    }

    /// Write a big-endian 16-bit word directly.
    pub fn write_u16be(&mut self, v: u16) -> Result<(), CodecError> {
        self.inner
            .write_all(&v.to_be_bytes())
            .map_err(CodecError::from)
    }

    /// Borrow the inner writer (e.g. for writing marker bytes directly).
    pub fn inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Consume this writer and return the underlying writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

// Convenience: allow using `io::Write` on the inner writer through the
// BitWriter when we know we are byte-aligned (marker writing).
impl<W: Write> BitWriter<W> {
    /// Write raw bytes. Must only be called when the bit buffer is empty
    /// (byte-aligned).
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

    #[test]
    fn roundtrip_bits() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0b101, 3).unwrap();
            bw.write_bits(0b1100, 4).unwrap();
            bw.write_bits(0b1, 1).unwrap();
            bw.flush().unwrap();
        }
        // 10111001 = 0xB9
        assert_eq!(buf, vec![0xB9]);

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(3).unwrap(), 0b101);
        assert_eq!(br.read_bits(4).unwrap(), 0b1100);
        assert_eq!(br.read_bits(1).unwrap(), 0b1);
    }

    #[test]
    fn roundtrip_golomb_k0() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            // val=0, k=0 -> just "1"
            bw.write_golomb(0, 0).unwrap();
            // val=3, k=0 -> "0001"
            bw.write_golomb(0, 3).unwrap();
            // val=1, k=0 -> "01"
            bw.write_golomb(0, 1).unwrap();
            bw.flush().unwrap();
        }

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_golomb(0).unwrap(), 0);
        assert_eq!(br.read_golomb(0).unwrap(), 3);
        assert_eq!(br.read_golomb(0).unwrap(), 1);
    }

    #[test]
    fn roundtrip_golomb_k2() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            // val=5, k=2 -> q=1, r=1 -> "01" + "01" = "0101"
            bw.write_golomb(2, 5).unwrap();
            // val=0, k=2 -> q=0, r=0 -> "1" + "00" = "100"
            bw.write_golomb(2, 0).unwrap();
            bw.flush().unwrap();
        }

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_golomb(2).unwrap(), 5);
        assert_eq!(br.read_golomb(2).unwrap(), 0);
    }

    #[test]
    fn roundtrip_golomb_many_values() {
        for k in 0..8 {
            let values: Vec<u32> = (0..50).collect();
            let mut buf = Vec::new();
            {
                let mut bw = BitWriter::new(&mut buf);
                for &v in &values {
                    bw.write_golomb(k, v).unwrap();
                }
                bw.flush().unwrap();
            }

            let mut br = BitReader::new(&buf);
            for &v in &values {
                let decoded = br.read_golomb(k).unwrap();
                assert_eq!(decoded, v, "k={k}, expected {v}, got {decoded}");
            }
        }
    }

    #[test]
    fn byte_stuffing_0xff_on_write() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0xFF, 8).unwrap();
            bw.flush().unwrap();
        }
        // 0xFF should be followed by 0x00 stuff byte
        assert_eq!(buf, vec![0xFF, 0x00]);
    }

    #[test]
    fn byte_stuffing_roundtrip() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            // Write value 0xFF (8 bits) then value 0x01 (8 bits)
            bw.write_bits(0xFF, 8).unwrap();
            bw.write_bits(0x01, 8).unwrap();
            bw.flush().unwrap();
        }
        // Should be: FF 00 01
        assert_eq!(buf, vec![0xFF, 0x00, 0x01]);

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(8).unwrap(), 0xFF);
        assert_eq!(br.read_bits(8).unwrap(), 0x01);
    }

    #[test]
    fn read_bits_empty_returns_error() {
        let buf = [];
        let mut br = BitReader::new(&buf);
        assert!(br.read_bit().is_err());
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
    fn read_zero_bits_returns_zero() {
        let buf = [0xAB];
        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(0).unwrap(), 0);
    }
}
