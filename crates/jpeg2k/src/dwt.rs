//! 5/3 reversible discrete wavelet transform (lifting scheme).
//!
//! Implements the forward and inverse DWT as specified in ITU-T T.800
//! Annex F, using the lifting-based factorisation of the Le Gall 5/3 filter.

// ---------------------------------------------------------------------------
// 1-D transforms
// ---------------------------------------------------------------------------

/// Forward 1-D 5/3 wavelet transform (in-place, lifting scheme).
///
/// After the transform the first `(n+1)/2` samples contain the low-pass
/// coefficients and the remaining `n/2` samples contain the high-pass
/// coefficients.
pub fn forward_1d(signal: &mut [i32]) {
    let n = signal.len();
    if n < 2 {
        return;
    }

    let half = n.div_ceil(2);
    let num_high = n - half;

    // Split into even (low) and odd (high) samples.
    let mut low = vec![0i32; half];
    let mut high = vec![0i32; num_high];

    for i in 0..half {
        low[i] = signal[2 * i];
    }
    for i in 0..num_high {
        high[i] = signal[2 * i + 1];
    }

    // Predict step  (high-pass):
    //   d[i] = x[2i+1] - floor((x[2i] + x[2i+2]) / 2)
    // Arithmetic right shift gives floor for negative operands (T.800 F.4.8.2).
    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left }; // symmetric extension
        high[i] -= (left + right) >> 1;
    }

    // Update step  (low-pass):
    //   s[i] = x[2i] + floor((d[i-1] + d[i] + 2) / 4)
    for i in 0..half {
        let left = if i > 0 {
            high[i - 1]
        } else if !high.is_empty() {
            high[0] // symmetric extension
        } else {
            0
        };
        let right = if i < num_high { high[i] } else { left };
        low[i] += (left + right + 2) >> 2;
    }

    // Pack: low first, then high.
    signal[..half].copy_from_slice(&low);
    signal[half..].copy_from_slice(&high);
}

/// Inverse 1-D 5/3 wavelet transform (in-place, lifting scheme).
///
/// Expects low-pass coefficients in `signal[..half]` and high-pass in
/// `signal[half..]` where `half = (n+1)/2`.
pub fn inverse_1d(signal: &mut [i32]) {
    let n = signal.len();
    if n < 2 {
        return;
    }

    let half = n.div_ceil(2);
    let num_high = n - half;

    let mut low = vec![0i32; half];
    let mut high = vec![0i32; num_high];

    low.copy_from_slice(&signal[..half]);
    high.copy_from_slice(&signal[half..]);

    // Inverse update step.
    for i in 0..half {
        let left = if i > 0 {
            high[i - 1]
        } else if !high.is_empty() {
            high[0]
        } else {
            0
        };
        let right = if i < num_high { high[i] } else { left };
        low[i] -= (left + right + 2) >> 2;
    }

    // Inverse predict step.
    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left };
        high[i] += (left + right) >> 1;
    }

    // Interleave.
    for i in 0..half {
        signal[2 * i] = low[i];
    }
    for i in 0..num_high {
        signal[2 * i + 1] = high[i];
    }
}

// ---------------------------------------------------------------------------
// Legacy 1-D transforms (truncating division)
// ---------------------------------------------------------------------------
//
// The 1.0.0 raw-DWT pipeline (still wired through `encode`/`decode` via
// `forward_multi_level`/`inverse_multi_level`) used truncating integer
// division (`/2`, `/4`), which rounds toward zero rather than flooring. The
// checked-in legacy fixtures pin those exact bytes, so the legacy multi-level
// entry points MUST keep using these functions verbatim. Do not "fix" them.

/// Legacy forward 1-D 5/3 transform (truncating division). Frozen for byte
/// compatibility with 1.0.0 codestreams -- see `tests/legacy_fixtures.rs`.
#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
fn legacy_forward_1d(signal: &mut [i32]) {
    let n = signal.len();
    if n < 2 {
        return;
    }

    let half = n.div_ceil(2);
    let num_high = n - half;

    let mut low = vec![0i32; half];
    let mut high = vec![0i32; num_high];

    for i in 0..half {
        low[i] = signal[2 * i];
    }
    for i in 0..num_high {
        high[i] = signal[2 * i + 1];
    }

    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left };
        high[i] -= (left + right) / 2;
    }

    for i in 0..half {
        let left = if i > 0 {
            high[i - 1]
        } else if !high.is_empty() {
            high[0]
        } else {
            0
        };
        let right = if i < num_high { high[i] } else { left };
        low[i] += (left + right + 2) / 4;
    }

    signal[..half].copy_from_slice(&low);
    signal[half..].copy_from_slice(&high);
}

/// Legacy inverse 1-D 5/3 transform (truncating division). Frozen for byte
/// compatibility with 1.0.0 codestreams -- see `tests/legacy_fixtures.rs`.
#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
fn legacy_inverse_1d(signal: &mut [i32]) {
    let n = signal.len();
    if n < 2 {
        return;
    }

    let half = n.div_ceil(2);
    let num_high = n - half;

    let mut low = vec![0i32; half];
    let mut high = vec![0i32; num_high];

    low.copy_from_slice(&signal[..half]);
    high.copy_from_slice(&signal[half..]);

    for i in 0..half {
        let left = if i > 0 {
            high[i - 1]
        } else if !high.is_empty() {
            high[0]
        } else {
            0
        };
        let right = if i < num_high { high[i] } else { left };
        low[i] -= (left + right + 2) / 4;
    }

    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left };
        high[i] += (left + right) / 2;
    }

    for i in 0..half {
        signal[2 * i] = low[i];
    }
    for i in 0..num_high {
        signal[2 * i + 1] = high[i];
    }
}

// ---------------------------------------------------------------------------
// 2-D transforms (single level)
// ---------------------------------------------------------------------------

/// Forward 2-D 5/3 DWT -- produces LL, HL, LH, HH subbands (in-place).
#[allow(dead_code)] // TODO(t800): single-level 2-D helper, retained for tests
pub fn forward_2d(data: &mut [i32], width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Transform rows.
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * width;
        row.copy_from_slice(&data[off..off + width]);
        forward_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
    }

    // Transform columns.
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * width + x];
        }
        forward_1d(&mut col);
        for y in 0..height {
            data[y * width + x] = col[y];
        }
    }
}

/// Inverse 2-D 5/3 DWT -- reconstructs from LL, HL, LH, HH subbands (in-place).
#[allow(dead_code)] // TODO(t800): single-level 2-D helper, retained for tests
pub fn inverse_2d(data: &mut [i32], width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Inverse columns first.
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * width + x];
        }
        inverse_1d(&mut col);
        for y in 0..height {
            data[y * width + x] = col[y];
        }
    }

    // Inverse rows.
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * width;
        row.copy_from_slice(&data[off..off + width]);
        inverse_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
    }
}

// ---------------------------------------------------------------------------
// Multi-level transforms (operates on LL region of previous level)
// ---------------------------------------------------------------------------

/// Forward multi-level 2-D DWT (LEGACY truncating pipeline).
///
/// Each level transforms the LL subband from the previous level.
/// Returns the (width, height) of the final LL subband.
///
/// This is the path wired through `encode`; it uses [`legacy_forward_1d`] and
/// stops early on degenerate sub-regions, matching 1.0.0 byte output. Do not
/// change its behaviour -- use the `*_conformant` entry points for new work.
#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
pub fn forward_multi_level(
    data: &mut [i32],
    width: usize,
    height: usize,
    levels: usize,
) -> (usize, usize) {
    let mut ll_w = width;
    let mut ll_h = height;

    for _ in 0..levels {
        if ll_w < 2 || ll_h < 2 {
            break;
        }
        forward_ll_region(data, width, ll_w, ll_h);
        ll_w = ll_w.div_ceil(2);
        ll_h = ll_h.div_ceil(2);
    }
    (ll_w, ll_h)
}

/// Inverse multi-level 2-D DWT.
///
/// Levels are processed in reverse order, from smallest to largest.
#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
pub fn inverse_multi_level(data: &mut [i32], width: usize, height: usize, levels: usize) {
    // Pre-calculate LL dimensions at each level.
    let mut dims = vec![(0usize, 0usize); levels + 1];
    dims[0] = (width, height);
    for i in 1..=levels {
        dims[i] = (dims[i - 1].0.div_ceil(2), dims[i - 1].1.div_ceil(2));
    }

    // Reconstruct from smallest to largest.
    for level in (0..levels).rev() {
        let (ll_w, ll_h) = dims[level];
        if ll_w < 2 || ll_h < 2 {
            continue;
        }
        inverse_ll_region(data, width, ll_w, ll_h);
    }
}

// ---------------------------------------------------------------------------
// Helpers -- forward/inverse on the LL sub-region
// ---------------------------------------------------------------------------

#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
fn forward_ll_region(data: &mut [i32], stride: usize, width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Transform rows in the region (legacy truncating 1-D).
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * stride;
        row.copy_from_slice(&data[off..off + width]);
        legacy_forward_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
    }

    // Transform columns in the region (legacy truncating 1-D).
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * stride + x];
        }
        legacy_forward_1d(&mut col);
        for y in 0..height {
            data[y * stride + x] = col[y];
        }
    }
}

#[allow(dead_code)] // frozen legacy (v1.0.0) DWT helper; live only via `legacy` decode
fn inverse_ll_region(data: &mut [i32], stride: usize, width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Inverse columns first (legacy truncating 1-D).
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * stride + x];
        }
        legacy_inverse_1d(&mut col);
        for y in 0..height {
            data[y * stride + x] = col[y];
        }
    }

    // Inverse rows (legacy truncating 1-D).
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * stride;
        row.copy_from_slice(&data[off..off + width]);
        legacy_inverse_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
    }
}

// ---------------------------------------------------------------------------
// Conformant multi-level transforms (T.800 F.4.8 -- floor lifting)
// ---------------------------------------------------------------------------
//
// These are the spec-conformant entry points the future tier-1/tier-2 pipeline
// will use. They differ from the legacy path in three ways:
//   * they call the floor-lifting `forward_1d`/`inverse_1d`;
//   * they transform degenerate 1xN / Nx1 sub-regions along the dimension of
//     length >= 2 (a 1x1 level is a no-op) rather than stopping early;
//   * they always perform exactly `levels` levels;
//   * they hoist one row-sized and one column-sized scratch buffer per call,
//     reused across every row, column, and level (no per-row allocation).

/// Forward multi-level 2-D 5/3 DWT (conformant, floor lifting).
///
/// Returns the `(width, height)` of the final LL sub-band.
// TODO(t800): unwire when tier-1 lands
#[allow(dead_code)]
pub fn forward_multi_level_conformant(
    data: &mut [i32],
    width: usize,
    height: usize,
    levels: usize,
) -> (usize, usize) {
    let mut row_scratch = vec![0i32; width];
    let mut col_scratch = vec![0i32; height];

    let mut ll_w = width;
    let mut ll_h = height;
    for _ in 0..levels {
        forward_ll_region_conformant(data, width, ll_w, ll_h, &mut row_scratch, &mut col_scratch);
        ll_w = ll_w.div_ceil(2);
        ll_h = ll_h.div_ceil(2);
    }
    (ll_w, ll_h)
}

/// Inverse multi-level 2-D 5/3 DWT (conformant, floor lifting).
// TODO(t800): unwire when tier-1 lands
#[allow(dead_code)]
pub fn inverse_multi_level_conformant(
    data: &mut [i32],
    width: usize,
    height: usize,
    levels: usize,
) {
    // LL dimensions at each level.
    let mut dims = vec![(0usize, 0usize); levels + 1];
    dims[0] = (width, height);
    for i in 1..=levels {
        dims[i] = (dims[i - 1].0.div_ceil(2), dims[i - 1].1.div_ceil(2));
    }

    let mut row_scratch = vec![0i32; width];
    let mut col_scratch = vec![0i32; height];

    for level in (0..levels).rev() {
        let (ll_w, ll_h) = dims[level];
        inverse_ll_region_conformant(data, width, ll_w, ll_h, &mut row_scratch, &mut col_scratch);
    }
}

/// Forward transform of one LL sub-region (conformant). Degenerate rows or
/// columns (length < 2) are left unchanged by the 1-D transform.
#[allow(dead_code)]
fn forward_ll_region_conformant(
    data: &mut [i32],
    stride: usize,
    width: usize,
    height: usize,
    row_scratch: &mut [i32],
    col_scratch: &mut [i32],
) {
    // Columns first, then rows. The reversible 5/3 lifting uses floor rounding,
    // so the 2-D pass order is observable in the integer coefficients; T.800 /
    // OpenJPEG transform columns (vertical) before rows (horizontal) on the
    // forward transform (the inverse undoes them in the opposite order). Coding
    // rows first here produced off-by-one coefficients versus OpenJPEG.
    let col = &mut col_scratch[..height];
    for x in 0..width {
        for (y, slot) in col.iter_mut().enumerate() {
            *slot = data[y * stride + x];
        }
        forward_1d(col);
        for (y, &v) in col.iter().enumerate() {
            data[y * stride + x] = v;
        }
    }

    for y in 0..height {
        let off = y * stride;
        let row = &mut row_scratch[..width];
        row.copy_from_slice(&data[off..off + width]);
        forward_1d(row);
        data[off..off + width].copy_from_slice(row);
    }
}

/// Inverse transform of one LL sub-region (conformant).
#[allow(dead_code)]
fn inverse_ll_region_conformant(
    data: &mut [i32],
    stride: usize,
    width: usize,
    height: usize,
    row_scratch: &mut [i32],
    col_scratch: &mut [i32],
) {
    // Rows first, then columns -- the exact reverse of the forward pass order
    // (columns-then-rows), so this is its true inverse and matches OpenJPEG's
    // inverse transform.
    for y in 0..height {
        let off = y * stride;
        let row = &mut row_scratch[..width];
        row.copy_from_slice(&data[off..off + width]);
        inverse_1d(row);
        data[off..off + width].copy_from_slice(row);
    }

    let col = &mut col_scratch[..height];
    for x in 0..width {
        for (y, slot) in col.iter_mut().enumerate() {
            *slot = data[y * stride + x];
        }
        inverse_1d(col);
        for (y, &v) in col.iter().enumerate() {
            data[y * stride + x] = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_inverse_1d_roundtrip() {
        let original = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let mut data = original.clone();
        forward_1d(&mut data);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_1d_odd_length() {
        let original = vec![5, 15, 25, 35, 45];
        let mut data = original.clone();
        forward_1d(&mut data);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_1d_length_2() {
        let original = vec![100, 200];
        let mut data = original.clone();
        forward_1d(&mut data);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_1d_length_3() {
        let original = vec![7, 13, 42];
        let mut data = original.clone();
        forward_1d(&mut data);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_1d_length_1_noop() {
        let original = vec![42];
        let mut data = original.clone();
        forward_1d(&mut data);
        assert_eq!(data, original);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_2d_roundtrip() {
        let original: Vec<i32> = (0..64).collect();
        let mut data = original.clone();
        forward_2d(&mut data, 8, 8);
        inverse_2d(&mut data, 8, 8);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_2d_non_square() {
        let w = 6;
        let h = 4;
        let original: Vec<i32> = (0..(w * h) as i32).collect();
        let mut data = original.clone();
        forward_2d(&mut data, w, h);
        inverse_2d(&mut data, w, h);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_multi_level_roundtrip() {
        let w = 16;
        let h = 16;
        let original: Vec<i32> = (0..(w * h) as i32).collect();
        let mut data = original.clone();
        let (ll_w, ll_h) = forward_multi_level(&mut data, w, h, 3);
        assert!(ll_w > 0 && ll_h > 0);
        inverse_multi_level(&mut data, w, h, 3);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_multi_level_5_levels() {
        let w = 64;
        let h = 64;
        let original: Vec<i32> = (0..(w * h) as i32).map(|x| x % 256).collect();
        let mut data = original.clone();
        forward_multi_level(&mut data, w, h, 5);
        inverse_multi_level(&mut data, w, h, 5);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_multi_level_odd_dims() {
        let w = 13;
        let h = 11;
        let original: Vec<i32> = (0..(w * h) as i32).collect();
        let mut data = original.clone();
        forward_multi_level(&mut data, w, h, 3);
        inverse_multi_level(&mut data, w, h, 3);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_inverse_multi_level_large_values() {
        let w = 32;
        let h = 32;
        let original: Vec<i32> = (0..(w * h) as i32).map(|x| x * 100 + 5000).collect();
        let mut data = original.clone();
        forward_multi_level(&mut data, w, h, 4);
        inverse_multi_level(&mut data, w, h, 4);
        assert_eq!(data, original);
    }

    #[test]
    fn forward_1d_all_zeros() {
        let mut data = vec![0i32; 8];
        forward_1d(&mut data);
        assert!(data.iter().all(|&x| x == 0));
    }

    #[test]
    fn forward_1d_constant() {
        let mut data = vec![42i32; 8];
        let original = data.clone();
        forward_1d(&mut data);
        inverse_1d(&mut data);
        assert_eq!(data, original);
    }

    // -- Brute-force T.800 F.4.8.2.1 reference --------------------------------

    /// Whole-sample symmetric extension: reflect `i` into `[0, n)`.
    fn mirror(n: isize, i: isize) -> usize {
        if n == 1 {
            return 0;
        }
        let period = 2 * (n - 1);
        let mut k = i.rem_euclid(period);
        if k >= n {
            k = period - k;
        }
        k as usize
    }

    /// Direct (non-lifting-factored) reference of the reversible 5/3 forward
    /// transform, using floor division via `div_euclid`, in the same
    /// deinterleaved `[lows.., highs..]` layout as `forward_1d`.
    fn ref_forward_1d(x: &[i32]) -> Vec<i32> {
        let n = x.len();
        if n < 2 {
            return x.to_vec();
        }
        let half = n.div_ceil(2);
        let num_high = n - half;
        let ni = n as isize;

        // Interleaved output Y.
        let mut y = vec![0i32; n];

        // High-pass: Y[2i+1] = X[2i+1] - floor((X[2i] + X[2i+2]) / 2).
        for i in 0..num_high {
            let idx = 2 * i + 1;
            let a = x[mirror(ni, idx as isize - 1)];
            let b = x[mirror(ni, idx as isize + 1)];
            y[idx] = x[idx] - (a + b).div_euclid(2);
        }

        // Low-pass: Y[2i] = X[2i] + floor((Y[2i-1] + Y[2i+1] + 2) / 4).
        for i in 0..half {
            let idx = 2 * i;
            let a = y[mirror(ni, idx as isize - 1)];
            let b = y[mirror(ni, idx as isize + 1)];
            y[idx] = x[idx] + (a + b + 2).div_euclid(4);
        }

        // Deinterleave into lows then highs.
        let mut out = Vec::with_capacity(n);
        for i in 0..half {
            out.push(y[2 * i]);
        }
        for i in 0..num_high {
            out.push(y[2 * i + 1]);
        }
        out
    }

    fn signed_ramp(n: usize) -> Vec<i32> {
        // Includes negatives and asymmetry so floor != truncation shows up.
        (0..n as i32)
            .map(|i| (i * 7 - 3 * n as i32) * ((i % 3) - 1))
            .collect()
    }

    #[test]
    fn forward_1d_matches_reference_with_negatives() {
        for n in 1..=17usize {
            let x = signed_ramp(n);
            let mut got = x.clone();
            forward_1d(&mut got);
            assert_eq!(got, ref_forward_1d(&x), "forward mismatch at n={n}");
        }
    }

    #[test]
    fn inverse_1d_inverts_reference_with_negatives() {
        for n in 1..=17usize {
            let x = signed_ramp(n);
            // inverse_1d must invert both forward_1d and the reference forward.
            let mut a = x.clone();
            forward_1d(&mut a);
            inverse_1d(&mut a);
            assert_eq!(a, x, "inverse(forward) mismatch at n={n}");

            let mut b = ref_forward_1d(&x);
            inverse_1d(&mut b);
            assert_eq!(b, x, "inverse(ref_forward) mismatch at n={n}");
        }
    }

    #[test]
    fn forward_1d_uses_floor_not_truncation() {
        // Adjacent even (low) samples -1 and -2 sum to -3 (negative, odd), so
        // the predict step's floor `>>1` (= -2) differs from truncation (= -1).
        let x = vec![-1i32, 5, -2];
        let mut floored = x.clone();
        forward_1d(&mut floored);
        assert_eq!(floored, ref_forward_1d(&x));
        // Legacy (truncating) path must differ on this input.
        let mut trunc = x.clone();
        legacy_forward_1d(&mut trunc);
        assert_ne!(
            floored, trunc,
            "floor and truncation must differ on negative-odd sums"
        );
    }

    // -- Conformant multi-level paths ----------------------------------------

    fn conformant_roundtrip(w: usize, h: usize, levels: usize, seed: i32) {
        let original: Vec<i32> = (0..(w * h) as i32)
            .map(|i| (i.wrapping_mul(seed) % 4096) - 2048)
            .collect();
        let mut data = original.clone();
        forward_multi_level_conformant(&mut data, w, h, levels);
        inverse_multi_level_conformant(&mut data, w, h, levels);
        assert_eq!(data, original, "roundtrip failed for {w}x{h} @ {levels}L");
    }

    #[test]
    fn conformant_roundtrip_degenerate_13x7_5_levels() {
        conformant_roundtrip(13, 7, 5, 31);
    }

    #[test]
    fn conformant_roundtrip_1x16() {
        conformant_roundtrip(1, 16, 4, 17);
    }

    #[test]
    fn conformant_roundtrip_16x1() {
        conformant_roundtrip(16, 1, 4, 23);
    }

    #[test]
    fn conformant_roundtrip_1x1_noop() {
        let original = vec![42i32];
        let mut data = original.clone();
        forward_multi_level_conformant(&mut data, 1, 1, 3);
        assert_eq!(data, original, "1x1 forward must be a no-op");
        inverse_multi_level_conformant(&mut data, 1, 1, 3);
        assert_eq!(data, original);
    }

    #[test]
    fn conformant_roundtrip_square_and_odd() {
        conformant_roundtrip(64, 64, 5, 3);
        conformant_roundtrip(17, 13, 3, 5);
        conformant_roundtrip(2, 2, 1, 9);
    }

    #[test]
    fn conformant_transforms_degenerate_region() {
        // A 1x4 column region has a length-4 column that MUST be transformed
        // (the legacy path would stop early and leave it untouched).
        let original = vec![10i32, 20, 30, 40];
        let mut data = original.clone();
        forward_multi_level_conformant(&mut data, 1, 4, 1);
        assert_ne!(data, original, "1x4 column must be transformed");
        inverse_multi_level_conformant(&mut data, 1, 4, 1);
        assert_eq!(data, original);
    }
}
