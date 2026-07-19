//! T.87 (ITU-T T.87 / ISO 14495-1) JPEG-LS context model.
//!
//! Maintains the A/B/C/N statistics, the near-aware gradient quantizer, the
//! Golomb parameter derivation, and the run-mode statistics used by both the
//! encoder and the decoder.
//!
//! References: T.87 A.2 (initialization), A.3 (context quantization), A.5
//! (Golomb parameter / limited coding), A.6 (variable update), A.7 (run mode).

/// Number of regular contexts (indices `0..=364`).
const NUM_REGULAR_CONTEXTS: usize = 365;
/// Two additional contexts for run-interruption samples (indices `365`, `366`).
const NUM_RUN_CONTEXTS: usize = 2;
/// Total context array size (`367`).
const NUM_CONTEXTS: usize = NUM_REGULAR_CONTEXTS + NUM_RUN_CONTEXTS;

/// Index of the run-interruption context with RItype = 0 (Ra == Rb).
const RUN_CTX_BASE: usize = 365;

/// Bias-correction bounds (T.87 A.6.2, MIN_C / MAX_C).
const MIN_C: i32 = -128;
const MAX_C: i32 = 127;

/// Default basic thresholds (T.87 A.2.1: BASIC_T1/T2/T3).
const BASIC_T1: i32 = 3;
const BASIC_T2: i32 = 7;
const BASIC_T3: i32 = 21;

/// J-table for run-length coding (T.87 A.7.1.1, Table A.16 — 32 entries).
const J_TABLE: [i32; 32] = [
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10, 11, 12, 13,
    14, 15,
];

/// Smallest `k` such that `2^k >= x` (ceil of log2), for `x >= 1`.
fn ceil_log2(x: i32) -> i32 {
    let mut k = 0;
    while (1i64 << k) < i64::from(x) {
        k += 1;
    }
    k
}

/// T.87 threshold clamp (A.2.1): returns `lo` when `i` falls outside
/// `[lo, hi]` (note: an over-`hi` value collapses to `lo`, not `hi`).
fn clamp_threshold(i: i32, lo: i32, hi: i32) -> i32 {
    if i > hi || i < lo {
        lo
    } else {
        i
    }
}

/// The T.87 context model state used during encoding and decoding.
pub(crate) struct ContextModel {
    // --- Quantization thresholds (T.87 A.2.1 / A.3) ---
    pub t1: i32,
    pub t2: i32,
    pub t3: i32,

    /// Maximum sample value (`MAXVAL`).
    pub max_val: i32,
    /// Near-lossless error tolerance (`NEAR`; 0 for lossless).
    pub near: i32,
    /// Reconstruction range (`RANGE`, T.87 A.3.1).
    pub range: i32,
    /// Bits needed to represent a mapped/limited value (`qbpp`, T.87 A.5.3).
    pub qbpp: i32,
    /// Unary escape limit (`LIMIT`, T.87 A.5.3 / A.2).
    pub limit: i32,

    // --- Per-context statistics ---
    /// A[Q]: sum of absolute prediction errors.
    pub a: Vec<i32>,
    /// B[Q]: bias accumulator.
    pub b: Vec<i32>,
    /// C[Q]: bias correction value.
    pub c: Vec<i32>,
    /// N[Q]: occurrence count.
    pub n: Vec<i32>,
    /// Nn[RItype]: negative-error counters for the two run contexts (A.7.2).
    pub nn: [i32; 2],

    /// Reset threshold — halve statistics when `N[Q]` reaches this (`RESET`).
    pub reset: i32,

    // --- Run mode ---
    /// J-table for run-length coding.
    pub j: [i32; 32],
    /// Current run index (`RUNindex`).
    pub run_index: usize,
}

impl ContextModel {
    /// Create a new context model for the given `max_val`, `near`, and `reset`.
    pub fn new(max_val: i32, near: i32, reset: i32) -> Self {
        // Derived parameters (T.87 A.2 / A.3.1 / A.5.3).
        let bpp = ceil_log2(max_val + 1).max(2);
        let limit = 2 * (bpp + bpp.max(8));
        let range = if near == 0 {
            max_val + 1
        } else {
            (max_val + 2 * near) / (2 * near + 1) + 1
        };
        let qbpp = ceil_log2(range);

        let (t1, t2, t3) = Self::default_thresholds(max_val, near);

        // A[Q] initialization (T.87 A.2.1 / A.8): max(2, floor((RANGE+32)/64))
        // for ALL contexts (regular AND run).
        let a_init = ((range + 32) / 64).max(2);

        // T.87 A.2.1 / A.8 initialization: A[Q] = a_init, N[Q] = 1,
        // B[Q] = C[Q] = 0, Nn = 0.
        Self {
            t1,
            t2,
            t3,
            max_val,
            near,
            range,
            qbpp,
            limit,
            a: vec![a_init; NUM_CONTEXTS],
            b: vec![0; NUM_CONTEXTS],
            c: vec![0; NUM_CONTEXTS],
            n: vec![1; NUM_CONTEXTS],
            nn: [0, 0],
            reset,
            j: J_TABLE,
            run_index: 0,
        }
    }

    /// Compute the default T1/T2/T3 thresholds (T.87 A.2.1).
    fn default_thresholds(max_val: i32, near: i32) -> (i32, i32, i32) {
        if max_val >= 128 {
            let factor = (max_val.min(4095) + 128) / 256;
            let t1 = clamp_threshold(factor * (BASIC_T1 - 2) + 2 + 3 * near, near + 1, max_val);
            let t2 = clamp_threshold(factor * (BASIC_T2 - 3) + 3 + 5 * near, t1, max_val);
            let t3 = clamp_threshold(factor * (BASIC_T3 - 4) + 4 + 7 * near, t2, max_val);
            (t1, t2, t3)
        } else {
            let factor = 256 / (max_val + 1);
            let t1 = clamp_threshold((BASIC_T1 / factor + 3 * near).max(2), near + 1, max_val);
            let t2 = clamp_threshold((BASIC_T2 / factor + 5 * near).max(3), t1, max_val);
            let t3 = clamp_threshold((BASIC_T3 / factor + 7 * near).max(4), t2, max_val);
            (t1, t2, t3)
        }
    }

    /// Near-aware gradient quantization into one of 9 buckets (-4..=4), T.87
    /// A.3.3.
    #[inline]
    pub fn quantize_gradient(&self, d: i32) -> i32 {
        if d <= -self.t3 {
            -4
        } else if d <= -self.t2 {
            -3
        } else if d <= -self.t1 {
            -2
        } else if d < -self.near {
            -1
        } else if d <= self.near {
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

        let index = (q1 * 81 + q2 * 9 + q3) as usize;
        (index, sign)
    }

    /// Compute the Golomb-Rice parameter `k` for a regular context `q`
    /// (T.87 A.5.1): smallest `k` with `N[Q] << k >= A[Q]`.
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

    /// Update A/B/N (T.87 A.6.1, code segment A.12) then the bias C (A.6.2,
    /// A.13) for regular context `q` after observing corrected error `err_val`.
    pub fn update_stats(&mut self, q: usize, err_val: i32) {
        // A.12
        self.b[q] += err_val * (2 * self.near + 1);
        self.a[q] += err_val.abs();
        if self.n[q] == self.reset {
            self.a[q] >>= 1;
            self.b[q] = if self.b[q] >= 0 {
                self.b[q] >> 1
            } else {
                -((1 - self.b[q]) >> 1)
            };
            self.n[q] >>= 1;
        }
        self.n[q] += 1;

        self.update_bias(q);
    }

    /// Single-step bias update with clamping (T.87 A.6.2, code segment A.13).
    ///
    /// Uses `N[Q]` *after* the increment performed in [`Self::update_stats`].
    fn update_bias(&mut self, q: usize) {
        if self.b[q] <= -self.n[q] {
            if self.c[q] > MIN_C {
                self.c[q] -= 1;
            }
            self.b[q] += self.n[q];
            if self.b[q] <= -self.n[q] {
                self.b[q] = -self.n[q] + 1;
            }
        } else if self.b[q] > 0 {
            if self.c[q] < MAX_C {
                self.c[q] += 1;
            }
            self.b[q] -= self.n[q];
            if self.b[q] > 0 {
                self.b[q] = 0;
            }
        }
    }

    /// Golomb parameter `k` for a run-interruption sample (T.87 A.7.2.2).
    ///
    /// `ri_type` is 0 (Ra == Rb) or 1 (Ra != Rb), selecting run context
    /// `365 + ri_type`.
    pub fn compute_k_run(&self, ri_type: usize) -> i32 {
        let ctx = RUN_CTX_BASE + ri_type;
        let temp = if ri_type == 1 {
            self.a[RUN_CTX_BASE + 1] + (self.n[RUN_CTX_BASE + 1] >> 1)
        } else {
            self.a[RUN_CTX_BASE]
        };
        let n = self.n[ctx];
        let mut k = 0i32;
        while k < 31 && (n << k) < temp {
            k += 1;
        }
        k
    }

    /// Update run-interruption statistics (T.87 A.7.2, code segment A.23).
    ///
    /// `e_mapped` is `EMErrval`; `err_negative` indicates the (corrected)
    /// error was negative.
    pub fn update_run_stats(&mut self, ri_type: usize, e_mapped: u32, err_negative: bool) {
        let ctx = RUN_CTX_BASE + ri_type;
        if err_negative {
            self.nn[ri_type] += 1;
        }
        self.a[ctx] += ((e_mapped + 1 - ri_type as u32) >> 1) as i32;
        if self.n[ctx] == self.reset {
            self.a[ctx] >>= 1;
            self.n[ctx] >>= 1;
            self.nn[ri_type] >>= 1;
        }
        self.n[ctx] += 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn model_8bit() -> ContextModel {
        ContextModel::new(255, 0, 64)
    }

    fn model_16bit() -> ContextModel {
        ContextModel::new(65535, 0, 64)
    }

    // -- Derived parameters --

    #[test]
    fn derived_params_8bit() {
        let m = model_8bit();
        assert_eq!(m.range, 256);
        assert_eq!(m.qbpp, 8);
        // bpp = 8, LIMIT = 2*(8 + max(8,8)) = 32.
        assert_eq!(m.limit, 32);
    }

    #[test]
    fn derived_params_16bit() {
        let m = model_16bit();
        assert_eq!(m.range, 65536);
        assert_eq!(m.qbpp, 16);
        // bpp = 16, LIMIT = 2*(16 + max(8,16)) = 64.
        assert_eq!(m.limit, 64);
    }

    // -- A[Q] initialization (Codex finding 7) --

    #[test]
    fn a_init_8bit_is_4() {
        // max(2, floor((256+32)/64)) = max(2, 4) = 4.
        let m = model_8bit();
        for i in 0..NUM_CONTEXTS {
            assert_eq!(m.a[i], 4, "ctx {i}");
            assert_eq!(m.n[i], 1);
            assert_eq!(m.b[i], 0);
            assert_eq!(m.c[i], 0);
        }
        assert_eq!(m.nn, [0, 0]);
    }

    #[test]
    fn a_init_16bit_is_1024() {
        // max(2, floor((65536+32)/64)) = max(2, 1024) = 1024, for ALL contexts
        // including the two run contexts.
        let m = model_16bit();
        for i in 0..NUM_CONTEXTS {
            assert_eq!(m.a[i], 1024, "ctx {i}");
        }
    }

    // -- Thresholds --

    #[test]
    fn thresholds_8bit() {
        let m = model_8bit();
        assert_eq!((m.t1, m.t2, m.t3), (3, 7, 21));
    }

    #[test]
    fn thresholds_16bit() {
        let m = model_16bit();
        // factor = (min(65535,4095)+128)/256 = 16 -> 18, 67, 276.
        assert_eq!((m.t1, m.t2, m.t3), (18, 67, 276));
    }

    #[test]
    fn thresholds_near2_8bit() {
        let m = ContextModel::new(255, 2, 64);
        // factor=1: T1=1+2+6=9, T2=4+3+10=17, T3=17+4+14=35.
        assert_eq!((m.t1, m.t2, m.t3), (9, 17, 35));
    }

    #[test]
    fn thresholds_small_maxval() {
        // MAXVAL < 128 branch. factor = 256/101 = 2.
        let m = ContextModel::new(100, 0, 64);
        // T1=max(2,3/2)=2, T2=max(3,7/2=3)=3, T3=max(4,21/2=10)=10.
        assert_eq!((m.t1, m.t2, m.t3), (2, 3, 10));
    }

    // -- Quantizer --

    #[test]
    fn quantize_near0() {
        let m = model_8bit(); // T1=3, T2=7, T3=21, near=0
        assert_eq!(m.quantize_gradient(0), 0);
        assert_eq!(m.quantize_gradient(1), 1);
        assert_eq!(m.quantize_gradient(2), 1);
        assert_eq!(m.quantize_gradient(3), 2);
        assert_eq!(m.quantize_gradient(6), 2);
        assert_eq!(m.quantize_gradient(7), 3);
        assert_eq!(m.quantize_gradient(20), 3);
        assert_eq!(m.quantize_gradient(21), 4);
        assert_eq!(m.quantize_gradient(-1), -1);
        assert_eq!(m.quantize_gradient(-3), -2);
        assert_eq!(m.quantize_gradient(-7), -3);
        assert_eq!(m.quantize_gradient(-21), -4);
    }

    #[test]
    fn quantize_near2() {
        let m = ContextModel::new(255, 2, 64); // T1=9, T2=17, T3=35, near=2
        assert_eq!(m.quantize_gradient(0), 0);
        assert_eq!(m.quantize_gradient(2), 0); // |d| <= near
        assert_eq!(m.quantize_gradient(-2), 0);
        assert_eq!(m.quantize_gradient(3), 1); // d > near, < T1
        assert_eq!(m.quantize_gradient(-3), -1);
        assert_eq!(m.quantize_gradient(9), 2); // >= T1, < T2
        assert_eq!(m.quantize_gradient(-9), -2);
        assert_eq!(m.quantize_gradient(17), 3);
        assert_eq!(m.quantize_gradient(35), 4);
        assert_eq!(m.quantize_gradient(-35), -4);
    }

    // -- Context index --

    #[test]
    fn context_index_all_zero() {
        let m = model_8bit();
        assert_eq!(m.get_context_index(0, 0, 0), (0, 1));
    }

    #[test]
    fn context_index_sign_flip() {
        let m = model_8bit();
        let (idx1, sign1) = m.get_context_index(-1, 0, 0);
        let (idx2, sign2) = m.get_context_index(1, 0, 0);
        assert_eq!(idx1, idx2);
        assert_eq!(sign1, -1);
        assert_eq!(sign2, 1);
    }

    // -- Golomb k --

    #[test]
    fn compute_k_initial_8bit() {
        // A=4, N=1 -> smallest k with (1<<k) >= 4 is 2.
        assert_eq!(model_8bit().compute_k(0), 2);
    }

    // -- Regular stats update (A.12 / A.13, single step) --

    #[test]
    fn update_stats_single_step_bias() {
        let mut m = model_8bit();
        // near=0: B += 5, A += 5. Then single-step bias: B=5>0 -> C=1,
        // B -= N(2) => 3, B>0 => B=0.
        m.update_stats(0, 5);
        assert_eq!(m.a[0], 9);
        assert_eq!(m.b[0], 0);
        assert_eq!(m.c[0], 1);
        assert_eq!(m.n[0], 2);
    }

    #[test]
    fn update_stats_reset_halving() {
        let mut m = ContextModel::new(255, 0, 4);
        // A=4, N=1. The N == RESET check fires on the 4th call (N reaches 4).
        m.update_stats(0, 0); // N 1 -> 2
        m.update_stats(0, 0); // N 2 -> 3
        m.update_stats(0, 0); // N 3 -> 4
        assert_eq!(m.n[0], 4);
        assert_eq!(m.a[0], 4);
        m.update_stats(0, 0); // N == 4 -> halve A to 2, N to 2, then N -> 3
        assert_eq!(m.n[0], 3);
        assert_eq!(m.a[0], 2);
    }

    #[test]
    fn bias_correction_clamps() {
        let mut m = model_8bit();
        for _ in 0..1000 {
            m.update_stats(0, -100);
        }
        assert!(m.c[0] >= MIN_C);

        let mut m2 = model_8bit();
        for _ in 0..1000 {
            m2.update_stats(1, 100);
        }
        assert!(m2.c[1] <= MAX_C);
    }

    // -- Run mode --

    #[test]
    fn compute_k_run_initial_8bit() {
        let m = model_8bit();
        // RItype 0: TEMP = A[365] = 4, N=1 -> k=2.
        assert_eq!(m.compute_k_run(0), 2);
        // RItype 1: TEMP = A[366] + (N[366]>>1) = 4 + 0 = 4, N=1 -> k=2.
        assert_eq!(m.compute_k_run(1), 2);
    }

    #[test]
    fn update_run_stats_counts_and_reset() {
        let mut m = ContextModel::new(255, 0, 4);
        let ctx = RUN_CTX_BASE; // ri_type 0
                                // A starts 4, N starts 1.
        m.update_run_stats(0, 3, true); // A += (3+1-0)>>1 = 2 -> 6; Nn[0]=1; N 1->2
        assert_eq!(m.a[ctx], 6);
        assert_eq!(m.nn[0], 1);
        assert_eq!(m.n[ctx], 2);

        m.update_run_stats(0, 1, false); // A += (1+1)>>1 = 1 -> 7; N 2->3
        m.update_run_stats(0, 1, false); // A += 1 -> 8; N 3->4
        assert_eq!(m.n[ctx], 4);
        m.update_run_stats(0, 1, true); // N==4 -> halve: A=(8+1)>>1=4, N=2, Nn=0; then N->3
        assert_eq!(m.n[ctx], 3);
        assert_eq!(m.a[ctx], 4);
        // Nn[0] was 1, incremented to 2 by err_negative, halved to 1.
        assert_eq!(m.nn[0], 1);
    }

    #[test]
    fn j_table_matches_spec() {
        let m = model_8bit();
        assert_eq!(
            m.j,
            [
                0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10,
                11, 12, 13, 14, 15
            ]
        );
    }
}
