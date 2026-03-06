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
    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left }; // symmetric extension
        high[i] -= (left + right) / 2;
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
        low[i] += (left + right + 2) / 4;
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
        low[i] -= (left + right + 2) / 4;
    }

    // Inverse predict step.
    for i in 0..num_high {
        let left = low[i];
        let right = if i + 1 < half { low[i + 1] } else { left };
        high[i] += (left + right) / 2;
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
// 2-D transforms (single level)
// ---------------------------------------------------------------------------

/// Forward 2-D 5/3 DWT -- produces LL, HL, LH, HH subbands (in-place).
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

/// Forward multi-level 2-D DWT.
///
/// Each level transforms the LL subband from the previous level.
/// Returns the (width, height) of the final LL subband.
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

fn forward_ll_region(data: &mut [i32], stride: usize, width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Transform rows in the region.
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * stride;
        row.copy_from_slice(&data[off..off + width]);
        forward_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
    }

    // Transform columns in the region.
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * stride + x];
        }
        forward_1d(&mut col);
        for y in 0..height {
            data[y * stride + x] = col[y];
        }
    }
}

fn inverse_ll_region(data: &mut [i32], stride: usize, width: usize, height: usize) {
    if width < 2 || height < 2 {
        return;
    }

    // Inverse columns first.
    let mut col = vec![0i32; height];
    for x in 0..width {
        for y in 0..height {
            col[y] = data[y * stride + x];
        }
        inverse_1d(&mut col);
        for y in 0..height {
            data[y * stride + x] = col[y];
        }
    }

    // Inverse rows.
    let mut row = vec![0i32; width];
    for y in 0..height {
        let off = y * stride;
        row.copy_from_slice(&data[off..off + width]);
        inverse_1d(&mut row);
        data[off..off + width].copy_from_slice(&row);
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
}
