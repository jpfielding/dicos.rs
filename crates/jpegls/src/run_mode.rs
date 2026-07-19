//! Run-length mode for JPEG-LS.
//!
//! When all three local gradients are zero (D1 == D2 == D3 == 0) the codec
//! enters run mode, efficiently coding long runs of identical pixel values.
//!
//! Reference: ISO/IEC 14495-1, Section A.7.

// TODO(t87): run mode is rewritten and wired into the scan loops in L4. Until
// then this module builds on the frozen legacy primitives so its behavior and
// tests stay stable while bitstream.rs / context.rs diverge toward T.87.
use crate::error::CodecError;
use crate::legacy::{BitReader, BitWriter, ContextModel};
use std::io::Write;

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Encode a run-mode segment starting at column `x` in the current line.
///
/// On return, `*x` is updated to point past the last encoded pixel.
pub(crate) fn encode_run<W: Write>(
    bw: &mut BitWriter<W>,
    ctx: &mut ContextModel,
    curr_line: &[i32],
    x: &mut usize,
    width: usize,
    ra: i32,
    rb: i32,
) -> Result<(), CodecError> {
    let max_val = ctx.max_val;

    // 1. Measure the run of pixels equal to Ra starting at *x.
    let start = *x;
    while *x < width && curr_line[*x] == ra {
        *x += 1;
    }
    let mut run_length = *x - start;

    // 2. Encode the run using the J table.
    loop {
        let j = ctx.j[ctx.run_index] as u32;
        let limit = 1u32 << j;

        if run_length >= limit as usize {
            // Full segment -- write a 1-bit.
            bw.write_bit(1)?;
            run_length -= limit as usize;
            if ctx.run_index < 31 {
                ctx.run_index += 1;
            }

            // If the run consumed exactly to the end of the line, we are
            // done.  No trailing 0-bit or remainder is written in this
            // case (ISO 14495-1 A.7.1.1: the decoder infers EOL from the
            // line width).
            if run_length == 0 && *x >= width {
                return Ok(());
            }
        } else {
            // Partial segment -- write a 0-bit, then the remainder in j bits.
            bw.write_bit(0)?;
            if j > 0 {
                bw.write_bits(run_length as u32, j as i32)?;
            }
            if ctx.run_index > 0 {
                ctx.run_index -= 1;
            }

            // If we consumed the entire line, we are done.
            if *x >= width {
                return Ok(());
            }

            // 3. Encode the interruption sample.
            let ix = curr_line[*x];
            let (px, sign) = run_interruption_prediction(ra, rb);

            let mut err_val = ix - px;
            if sign == -1 {
                err_val = -err_val;
            }

            // Modulo reduction.
            let range_val = max_val + 1;
            if err_val < -range_val / 2 {
                err_val += range_val;
            }
            if err_val > range_val / 2 {
                err_val -= range_val;
            }

            // Context for interruption: Q = 365 if Ra == Rb, else 366.
            let q = if ra == rb { 365 } else { 366 };

            // Map error to non-negative value.
            let mapped = if err_val >= 0 {
                (2 * err_val) as u32
            } else {
                (-2 * err_val - 1) as u32
            };

            let k = ctx.compute_k(q);
            bw.write_golomb(k, mapped)?;
            ctx.update_stats(q, err_val);

            *x += 1;
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// Decode a run-mode segment, writing pixels into `curr_line` starting at `*x`.
///
/// On return, `*x` is updated to point past the last decoded pixel.
pub(crate) fn decode_run(
    br: &mut BitReader<'_>,
    ctx: &mut ContextModel,
    curr_line: &mut [i32],
    x: &mut usize,
    width: usize,
    ra: i32,
    rb: i32,
) -> Result<(), CodecError> {
    let max_val = ctx.max_val;

    loop {
        let b = br.read_bit()?;

        if b == 1 {
            // Full segment of 2^J[RunIndex] pixels all equal to Ra.
            let j = ctx.j[ctx.run_index] as u32;
            let mut run_length = (1usize) << j;

            let remaining = width - *x;
            if run_length > remaining {
                run_length = remaining;
            }

            for _ in 0..run_length {
                curr_line[*x] = ra;
                *x += 1;
            }

            if ctx.run_index < 31 {
                ctx.run_index += 1;
            }

            // If we filled the entire line, return.
            if *x >= width {
                return Ok(());
            }
            // Otherwise loop to read the next segment.
        } else {
            // Partial segment -- read j bits for remainder.
            let j = ctx.j[ctx.run_index] as u32;
            let r_bits = if j > 0 { br.read_bits(j as i32)? } else { 0 };
            let mut run_length = r_bits as usize;

            let remaining = width - *x;
            if run_length > remaining {
                run_length = remaining;
            }

            for _ in 0..run_length {
                curr_line[*x] = ra;
                *x += 1;
            }

            if ctx.run_index > 0 {
                ctx.run_index -= 1;
            }

            // End of line?
            if *x >= width {
                return Ok(());
            }

            // Decode the interruption sample.
            let q = if ra == rb { 365usize } else { 366 };
            let k = ctx.compute_k(q);
            let mapped_err = br.read_golomb(k)?;

            let err_val = if mapped_err % 2 == 0 {
                (mapped_err / 2) as i32
            } else {
                -((mapped_err.wrapping_add(1) / 2) as i32)
            };

            ctx.update_stats(q, err_val);

            let (px, sign) = run_interruption_prediction(ra, rb);
            // Use wrapping arithmetic -- the intermediate value can exceed
            // i32 range for 16-bit images. Modulo reduction brings it back.
            let mut ix = (px as i64 + sign as i64 * err_val as i64) as i32;

            // Modulo reduction to [0, max_val].
            let range_val = max_val + 1;
            if ix < 0 {
                ix += range_val;
            }
            if ix > max_val {
                ix -= range_val;
            }
            ix = ix.clamp(0, max_val);

            curr_line[*x] = ix;
            *x += 1;
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the prediction and sign for a run interruption sample.
///
/// Returns `(Px, sign)`.
#[inline]
fn run_interruption_prediction(ra: i32, rb: i32) -> (i32, i32) {
    if ra == rb {
        (ra, 1)
    } else if ra > rb {
        (rb, -1)
    } else {
        (rb, 1)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy::{BitReader, BitWriter, ContextModel};

    /// Encode then decode a run and verify the output matches.
    fn roundtrip_run(curr_line: &[i32], ra: i32, rb: i32) -> Vec<i32> {
        let width = curr_line.len();

        // Encode
        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            encode_run(&mut bw, &mut ctx, curr_line, &mut x, width, ra, rb).unwrap();
            assert_eq!(x, width);
            bw.flush().unwrap();
        }

        // Decode
        let mut out = vec![0i32; width];
        {
            let mut br = BitReader::new(&buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            decode_run(&mut br, &mut ctx, &mut out, &mut x, width, ra, rb).unwrap();
            assert_eq!(x, width);
        }
        out
    }

    #[test]
    fn run_all_same() {
        let line = vec![42i32; 8];
        let out = roundtrip_run(&line, 42, 42);
        assert_eq!(out, line);
    }

    #[test]
    fn run_with_interruption() {
        // 5 pixels of Ra=100, then one different pixel 120.
        let line = vec![100, 100, 100, 100, 100, 120];
        let ra = 100;
        let rb = 100;
        let width = line.len();

        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            encode_run(&mut bw, &mut ctx, &line, &mut x, width, ra, rb).unwrap();
            // x should be at 6 (past the interruption sample).
            assert_eq!(x, 6);
            bw.flush().unwrap();
        }

        let mut out = vec![0i32; width];
        {
            let mut br = BitReader::new(&buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            decode_run(&mut br, &mut ctx, &mut out, &mut x, width, ra, rb).unwrap();
            assert_eq!(x, 6);
        }
        assert_eq!(out, line);
    }

    #[test]
    fn run_single_pixel_differs() {
        // Immediate interruption: first pixel differs from Ra.
        let line = vec![50i32];
        let ra = 100;
        let rb = 100;
        let width = 1;

        let mut buf = Vec::new();
        {
            let mut bw = BitWriter::new(&mut buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            encode_run(&mut bw, &mut ctx, &line, &mut x, width, ra, rb).unwrap();
            assert_eq!(x, 1);
            bw.flush().unwrap();
        }

        let mut out = vec![0i32; width];
        {
            let mut br = BitReader::new(&buf);
            let mut ctx = ContextModel::new(255, 0, 64);
            let mut x = 0usize;
            decode_run(&mut br, &mut ctx, &mut out, &mut x, width, ra, rb).unwrap();
            assert_eq!(x, 1);
        }
        assert_eq!(out, line);
    }

    #[test]
    fn run_interruption_prediction_same() {
        let (px, sign) = run_interruption_prediction(100, 100);
        assert_eq!(px, 100);
        assert_eq!(sign, 1);
    }

    #[test]
    fn run_interruption_prediction_ra_gt_rb() {
        let (px, sign) = run_interruption_prediction(200, 100);
        assert_eq!(px, 100);
        assert_eq!(sign, -1);
    }

    #[test]
    fn run_interruption_prediction_ra_lt_rb() {
        let (px, sign) = run_interruption_prediction(50, 200);
        assert_eq!(px, 200);
        assert_eq!(sign, 1);
    }
}
