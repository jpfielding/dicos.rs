//! Reversible Color Transform (ITU-T T.800 Annex G).
//!
//! For single-component (grayscale) images this is the identity transform.
//! Provided for completeness; the DICOS codec uses single-component images.

/// Forward RCT: RGB -> YCbCr (reversible).
///
/// `r`, `g`, `b` are modified in place to become Y, Cb, Cr respectively.
pub fn forward_rct_in_place(r: &mut [i32], g: &mut [i32], b: &mut [i32]) {
    for i in 0..r.len() {
        let ri = r[i];
        let gi = g[i];
        let bi = b[i];
        r[i] = (ri + 2 * gi + bi) >> 2; // Y
        g[i] = bi - gi; // Cb
        b[i] = ri - gi; // Cr
    }
}

/// Inverse RCT: YCbCr -> RGB (reversible).
///
/// `y`, `cb`, `cr` are modified in place to become R, G, B respectively.
pub fn inverse_rct_in_place(y: &mut [i32], cb: &mut [i32], cr: &mut [i32]) {
    for i in 0..y.len() {
        let yi = y[i];
        let cbi = cb[i];
        let cri = cr[i];
        let g = yi - ((cbi + cri) >> 2);
        y[i] = cri + g; // R
        cb[i] = g; // G
        cr[i] = cbi + g; // B
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rct_roundtrip() {
        let mut r = vec![100, 200, 50, 0, 255];
        let mut g = vec![150, 100, 200, 128, 64];
        let mut b = vec![200, 50, 100, 255, 0];

        let orig_r = r.clone();
        let orig_g = g.clone();
        let orig_b = b.clone();

        forward_rct_in_place(&mut r, &mut g, &mut b);
        inverse_rct_in_place(&mut r, &mut g, &mut b);

        assert_eq!(r, orig_r);
        assert_eq!(g, orig_g);
        assert_eq!(b, orig_b);
    }

    #[test]
    fn rct_identity_grayscale() {
        // For a single-component image the RCT is not applied.
        // This test verifies the logic that the transform is reversible
        // even when all components are equal (grayscale-like).
        let mut r = vec![128; 10];
        let mut g = vec![128; 10];
        let mut b = vec![128; 10];

        forward_rct_in_place(&mut r, &mut g, &mut b);
        // Y = (128 + 256 + 128)/4 = 128, Cb = 0, Cr = 0
        assert!(r.iter().all(|&v| v == 128));
        assert!(g.iter().all(|&v| v == 0));
        assert!(b.iter().all(|&v| v == 0));

        inverse_rct_in_place(&mut r, &mut g, &mut b);
        assert!(r.iter().all(|&v| v == 128));
        assert!(g.iter().all(|&v| v == 128));
        assert!(b.iter().all(|&v| v == 128));
    }
}
