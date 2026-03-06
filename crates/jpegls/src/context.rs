//! JPEG-LS context model.
//!
//! Maintains the A/B/C/N statistic arrays, gradient quantization tables,
//! and the Golomb parameter `k` computation used by both encoder and decoder.
//!
//! Reference: ISO/IEC 14495-1, Sections A.2 -- A.6.

use crate::predictor::clamp;

/// Number of regular contexts (5 * 9 * 9 = 405 possible, but after sign
/// normalisation the first non-zero quantized gradient is always >= 0,
/// giving a maximum index of 4*81 + 4*9 + 4 = 364, i.e. 365 contexts).
const NUM_REGULAR_CONTEXTS: usize = 365;

/// Two additional contexts for run interruption samples (A.4.2).
const NUM_RUN_CONTEXTS: usize = 2;

/// Total context array size.
const NUM_CONTEXTS: usize = NUM_REGULAR_CONTEXTS + NUM_RUN_CONTEXTS;

/// J-table values for run-length coding (ISO 14495-1, Table A.3).
const J_TABLE: [i32; 32] = [
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10, 11, 12, 13,
    14, 15,
];

/// The context model state used during encoding and decoding.
pub(crate) struct ContextModel {
    // --- Quantization thresholds (ISO A.3) ---
    pub t1: i32,
    pub t2: i32,
    pub t3: i32,

    /// Maximum sample value (`(1 << precision) - 1`).
    pub max_val: i32,

    // --- Per-context statistics ---
    /// A[Q]: sum of absolute prediction errors.
    pub a: Vec<i32>,
    /// B[Q]: sum of prediction errors (bias accumulator).
    pub b: Vec<i32>,
    /// C[Q]: bias correction value.
    pub c: Vec<i32>,
    /// N[Q]: occurrence count.
    pub n: Vec<i32>,

    /// Reset threshold -- halve statistics when `N[Q]` reaches this.
    pub reset: i32,

    // --- Run mode ---
    /// J-table for run-length coding.
    pub j: [i32; 32],
    /// Current run index (reset to 0 at the start of each line).
    pub run_index: usize,
}

impl ContextModel {
    /// Create a new context model for the given `max_val`, `near`, and `reset`.
    ///
    /// `near` is the near-lossless tolerance (0 for lossless).
    pub fn new(max_val: i32, near: i32, reset: i32) -> Self {
        // Compute quantization thresholds (ISO A.3).
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

        // Initialise statistics (ISO A.2): A[Q] = 4, N[Q] = 1.
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
    ///
    /// The sign normalisation ensures that the first non-zero quantized
    /// gradient is always non-negative.  When the sign is flipped, the
    /// prediction error must also be negated.
    ///
    /// Returns `(Q, sign)` where `sign` is `1` or `-1`.
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

        // Index in [0, 364].
        let index = (q1 * 81 + q2 * 9 + q3) as usize;
        (index, sign)
    }

    /// Compute the Golomb-Rice parameter `k` for context `q`.
    ///
    /// `k` is the smallest integer such that `N[Q] << k >= A[Q]`.
    /// Capped at 31 to prevent shift overflow.
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
    ///
    /// `err_val` is the prediction error *before* sign adjustment (i.e. the
    /// value used for the mapped-error computation, not the raw pixel diff).
    ///
    /// Uses saturating arithmetic to prevent overflow with 16-bit image data
    /// where prediction errors can be large.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a default 8-bit lossless context model.
    fn model_8bit() -> ContextModel {
        ContextModel::new(255, 0, 64)
    }

    /// Helper: create a default 16-bit lossless context model.
    fn model_16bit() -> ContextModel {
        ContextModel::new(65535, 0, 64)
    }

    // -- Threshold tests --

    #[test]
    fn thresholds_8bit() {
        let m = model_8bit();
        assert_eq!(m.t1, 3);
        assert_eq!(m.t2, 7);
        assert_eq!(m.t3, 21);
    }

    #[test]
    fn thresholds_16bit() {
        let m = model_16bit();
        // factor = (4095+128)/256 = 16
        // T1 = 16*1 + 2 = 18
        // T2 = 16*4 + 3 = 67
        // T3 = 16*17 + 4 = 276
        assert_eq!(m.t1, 18);
        assert_eq!(m.t2, 67);
        assert_eq!(m.t3, 276);
    }

    // -- Gradient quantization --

    #[test]
    fn quantize_zero() {
        let m = model_8bit();
        assert_eq!(m.quantize_gradient(0), 0);
    }

    #[test]
    fn quantize_positive_buckets() {
        let m = model_8bit(); // T1=3, T2=7, T3=21
        assert_eq!(m.quantize_gradient(1), 1); // 0 < d < T1
        assert_eq!(m.quantize_gradient(2), 1);
        assert_eq!(m.quantize_gradient(3), 2); // T1 <= d < T2
        assert_eq!(m.quantize_gradient(6), 2);
        assert_eq!(m.quantize_gradient(7), 3); // T2 <= d < T3
        assert_eq!(m.quantize_gradient(20), 3);
        assert_eq!(m.quantize_gradient(21), 4); // d >= T3
        assert_eq!(m.quantize_gradient(100), 4);
    }

    #[test]
    fn quantize_negative_buckets() {
        let m = model_8bit();
        assert_eq!(m.quantize_gradient(-1), -1);
        assert_eq!(m.quantize_gradient(-2), -1);
        assert_eq!(m.quantize_gradient(-3), -2);
        assert_eq!(m.quantize_gradient(-7), -3);
        assert_eq!(m.quantize_gradient(-21), -4);
        assert_eq!(m.quantize_gradient(-100), -4);
    }

    // -- Context index --

    #[test]
    fn context_index_all_zero() {
        let m = model_8bit();
        let (idx, sign) = m.get_context_index(0, 0, 0);
        assert_eq!(idx, 0);
        assert_eq!(sign, 1);
    }

    #[test]
    fn context_index_sign_flip() {
        let m = model_8bit();
        // D1=-1 (Q1=-1) -> first nonzero is negative -> flip
        let (idx1, sign1) = m.get_context_index(-1, 0, 0);
        let (idx2, sign2) = m.get_context_index(1, 0, 0);
        assert_eq!(idx1, idx2);
        assert_eq!(sign1, -1);
        assert_eq!(sign2, 1);
    }

    // -- Golomb k --

    #[test]
    fn compute_k_initial() {
        let m = model_8bit();
        // A=4, N=1 -> k: 1<<k >= 4 -> k=2
        assert_eq!(m.compute_k(0), 2);
    }

    #[test]
    fn compute_k_zero_n() {
        let mut m = model_8bit();
        m.n[0] = 0;
        assert_eq!(m.compute_k(0), 0);
    }

    // -- Stats update --

    #[test]
    fn update_stats_basic() {
        let mut m = model_8bit();
        // Initial: A=4, B=0, N=1
        m.update_stats(0, 5);
        assert_eq!(m.a[0], 9); // 4 + |5| = 9
                               // B starts at 0 + 5 = 5, then bias correction:
                               //   B=5 > 0: B -= N(2) => 3, C++; still > 0: B -= 2 => 1, C++
        assert_eq!(m.b[0], 1);
        assert_eq!(m.c[0], 2);
        assert_eq!(m.n[0], 2);
    }

    #[test]
    fn update_stats_reset() {
        let mut m = ContextModel::new(255, 0, 4);
        // Initial: A=4, N=1.
        // ISO 14495-1 A.6.1: the halving check (N[Q] == RESET) happens
        // *before* N is incremented, so N must reach RESET first and the
        // halving occurs on the *next* call.
        m.update_stats(0, 0); // N: 1 (not >= 4) -> N = 2
        m.update_stats(0, 0); // N: 2 (not >= 4) -> N = 3
        m.update_stats(0, 0); // N: 3 (not >= 4) -> N = 4
        assert_eq!(m.n[0], 4);
        m.update_stats(0, 0); // N: 4 (>= 4) -> halve N to 2, then N++ = 3
                              // After halving: A=2, B=0, N=2, then N++ -> 3
        assert_eq!(m.n[0], 3);
    }

    #[test]
    fn bias_correction_clamp() {
        let mut m = model_8bit();
        // Drive C[0] below -128
        for _ in 0..300 {
            m.update_stats(0, -100);
        }
        assert!(m.c[0] >= -128);

        // Drive C[1] above 127
        let mut m2 = model_8bit();
        for _ in 0..300 {
            m2.update_stats(1, 100);
        }
        assert!(m2.c[1] <= 127);
    }

    // -- Initialisation --

    #[test]
    fn initial_stats() {
        let m = model_8bit();
        for i in 0..NUM_CONTEXTS {
            assert_eq!(m.a[i], 4);
            assert_eq!(m.b[i], 0);
            assert_eq!(m.c[i], 0);
            assert_eq!(m.n[i], 1);
        }
    }

    #[test]
    fn j_table_length() {
        let m = model_8bit();
        assert_eq!(m.j.len(), 32);
        assert_eq!(m.j[0], 0);
        assert_eq!(m.j[31], 15);
    }
}
