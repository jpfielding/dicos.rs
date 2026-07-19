//! Conformant T.800 tile-component pipeline (Workstream 1 step 8).
//!
//! Encodes/decodes a single tile-component through the full lossless pipeline:
//! reversible 5/3 DWT ⇄ resolution/band/code-block geometry ⇄ EBCOT tier-1 ⇄
//! LRCP tier-2 packets. For our profile (1 tile, 1 component, 1 layer, 1
//! precinct per band, LRCP progression) the packet order is simply resolution
//! `r` ascending, one packet per resolution covering that resolution's bands.
//!
//! `Mb` (the maximum number of magnitude bit-planes for a band, `Mb = εb + G −
//! 1`) is supplied per band by the `mb_for(kind, level)` closure, where `level`
//! is the band's *decomposition level* (`NL` for the LL band, `NL..=1` for the
//! detail bands) — the same key the QCD marker uses to order its step sizes.
//! Step 9 (codestream) will pass a closure computed from the parsed `QcdMarker`;
//! the tests here use a constant.
//!
//! This module is not yet wired into the public `encode`/`decode` (still the
//! frozen `legacy` path) — that is Workstream 1 step 9.

use crate::dwt;
use crate::ebcot::{decode_code_block, encode_code_block, CodedBlock};
use crate::error::CodecError;
use crate::geometry::{build_geometry, Band, BandKind, CodeBlockGeom};
use crate::packet::{read_packet, write_packet, PrecinctState};

/// Maximum tile area (samples) we will allocate for, guarding against hostile
/// dimensions (matches the codestream-level cap in the plan).
const MAX_TILE_AREA: usize = 1 << 28;

/// Decomposition level of a resolution's bands: `NL` for the coarsest
/// resolution (`r = 0`, the LL band) and for `r = 1`, decreasing to `1` at the
/// finest resolution. This mirrors `geometry`'s `n = NL − r + 1`.
fn decomposition_level(r: u8, num_levels: u8) -> u8 {
    if r == 0 {
        num_levels
    } else {
        num_levels - r + 1
    }
}

/// Validate dimensions and return the sample count `w * h`.
fn checked_area(w: u32, h: u32) -> Result<usize, CodecError> {
    if w == 0 || h == 0 {
        return Err(CodecError::InvalidData("tile has a zero dimension".into()));
    }
    let n = (w as usize)
        .checked_mul(h as usize)
        .ok_or_else(|| CodecError::InvalidData("tile dimensions overflow usize".into()))?;
    if n > MAX_TILE_AREA {
        return Err(CodecError::InvalidData(format!(
            "tile area {n} exceeds the {MAX_TILE_AREA} sample cap"
        )));
    }
    Ok(n)
}

/// Gather a code-block's coefficients out of the packed Mallat tile buffer.
///
/// The block rectangle is band-native; its physical position in the full tile
/// buffer is `band.placement + cb.rect.(x0, y0)`, indexed at the full tile
/// `stride`.
fn gather(buf: &[i32], stride: usize, band: &Band, cb: &CodeBlockGeom) -> Vec<i32> {
    let bw = cb.rect.width() as usize;
    let bh = cb.rect.height() as usize;
    let x_off = band.placement.x_off as usize + cb.rect.x0 as usize;
    let y_off = band.placement.y_off as usize + cb.rect.y0 as usize;
    let mut v = vec![0i32; bw * bh];
    for yy in 0..bh {
        let src = (y_off + yy) * stride + x_off;
        v[yy * bw..yy * bw + bw].copy_from_slice(&buf[src..src + bw]);
    }
    v
}

/// Scatter a code-block's decoded coefficients back into the Mallat buffer.
fn scatter(buf: &mut [i32], stride: usize, band: &Band, cb: &CodeBlockGeom, coeffs: &[i32]) {
    let bw = cb.rect.width() as usize;
    let bh = cb.rect.height() as usize;
    let x_off = band.placement.x_off as usize + cb.rect.x0 as usize;
    let y_off = band.placement.y_off as usize + cb.rect.y0 as usize;
    for yy in 0..bh {
        let dst = (y_off + yy) * stride + x_off;
        buf[dst..dst + bw].copy_from_slice(&coeffs[yy * bw..yy * bw + bw]);
    }
}

/// Encode one tile-component (`w × h` samples, row-major) to a concatenation of
/// LRCP packets.
///
/// `mb_for(kind, level)` supplies `Mb` per band. An `Err` is returned if any
/// code-block needs more magnitude bit-planes than its band's `Mb` allows.
pub fn encode_tile(
    component: &[i32],
    w: u32,
    h: u32,
    num_levels: u8,
    xcb: u8,
    ycb: u8,
    mb_for: impl Fn(BandKind, u8) -> u32,
) -> Result<Vec<u8>, CodecError> {
    let n = checked_area(w, h)?;
    if component.len() != n {
        return Err(CodecError::DimensionMismatch {
            expected: n,
            actual: component.len(),
        });
    }
    let stride = w as usize;

    // Forward DWT into a packed Mallat buffer.
    let mut buf = component.to_vec();
    dwt::forward_multi_level_conformant(&mut buf, stride, h as usize, num_levels as usize);

    let geom = build_geometry(w, h, num_levels, xcb, ycb);
    let mut out = Vec::new();

    for res in &geom.resolutions {
        let level = decomposition_level(res.r, num_levels);
        let bands: Vec<Band> = res.bands.clone();
        let mut states: Vec<PrecinctState> = bands
            .iter()
            .map(|b| PrecinctState::new(b.cb_grid_w, b.cb_grid_h))
            .collect();
        let mut mbs: Vec<u32> = Vec::with_capacity(bands.len());
        let mut all_blocks: Vec<Vec<Option<CodedBlock>>> = Vec::with_capacity(bands.len());

        for band in &bands {
            let mb = mb_for(band.kind, level);
            mbs.push(mb);
            let mut blocks: Vec<Option<CodedBlock>> = Vec::with_capacity(band.cbs.len());
            for cb in &band.cbs {
                let bw = cb.rect.width() as usize;
                let bh = cb.rect.height() as usize;
                let coeffs = gather(&buf, stride, band, cb);
                let coded = encode_code_block(&coeffs, bw, bh, band.kind);
                // `mb` is caller-supplied (from QCD at step 9), so a too-small
                // Mb is a runtime error, not an internal invariant — no
                // debug_assert here, which would mask the clean Err in tests.
                if coded.num_bitplanes > mb {
                    return Err(CodecError::InvalidData(format!(
                        "code-block needs {} bit-planes but Mb is {mb} (band {:?})",
                        coded.num_bitplanes, band.kind
                    )));
                }
                blocks.push(if coded.num_bitplanes == 0 {
                    None
                } else {
                    Some(coded)
                });
            }
            all_blocks.push(blocks);
        }

        let block_refs: Vec<&[Option<CodedBlock>]> =
            all_blocks.iter().map(|v| v.as_slice()).collect();
        write_packet(&mut states, &bands, &block_refs, &mbs, &mut out)?;
    }

    Ok(out)
}

/// Decode a tile-component from a concatenation of LRCP packets.
pub fn decode_tile(
    bitstream: &[u8],
    w: u32,
    h: u32,
    num_levels: u8,
    xcb: u8,
    ycb: u8,
    mb_for: impl Fn(BandKind, u8) -> u32,
) -> Result<Vec<i32>, CodecError> {
    let n = checked_area(w, h)?;
    let stride = w as usize;

    let geom = build_geometry(w, h, num_levels, xcb, ycb);
    let mut buf = vec![0i32; n];
    let mut pos = 0usize;

    for res in &geom.resolutions {
        let level = decomposition_level(res.r, num_levels);
        let bands: Vec<Band> = res.bands.clone();
        let mut states: Vec<PrecinctState> = bands
            .iter()
            .map(|b| PrecinctState::new(b.cb_grid_w, b.cb_grid_h))
            .collect();
        let mbs: Vec<u32> = bands.iter().map(|b| mb_for(b.kind, level)).collect();

        if pos > bitstream.len() {
            return Err(CodecError::InvalidData("tile bitstream truncated".into()));
        }
        let (contribs, header_len) = read_packet(&mut states, &bands, &mbs, &bitstream[pos..])?;
        let mut body_pos = pos
            .checked_add(header_len)
            .ok_or_else(|| CodecError::InvalidData("packet offset overflow".into()))?;

        for (bi, band) in bands.iter().enumerate() {
            let mb = mbs[bi];
            for (k, cb) in band.cbs.iter().enumerate() {
                let bw = cb.rect.width() as usize;
                let bh = cb.rect.height() as usize;
                let coeffs = match &contribs[bi][k] {
                    None => vec![0i32; bw * bh],
                    Some(c) => {
                        if c.zero_bitplanes > mb {
                            return Err(CodecError::InvalidData(format!(
                                "zero-bit-planes {} exceed Mb {mb}",
                                c.zero_bitplanes
                            )));
                        }
                        let num_bitplanes = mb - c.zero_bitplanes;
                        if num_bitplanes == 0 {
                            return Err(CodecError::InvalidData(
                                "included block with zero magnitude bit-planes".into(),
                            ));
                        }
                        let max_passes = 3 * num_bitplanes - 2;
                        if c.num_passes > max_passes {
                            return Err(CodecError::InvalidData(format!(
                                "num_passes {} exceeds 3*{num_bitplanes}-2 = {max_passes}",
                                c.num_passes
                            )));
                        }
                        let end = body_pos.checked_add(c.len).ok_or_else(|| {
                            CodecError::InvalidData("body offset overflow".into())
                        })?;
                        if end > bitstream.len() {
                            return Err(CodecError::InvalidData(
                                "code-block body truncated".into(),
                            ));
                        }
                        let data = &bitstream[body_pos..end];
                        body_pos = end;
                        decode_code_block(data, bw, bh, band.kind, num_bitplanes, c.num_passes)?
                    }
                };
                scatter(&mut buf, stride, band, cb, &coeffs);
            }
        }
        pos = body_pos;
    }

    dwt::inverse_multi_level_conformant(&mut buf, stride, h as usize, num_levels as usize);
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A comfortable constant Mb: 16-bit-ish input through a few DWT levels
    /// never needs this many magnitude bit-planes, so encode never overflows.
    /// Step 9 will supply real per-band values from the QCD marker.
    fn mb40(_: BandKind, _: u8) -> u32 {
        40
    }

    fn roundtrip(component: &[i32], w: u32, h: u32, nl: u8, xcb: u8, ycb: u8) {
        let enc = encode_tile(component, w, h, nl, xcb, ycb, mb40)
            .unwrap_or_else(|e| panic!("encode {w}x{h} nl={nl} cb=({xcb},{ycb}): {e}"));
        let dec = decode_tile(&enc, w, h, nl, xcb, ycb, mb40)
            .unwrap_or_else(|e| panic!("decode {w}x{h} nl={nl} cb=({xcb},{ycb}): {e}"));
        assert_eq!(
            dec, component,
            "round-trip mismatch {w}x{h} nl={nl} cb=({xcb},{ycb})"
        );
    }

    /// Deterministic pixel patterns, values kept < 4096 (12-bit) so the DWT
    /// output stays well within Mb=40.
    fn pattern(kind: usize, w: u32, h: u32) -> Vec<i32> {
        let n = (w * h) as usize;
        match kind {
            0 => vec![1234i32; n], // constant
            1 => (0..n as u32) // gradient
                .map(|i| ((i % w) as i32 * 3 + (i / w) as i32 * 5) % 4096)
                .collect(),
            2 => {
                // seeded pseudo-random
                let mut s = 0x9E37_79B9_7F4A_7C15u64 ^ ((w as u64) << 20) ^ (h as u64);
                (0..n)
                    .map(|_| {
                        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                        ((s >> 40) as i32) & 0xFFF
                    })
                    .collect()
            }
            _ => {
                // sparse
                let mut v = vec![0i32; n];
                if n > 0 {
                    v[0] = 2000;
                    v[n / 2] = -1500;
                    v[n - 1] = 999;
                }
                v
            }
        }
    }

    #[test]
    fn roundtrip_matrix() {
        let dims = [1u32, 2, 3, 5, 8, 13, 16, 31, 64, 65, 130];
        let levels = [0u8, 1, 2, 5];
        let cbs = [(2u8, 2u8), (6, 6), (10, 2)];
        for &w in &dims {
            for &h in &dims {
                for &nl in &levels {
                    for &(xcb, ycb) in &cbs {
                        for kind in 0..4 {
                            let comp = pattern(kind, w, h);
                            roundtrip(&comp, w, h, nl, xcb, ycb);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_zero_image_empty_packets_to_zeros() {
        let w = 64;
        let h = 48;
        let comp = vec![0i32; (w * h) as usize];
        let enc = encode_tile(&comp, w, h, 5, 6, 6, mb40).unwrap();
        // Every resolution's packet is empty (a single 0x00 byte). With NL=5
        // there are 6 resolutions.
        assert_eq!(enc, vec![0x00; 6]);
        let dec = decode_tile(&enc, w, h, 5, 6, 6, mb40).unwrap();
        assert_eq!(dec, comp);
    }

    #[test]
    fn tight_mb_equal_to_actual_bitplanes() {
        // Discover the minimal Mb for which encode succeeds (== global max
        // num_bitplanes), then round-trip at exactly that Mb.
        let w = 31;
        let h = 17;
        let comp = pattern(2, w, h);
        let nl = 3u8;
        let (xcb, ycb) = (4u8, 4u8);

        let tight = (0..=48u32)
            .find(|&mb| encode_tile(&comp, w, h, nl, xcb, ycb, |_, _| mb).is_ok())
            .expect("some Mb must work");
        assert!(tight > 0, "non-zero image needs at least one bit-plane");

        let enc = encode_tile(&comp, w, h, nl, xcb, ycb, |_, _| tight).unwrap();
        let dec = decode_tile(&enc, w, h, nl, xcb, ycb, |_, _| tight).unwrap();
        assert_eq!(dec, comp, "tight-Mb round-trip");

        // One below the tight value must fail cleanly at encode time.
        let err = encode_tile(&comp, w, h, nl, xcb, ycb, |_, _| tight - 1);
        assert!(matches!(err, Err(CodecError::InvalidData(_))));
    }

    #[test]
    fn mismatched_mb_decode_errors_cleanly() {
        let w = 32;
        let h = 32;
        let comp = pattern(1, w, h);
        let nl = 2u8;
        let enc = encode_tile(&comp, w, h, nl, 6, 6, mb40).unwrap();
        // Decoding with an Mb far smaller than the encoded bit-planes must
        // error, not panic and not silently corrupt.
        let dec = decode_tile(&enc, w, h, nl, 6, 6, |_, _| 5);
        assert!(matches!(dec, Err(CodecError::InvalidData(_))));
    }

    #[test]
    fn truncated_bitstream_never_panics() {
        let w = 16;
        let h = 13;
        let comp = pattern(2, w, h);
        let nl = 2u8;
        let enc = encode_tile(&comp, w, h, nl, 4, 4, mb40).unwrap();
        for cut in 0..enc.len() {
            // Must return Ok or Err but never panic.
            let _ = decode_tile(&enc[..cut], w, h, nl, 4, 4, mb40);
        }
        // The full stream decodes.
        assert_eq!(decode_tile(&enc, w, h, nl, 4, 4, mb40).unwrap(), comp);
    }

    #[test]
    fn zero_dimension_rejected() {
        assert!(matches!(
            encode_tile(&[], 0, 4, 0, 6, 6, mb40),
            Err(CodecError::InvalidData(_))
        ));
        assert!(matches!(
            decode_tile(&[0], 4, 0, 0, 6, 6, mb40),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let comp = vec![0i32; 10];
        assert!(matches!(
            encode_tile(&comp, 4, 4, 0, 6, 6, mb40),
            Err(CodecError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn single_pixel() {
        for v in [0i32, 1, 2047, -2047] {
            roundtrip(&[v], 1, 1, 0, 2, 2);
            roundtrip(&[v], 1, 1, 3, 6, 6);
        }
    }
}
