//! Frozen 1.0.0 / Go-compatible JPEG-LS coder.
//!
//! This module is a self-contained copy of the entropy-coded scan path as it
//! existed in the 1.0.0 release: **0xFF00 byte stuffing, no run mode, uncapped
//! Golomb, pre-spec bias updates**. It is byte-compatible with the Go dicos
//! codec and pinned forever by `tests/legacy_fixtures.rs`.
//!
//! DO NOT MODIFY. The conformant T.87 coder is being built alongside it in
//! `bitstream.rs` / `context.rs`; this frozen path preserves the legacy
//! bitstream verbatim so existing files keep decoding byte-for-byte.

use std::io::{self, Write};

use crate::error::CodecError;
use crate::predictor::{clamp, predict_med};

// ===========================================================================
// Frozen bit I/O (0xFF00 stuffing, 65536 unary cap)
// ===========================================================================

/// Reads bits from a byte slice, handling legacy 0xFF00 byte-stuffing.
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

    /// Read a Golomb-Rice code with parameter `k` (uncapped remainder).
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

/// Writes bits to an underlying `Write` sink, handling legacy 0xFF00 stuffing.
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

    /// Write raw bytes. Must only be called when the bit buffer is empty.
    pub fn write_bytes(&mut self, buf: &[u8]) -> io::Result<()> {
        debug_assert_eq!(
            self.n_bits, 0,
            "write_bytes called while bit buffer is non-empty"
        );
        self.inner.write_all(buf)
    }
}

// ===========================================================================
// Frozen context model (pre-spec bias / statistics)
// ===========================================================================

/// Number of regular contexts.
const NUM_REGULAR_CONTEXTS: usize = 365;
/// Two additional contexts for run interruption samples.
const NUM_RUN_CONTEXTS: usize = 2;
/// Total context array size.
const NUM_CONTEXTS: usize = NUM_REGULAR_CONTEXTS + NUM_RUN_CONTEXTS;

/// J-table values for run-length coding (ISO 14495-1, Table A.3).
const J_TABLE: [i32; 32] = [
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10, 11, 12, 13,
    14, 15,
];

/// The frozen context model used during legacy encoding and decoding.
pub(crate) struct ContextModel {
    pub t1: i32,
    pub t2: i32,
    pub t3: i32,
    pub max_val: i32,
    pub a: Vec<i32>,
    pub b: Vec<i32>,
    pub c: Vec<i32>,
    pub n: Vec<i32>,
    pub reset: i32,
    pub j: [i32; 32],
    pub run_index: usize,
}

impl ContextModel {
    /// Create a new context model for the given `max_val`, `near`, and `reset`.
    pub fn new(max_val: i32, near: i32, reset: i32) -> Self {
        // Compute quantization thresholds (legacy formula).
        let factor = (max_val.min(4095) + 128) / 256;

        let t1 = clamp(factor + 2 + 3 * near, near + 1, max_val);
        let t2 = clamp(factor * (7 - 3) + 3 + 5 * near, t1, max_val);
        let t3 = clamp(factor * (21 - 4) + 4 + 7 * near, t2, max_val);

        let mut model = Self {
            t1,
            t2,
            t3,
            max_val,
            a: vec![0; NUM_CONTEXTS],
            b: vec![0; NUM_CONTEXTS],
            c: vec![0; NUM_CONTEXTS],
            n: vec![0; NUM_CONTEXTS],
            reset,
            j: J_TABLE,
            run_index: 0,
        };

        // Initialise statistics: A[Q] = 4, N[Q] = 1.
        for i in 0..NUM_CONTEXTS {
            model.a[i] = 4;
            model.n[i] = 1;
        }

        model
    }

    /// Quantize a single gradient value `d` into one of 9 buckets (-4..=4).
    #[inline]
    pub fn quantize_gradient(&self, d: i32) -> i32 {
        if d <= -self.t3 {
            -4
        } else if d <= -self.t2 {
            -3
        } else if d <= -self.t1 {
            -2
        } else if d < 0 {
            -1
        } else if d == 0 {
            0
        } else if d < self.t1 {
            1
        } else if d < self.t2 {
            2
        } else if d < self.t3 {
            3
        } else {
            4
        }
    }

    /// Compute the context index `Q` and the `sign` from the three gradients.
    pub fn get_context_index(&self, d1: i32, d2: i32, d3: i32) -> (usize, i32) {
        let mut q1 = self.quantize_gradient(d1);
        let mut q2 = self.quantize_gradient(d2);
        let mut q3 = self.quantize_gradient(d3);

        let mut sign = 1i32;
        if q1 < 0 || (q1 == 0 && q2 < 0) || (q1 == 0 && q2 == 0 && q3 < 0) {
            q1 = -q1;
            q2 = -q2;
            q3 = -q3;
            sign = -1;
        }

        let index = (q1 * 81 + q2 * 9 + q3) as usize;
        (index, sign)
    }

    /// Compute the Golomb-Rice parameter `k` for context `q`.
    pub fn compute_k(&self, q: usize) -> i32 {
        let n = self.n[q];
        if n == 0 {
            return 0;
        }
        let a = self.a[q];
        let mut k = 0i32;
        while k < 31 && (n << k) < a {
            k += 1;
        }
        k
    }

    /// Update the A/B/C/N statistics for context `q` after observing `err_val`.
    pub fn update_stats(&mut self, q: usize, err_val: i32) {
        self.b[q] = self.b[q].saturating_add(err_val);
        self.a[q] = self.a[q].saturating_add(err_val.abs());

        // Halve when N reaches reset threshold.
        if self.n[q] >= self.reset {
            self.a[q] >>= 1;
            self.b[q] >>= 1;
            self.n[q] >>= 1;
        }
        self.n[q] = self.n[q].saturating_add(1);

        // Bias correction update.
        self.update_bias(q);
    }

    /// Adjust the bias correction variable C[Q] and keep B[Q] in range.
    fn update_bias(&mut self, q: usize) {
        if self.b[q] <= -self.n[q] {
            self.b[q] += self.n[q];
            self.c[q] -= 1;
            if self.b[q] <= -self.n[q] {
                self.b[q] += self.n[q];
                self.c[q] -= 1;
            }
        } else if self.b[q] > 0 {
            self.b[q] -= self.n[q];
            self.c[q] += 1;
            if self.b[q] > 0 {
                self.b[q] -= self.n[q];
                self.c[q] += 1;
            }
        }

        // Clamp C[Q] to [-128, 127].
        self.c[q] = self.c[q].clamp(-128, 127);
    }
}

// ===========================================================================
// Frozen scan coders
// ===========================================================================

/// Encode the entropy-coded scan body for a single-component image.
///
/// Creates a legacy [`BitWriter`] over `out`, runs the frozen regular-mode
/// scan loop, and flushes. Header/EOI markers are written by the caller.
pub(crate) fn encode_scan(
    out: &mut dyn Write,
    pixels: &[u16],
    width: usize,
    height: usize,
    max_val: i32,
    near: i32,
) -> Result<(), CodecError> {
    let mut bw = BitWriter::new(out);
    let mut ctx = ContextModel::new(max_val, near, 64);
    encode_scan_inner(&mut bw, &mut ctx, pixels, width, height, max_val)?;
    bw.flush()?;
    Ok(())
}

fn encode_scan_inner<W: Write>(
    bw: &mut BitWriter<W>,
    ctx: &mut ContextModel,
    pixels: &[u16],
    w: usize,
    h: usize,
    max_val: i32,
) -> Result<(), CodecError> {
    let mut curr_line = vec![0i32; w];
    let mut prev_line = vec![0i32; w];

    let range_val = max_val + 1;

    for y in 0..h {
        ctx.run_index = 0;

        // Read current line into curr_line.
        for x in 0..w {
            curr_line[x] = i32::from(pixels[y * w + x]);
        }

        let mut x = 0usize;
        while x < w {
            let ra = if x > 0 {
                curr_line[x - 1]
            } else if y > 0 {
                prev_line[0]
            } else {
                0
            };

            let rb = if y > 0 { prev_line[x] } else { 0 };

            let rc = if y > 0 {
                if x > 0 {
                    prev_line[x - 1]
                } else {
                    prev_line[0]
                }
            } else {
                0
            };

            let rd = if y > 0 {
                if x < w - 1 {
                    prev_line[x + 1]
                } else {
                    rb
                }
            } else {
                0
            };

            let d1 = rd - rb;
            let d2 = rb - rc;
            let d3 = rc - ra;

            // Run mode is intentionally disabled for compatibility with
            // existing DICOS files produced by the Go codec. Regular mode:
            let (q, sign) = ctx.get_context_index(d1, d2, d3);

            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let ix = curr_line[x];
            let mut err_val = ix - px;
            if sign == -1 {
                err_val = -err_val;
            }

            // Modulo reduction.
            if err_val < -range_val / 2 {
                err_val += range_val;
            }
            if err_val > range_val / 2 {
                err_val -= range_val;
            }

            // Map error to non-negative.
            let mapped = if err_val >= 0 {
                (2 * err_val) as u32
            } else {
                (-2 * err_val - 1) as u32
            };

            let k = ctx.compute_k(q);
            bw.write_golomb(k, mapped)?;

            ctx.update_stats(q, err_val);
            x += 1;
        }

        prev_line.copy_from_slice(&curr_line);
    }
    Ok(())
}

/// Decode the entropy-coded scan body for a single-component image.
///
/// Creates a legacy [`BitReader`] over `scan_data` and reconstructs `pixels`.
pub(crate) fn decode_scan(
    scan_data: &[u8],
    pixels: &mut [u16],
    w: usize,
    h: usize,
    max_val: i32,
    near: i32,
) -> Result<(), CodecError> {
    let mut br = BitReader::new(scan_data);
    let mut ctx = ContextModel::new(max_val, near, 64);
    decode_scan_inner(&mut br, &mut ctx, pixels, w, h, max_val)
}

fn decode_scan_inner(
    br: &mut BitReader<'_>,
    ctx: &mut ContextModel,
    pixels: &mut [u16],
    w: usize,
    h: usize,
    max_val: i32,
) -> Result<(), CodecError> {
    let mut curr_line = vec![0i32; w];
    let mut prev_line = vec![0i32; w];

    let max_val_plus1 = max_val + 1;

    for y in 0..h {
        ctx.run_index = 0;

        let mut x = 0usize;
        while x < w {
            let ra = if x > 0 {
                curr_line[x - 1]
            } else if y > 0 {
                prev_line[0]
            } else {
                0
            };

            let rb = if y > 0 { prev_line[x] } else { 0 };

            let rc = if y > 0 {
                if x > 0 {
                    prev_line[x - 1]
                } else {
                    prev_line[0]
                }
            } else {
                0
            };

            let rd = if y > 0 {
                if x < w - 1 {
                    prev_line[x + 1]
                } else {
                    rb
                }
            } else {
                0
            };

            let d1 = rd - rb;
            let d2 = rb - rc;
            let d3 = rc - ra;

            // Run mode is intentionally disabled for compatibility with
            // existing DICOS files produced by the Go codec. Regular mode:
            let (q, sign) = ctx.get_context_index(d1, d2, d3);

            let mut px = predict_med(ra, rb, rc);
            px += sign * ctx.c[q];
            px = clamp(px, 0, max_val);

            let k = ctx.compute_k(q);
            let mapped_err = match br.read_golomb(k) {
                Ok(v) => v,
                Err(e) => {
                    // A marker-encountered error during the last pixels of
                    // the image is normal (EOI marker).
                    if e.to_string().contains("marker encountered") {
                        return Ok(());
                    }
                    return Err(e);
                }
            };

            // Inverse-map the error (using wrapping to handle large values).
            let em = mapped_err as i32;
            let stats_err = if em & 1 == 0 {
                em >> 1
            } else {
                em.wrapping_add(1).wrapping_neg() >> 1
            };

            let mut err_val = stats_err;
            if sign == -1 {
                err_val = -err_val;
            }

            ctx.update_stats(q, stats_err);

            let mut rx = (px as i64 + err_val as i64) as i32;

            // Modulo reduction to [0, max_val].
            if rx < 0 {
                rx += max_val_plus1;
            }
            if rx > max_val {
                rx -= max_val_plus1;
            }
            rx = rx.clamp(0, max_val);

            curr_line[x] = rx;
            pixels[y * w + x] = rx as u16;

            x += 1;
        }

        for xi in 0..w {
            pixels[y * w + xi] = curr_line[xi] as u16;
        }

        prev_line.copy_from_slice(&curr_line);
    }
    Ok(())
}

// ===========================================================================
// Tests -- lock the frozen primitives (end-to-end bytes are pinned by
// tests/legacy_fixtures.rs).
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_roundtrip() {
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
    fn golomb_roundtrip_many() {
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
                assert_eq!(br.read_golomb(k).unwrap(), v, "k={k}, v={v}");
            }
        }
    }

    #[test]
    fn byte_stuffing_0xff00() {
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            bw.write_bits(0xFF, 8).unwrap();
            bw.write_bits(0x01, 8).unwrap();
            bw.flush().unwrap();
        }
        // Legacy stuffing inserts a 0x00 after every 0xFF data byte.
        assert_eq!(buf, vec![0xFF, 0x00, 0x01]);

        let mut br = BitReader::new(&buf);
        assert_eq!(br.read_bits(8).unwrap(), 0xFF);
        assert_eq!(br.read_bits(8).unwrap(), 0x01);
    }

    #[test]
    fn context_thresholds_8bit() {
        let m = ContextModel::new(255, 0, 64);
        assert_eq!((m.t1, m.t2, m.t3), (3, 7, 21));
    }

    #[test]
    fn context_initial_stats() {
        let m = ContextModel::new(255, 0, 64);
        for i in 0..NUM_CONTEXTS {
            assert_eq!(m.a[i], 4);
            assert_eq!(m.n[i], 1);
        }
    }

    #[test]
    fn context_update_stats_double_step_bias() {
        let mut m = ContextModel::new(255, 0, 64);
        // Frozen (pre-spec) double-step bias: B=5 -> C=2, B=1.
        m.update_stats(0, 5);
        assert_eq!(m.a[0], 9);
        assert_eq!(m.b[0], 1);
        assert_eq!(m.c[0], 2);
        assert_eq!(m.n[0], 2);
    }
}
