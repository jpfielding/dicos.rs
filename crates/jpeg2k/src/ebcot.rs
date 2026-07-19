//! EBCOT (Embedded Block Coding with Optimal Truncation) -- ITU-T T.800 Annex D.
//!
//! Simplified tier-1 implementation for lossless encoding: significance
//! propagation, magnitude refinement, and cleanup passes over bit-planes
//! using the MQ arithmetic coder.

use crate::mq::{
    setup_default_contexts, MqDecoder, MqEncoder, MqState, CTX_MR_START, CTX_RUN_LENGTH,
    CTX_SC_START, CTX_UNIFORM,
};

// ---------------------------------------------------------------------------
// Code-block encoder
// ---------------------------------------------------------------------------

/// Encodes a single code-block using EBCOT tier-1.
pub struct CodeBlockEncoder {
    mq: MqEncoder,
    contexts: Vec<MqState>,
    width: usize,
    height: usize,
    /// Significance state with 1-pixel border: stride = width + 2.
    sigma: Vec<u8>,
    /// Snapshot of sigma at the start of each bit-plane, used to determine
    /// which coefficients belong to sig-prop vs cleanup.
    sigma_snapshot: Vec<u8>,
}

impl CodeBlockEncoder {
    pub fn new(width: usize, height: usize) -> Self {
        let n = (width + 2) * (height + 2);
        Self {
            mq: MqEncoder::new(),
            contexts: setup_default_contexts(),
            width,
            height,
            sigma: vec![0u8; n],
            sigma_snapshot: vec![0u8; n],
        }
    }

    /// Encode the coefficients and return `(coded_data, num_passes, num_bit_planes)`.
    ///
    /// Returns `(empty vec, 0, 0)` when all coefficients are zero.
    pub fn encode(&mut self, data: &[i32]) -> (Vec<u8>, usize, usize) {
        // Find maximum magnitude.
        let max_val = data.iter().map(|&v| v.unsigned_abs()).max().unwrap_or(0);
        if max_val == 0 {
            return (Vec::new(), 0, 0);
        }

        let num_bit_planes = 32 - max_val.leading_zeros() as usize;
        let mut passes = 0usize;

        for bp in (0..num_bit_planes).rev() {
            let mask = 1u32 << bp;

            // Snapshot sigma before this bit-plane so that sig-prop and
            // cleanup agree on which coefficients have significant neighbors
            // from *previous* bit-planes (not the current one).
            self.sigma_snapshot.copy_from_slice(&self.sigma);

            // Significance propagation pass.
            self.sig_prop_pass(data, mask);
            passes += 1;

            // Magnitude refinement pass (not for the first bit-plane).
            if bp < num_bit_planes - 1 {
                self.mag_ref_pass(data, mask);
                passes += 1;
            }

            // Cleanup pass.
            self.cleanup_pass(data, mask);
            passes += 1;
        }

        self.mq.flush();
        let bytes = self.mq.bytes().to_vec();
        (bytes, passes, num_bit_planes)
    }

    /// Reset for a new code-block.
    pub fn reset(&mut self) {
        self.mq.reset();
        self.contexts = setup_default_contexts();
        self.sigma.fill(0);
        self.sigma_snapshot.fill(0);
    }

    // -- Coding passes -------------------------------------------------------

    fn sig_prop_pass(&mut self, data: &[i32], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                if self.sigma_snapshot[idx] != 0 {
                    continue; // already significant from previous bit-planes
                }
                if !Self::has_significant_neighbor_in(&self.sigma_snapshot, idx, stride) {
                    continue;
                }

                let abs_val = data[y * self.width + x].unsigned_abs();
                let sig = if (abs_val & mask) != 0 { 1u8 } else { 0u8 };

                let ctx = Self::zc_context_in(&self.sigma_snapshot, idx, stride);
                self.mq.encode(sig, &mut self.contexts[ctx]);

                if sig == 1 {
                    self.sigma[idx] = 1;
                    let sign_bit = if data[y * self.width + x] < 0 {
                        1u8
                    } else {
                        0u8
                    };
                    self.mq.encode(sign_bit, &mut self.contexts[CTX_SC_START]);
                }
            }
        }
    }

    fn mag_ref_pass(&mut self, data: &[i32], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                // Only refine coefficients that were significant BEFORE this
                // bit-plane (i.e., in the snapshot).
                if self.sigma_snapshot[idx] == 0 {
                    continue;
                }

                let abs_val = data[y * self.width + x].unsigned_abs();
                let bit = if (abs_val & mask) != 0 { 1u8 } else { 0u8 };
                self.mq.encode(bit, &mut self.contexts[CTX_MR_START]);
            }
        }
    }

    fn cleanup_pass(&mut self, data: &[i32], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                if self.sigma[idx] != 0 {
                    continue; // already significant (from previous bit-planes or sig-prop)
                }
                // Use the snapshot to determine if this position was handled in
                // sig-prop.  Positions whose neighbors only became significant
                // during the *current* sig-prop or cleanup pass must still be
                // coded here.
                if Self::has_significant_neighbor_in(&self.sigma_snapshot, idx, stride) {
                    continue; // handled in sig-prop
                }

                let abs_val = data[y * self.width + x].unsigned_abs();
                let sig = if (abs_val & mask) != 0 { 1u8 } else { 0u8 };
                self.mq.encode(sig, &mut self.contexts[CTX_RUN_LENGTH]);

                if sig == 1 {
                    self.sigma[idx] = 1;
                    let sign_bit = if data[y * self.width + x] < 0 {
                        1u8
                    } else {
                        0u8
                    };
                    self.mq.encode(sign_bit, &mut self.contexts[CTX_UNIFORM]);
                }
            }
        }
    }

    // -- Context helpers ------------------------------------------------------

    fn has_significant_neighbor_in(sigma: &[u8], idx: usize, stride: usize) -> bool {
        sigma[idx - stride - 1] != 0
            || sigma[idx - stride] != 0
            || sigma[idx - stride + 1] != 0
            || sigma[idx - 1] != 0
            || sigma[idx + 1] != 0
            || sigma[idx + stride - 1] != 0
            || sigma[idx + stride] != 0
            || sigma[idx + stride + 1] != 0
    }

    fn zc_context_in(sigma: &[u8], idx: usize, stride: usize) -> usize {
        let mut count = 0usize;
        if sigma[idx - 1] != 0 {
            count += 1;
        }
        if sigma[idx + 1] != 0 {
            count += 1;
        }
        if sigma[idx - stride] != 0 {
            count += 1;
        }
        if sigma[idx + stride] != 0 {
            count += 1;
        }
        count.min(4) // context indices 0..=4
    }
}

// ---------------------------------------------------------------------------
// Code-block decoder
// ---------------------------------------------------------------------------

/// Decodes a single code-block using EBCOT tier-1.
pub struct CodeBlockDecoder<'a> {
    mq: MqDecoder<'a>,
    contexts: Vec<MqState>,
    width: usize,
    height: usize,
    sigma: Vec<u8>,
    sigma_snapshot: Vec<u8>,
}

impl<'a> CodeBlockDecoder<'a> {
    pub fn new(data: &'a [u8], width: usize, height: usize) -> Self {
        let n = (width + 2) * (height + 2);
        Self {
            mq: MqDecoder::new(data),
            contexts: setup_default_contexts(),
            width,
            height,
            sigma: vec![0u8; n],
            sigma_snapshot: vec![0u8; n],
        }
    }

    /// Decode the code-block data and return reconstructed coefficients.
    pub fn decode(&mut self, num_bit_planes: usize, num_passes: usize) -> Vec<i32> {
        let n = self.width * self.height;
        let mut coeffs = vec![0u32; n];
        let mut signs = vec![0u8; n];

        let mut pass_idx = 0usize;
        for bp in (0..num_bit_planes).rev() {
            let mask = 1u32 << bp;

            // Snapshot sigma before this bit-plane (must match encoder).
            self.sigma_snapshot.copy_from_slice(&self.sigma);

            // Significance propagation pass.
            if pass_idx < num_passes {
                self.decode_sig_prop_pass(&mut coeffs, &mut signs, mask);
                pass_idx += 1;
            }

            // Magnitude refinement pass.
            if bp < num_bit_planes - 1 && pass_idx < num_passes {
                self.decode_mag_ref_pass(&mut coeffs, mask);
                pass_idx += 1;
            }

            // Cleanup pass.
            if pass_idx < num_passes {
                self.decode_cleanup_pass(&mut coeffs, &mut signs, mask);
                pass_idx += 1;
            }
        }

        // Apply signs.
        coeffs
            .iter()
            .zip(signs.iter())
            .map(|(&c, &s)| if s != 0 { -(c as i32) } else { c as i32 })
            .collect()
    }

    // -- Decoding passes ------------------------------------------------------

    fn decode_sig_prop_pass(&mut self, coeffs: &mut [u32], signs: &mut [u8], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                if self.sigma_snapshot[idx] != 0 {
                    continue; // already significant from previous bit-planes
                }
                if !Self::has_significant_neighbor_in(&self.sigma_snapshot, idx, stride) {
                    continue;
                }

                let ctx = Self::zc_context_in(&self.sigma_snapshot, idx, stride);
                let sig = self.mq.decode(&mut self.contexts[ctx]);

                if sig == 1 {
                    self.sigma[idx] = 1;
                    coeffs[y * self.width + x] |= mask;
                    signs[y * self.width + x] = self.mq.decode(&mut self.contexts[CTX_SC_START]);
                }
            }
        }
    }

    fn decode_mag_ref_pass(&mut self, coeffs: &mut [u32], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                // Only refine coefficients that were significant BEFORE this
                // bit-plane (matching the encoder).
                if self.sigma_snapshot[idx] == 0 {
                    continue;
                }
                let bit = self.mq.decode(&mut self.contexts[CTX_MR_START]);
                if bit == 1 {
                    coeffs[y * self.width + x] |= mask;
                }
            }
        }
    }

    fn decode_cleanup_pass(&mut self, coeffs: &mut [u32], signs: &mut [u8], mask: u32) {
        let stride = self.width + 2;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (y + 1) * stride + (x + 1);
                if self.sigma[idx] != 0 {
                    continue; // already significant (from previous bit-planes or sig-prop)
                }
                // Use snapshot to determine if handled in sig-prop (matches encoder).
                if Self::has_significant_neighbor_in(&self.sigma_snapshot, idx, stride) {
                    continue;
                }

                let sig = self.mq.decode(&mut self.contexts[CTX_RUN_LENGTH]);
                if sig == 1 {
                    self.sigma[idx] = 1;
                    coeffs[y * self.width + x] |= mask;
                    signs[y * self.width + x] = self.mq.decode(&mut self.contexts[CTX_UNIFORM]);
                }
            }
        }
    }

    // -- Context helpers ------------------------------------------------------

    fn has_significant_neighbor_in(sigma: &[u8], idx: usize, stride: usize) -> bool {
        sigma[idx - stride - 1] != 0
            || sigma[idx - stride] != 0
            || sigma[idx - stride + 1] != 0
            || sigma[idx - 1] != 0
            || sigma[idx + 1] != 0
            || sigma[idx + stride - 1] != 0
            || sigma[idx + stride] != 0
            || sigma[idx + stride + 1] != 0
    }

    fn zc_context_in(sigma: &[u8], idx: usize, stride: usize) -> usize {
        let mut count = 0usize;
        if sigma[idx - 1] != 0 {
            count += 1;
        }
        if sigma[idx + 1] != 0 {
            count += 1;
        }
        if sigma[idx - stride] != 0 {
            count += 1;
        }
        if sigma[idx + stride] != 0 {
            count += 1;
        }
        count.min(4)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ebcot_all_zeros() {
        let mut enc = CodeBlockEncoder::new(4, 4);
        let data = vec![0i32; 16];
        let (bytes, passes, bit_planes) = enc.encode(&data);
        assert!(bytes.is_empty());
        assert_eq!(passes, 0);
        assert_eq!(bit_planes, 0);
    }

    #[test]

    fn ebcot_roundtrip_small() {
        let w = 4;
        let h = 4;
        let data: Vec<i32> = vec![
            1, -2, 3, -4, 5, -6, 7, -8, 9, -10, 11, -12, 13, -14, 15, -16,
        ];

        let mut enc = CodeBlockEncoder::new(w, h);
        let (bytes, passes, bit_planes) = enc.encode(&data);
        assert!(!bytes.is_empty());
        assert!(passes > 0);
        assert!(bit_planes > 0);

        let mut dec = CodeBlockDecoder::new(&bytes, w, h);
        let decoded = dec.decode(bit_planes, passes);
        assert_eq!(decoded, data);
    }

    #[test]

    fn ebcot_roundtrip_uniform() {
        let w = 8;
        let h = 8;
        let data = vec![42i32; w * h];

        let mut enc = CodeBlockEncoder::new(w, h);
        let (bytes, passes, bit_planes) = enc.encode(&data);

        let mut dec = CodeBlockDecoder::new(&bytes, w, h);
        let decoded = dec.decode(bit_planes, passes);
        assert_eq!(decoded, data);
    }

    #[test]
    fn ebcot_roundtrip_single_nonzero() {
        let w = 4;
        let h = 4;
        let mut data = vec![0i32; w * h];
        data[5] = 255;

        let mut enc = CodeBlockEncoder::new(w, h);
        let (bytes, passes, bit_planes) = enc.encode(&data);

        let mut dec = CodeBlockDecoder::new(&bytes, w, h);
        let decoded = dec.decode(bit_planes, passes);
        assert_eq!(decoded, data);
    }

    #[test]

    fn ebcot_roundtrip_negative_values() {
        let w = 4;
        let h = 4;
        let data: Vec<i32> = (-8..8).collect();

        let mut enc = CodeBlockEncoder::new(w, h);
        let (bytes, passes, bit_planes) = enc.encode(&data);

        let mut dec = CodeBlockDecoder::new(&bytes, w, h);
        let decoded = dec.decode(bit_planes, passes);
        assert_eq!(decoded, data);
    }

    #[test]

    fn ebcot_roundtrip_large_values() {
        let w = 4;
        let h = 4;
        let data: Vec<i32> = (0..16).map(|i| (i * 1000) - 8000).collect();

        let mut enc = CodeBlockEncoder::new(w, h);
        let (bytes, passes, bit_planes) = enc.encode(&data);

        let mut dec = CodeBlockDecoder::new(&bytes, w, h);
        let decoded = dec.decode(bit_planes, passes);
        assert_eq!(decoded, data);
    }

    #[test]

    fn ebcot_encoder_reset() {
        let w = 4;
        let h = 4;
        let data1 = vec![10i32; w * h];
        let data2: Vec<i32> = (1..=16).collect();

        let mut enc = CodeBlockEncoder::new(w, h);

        // Encode first block.
        let (bytes1, passes1, bp1) = enc.encode(&data1);
        enc.reset();

        // Encode second block.
        let (bytes2, passes2, bp2) = enc.encode(&data2);

        // Decode both independently.
        let mut dec1 = CodeBlockDecoder::new(&bytes1, w, h);
        assert_eq!(dec1.decode(bp1, passes1), data1);

        let mut dec2 = CodeBlockDecoder::new(&bytes2, w, h);
        assert_eq!(dec2.decode(bp2, passes2), data2);
    }
}
