//! Tier-2 packet-header codec (ITU-T T.800 B.10), single quality layer.
//!
//! A JPEG 2000 packet is a byte-aligned header followed by the concatenated
//! code-block bodies. For our profile (LRCP, 1 layer, 1 component, 1 precinct
//! per band) one packet is written per resolution, covering that resolution's
//! sub-bands in order (LL at r=0; HL, LH, HH at r>0).
//!
//! The header packs bits MSB-first with the B.10.1 bit-stuffing convention: a
//! byte equal to `0xFF` is followed by a byte that carries only 7 data bits
//! (its most-significant bit is forced to 0), so a header can never contain a
//! spurious in-band marker (`0xFF90`..`0xFFFF`). A header also never *ends*
//! with `0xFF`: if the last complete byte is `0xFF` a `0x00` stuffing byte is
//! appended before the packet body.
//!
//! Two tag trees per precinct carry inclusion (B.10.4) and zero-bit-plane
//! counts (B.10.5); pass counts use the B.10.6 comma/prefix code and lengths
//! the B.10.7 `Lblock` signalling. State ([`PrecinctState`]) persists across
//! packets that share a precinct so tag-tree and `Lblock` refinements are
//! incremental.
//!
//! NOTE (deviation from the plan prose): the number-of-passes code follows the
//! *actual* T.800 Table B.4 (identical to OpenJPEG's `opj_t2_{put,get}numpasses`)
//! — `11`+2 bits for 3..=5, `1111`+5 bits for 6..=36, `111111111`+7 bits for
//! 37..=164 — rather than the approximate prose in the plan, so the codestream
//! stays byte-conformant for the step-10 OpenJPEG interop.

use crate::ebcot::CodedBlock;
use crate::error::CodecError;
use crate::geometry::Band;
use crate::tagtree::TagTree;

// ---------------------------------------------------------------------------
// B.10.1 stuffing bit I/O
// ---------------------------------------------------------------------------

/// MSB-first bit writer with B.10.1 `0xFF` stuffing.
pub struct PacketBitWriter {
    bytes: Vec<u8>,
    /// Partial byte being assembled (bits shifted in from the LSB).
    cur: u8,
    /// Number of bits already placed into `cur`.
    filled: u8,
    /// Capacity of the current byte: 8, or 7 right after a `0xFF` byte.
    cap: u8,
}

impl Default for PacketBitWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketBitWriter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            filled: 0,
            cap: 8,
        }
    }

    /// Append one bit (only bit 0 of `bit` is used).
    pub fn put_bit(&mut self, bit: u8) {
        self.cur = (self.cur << 1) | (bit & 1);
        self.filled += 1;
        if self.filled == self.cap {
            self.emit();
        }
    }

    /// Append the low `n` bits of `val`, MSB-first. `n` must be `<= 64`.
    pub fn put_bits(&mut self, val: u64, n: u32) {
        for i in (0..n).rev() {
            self.put_bit(((val >> i) & 1) as u8);
        }
    }

    fn emit(&mut self) {
        let byte = self.cur;
        self.bytes.push(byte);
        self.cur = 0;
        self.filled = 0;
        // After a 0xFF byte the next byte carries only 7 data bits.
        self.cap = if byte == 0xFF { 7 } else { 8 };
    }

    /// Flush the final partial byte (zero-padded) and enforce the "a header
    /// never ends with 0xFF" rule by appending a `0x00` stuffing byte when the
    /// last complete byte is `0xFF`. Returns the finished header bytes.
    pub fn finish(mut self) -> Vec<u8> {
        if self.filled > 0 {
            // Left-justify the accumulated bits and zero-pad to `cap` bits.
            self.cur <<= self.cap - self.filled;
            self.emit();
        }
        if self.bytes.last() == Some(&0xFF) {
            self.bytes.push(0x00);
        }
        self.bytes
    }
}

/// MSB-first bit reader mirroring [`PacketBitWriter`], with strict bounds
/// checks (truncation surfaces as [`CodecError::InvalidData`], never a panic).
pub struct PacketBitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte to load.
    pos: usize,
    /// Current loaded byte.
    cur: u8,
    /// Number of unread valid bits remaining in `cur`.
    avail: u8,
    /// Whether the previously loaded byte was `0xFF` (⇒ next byte holds 7 bits).
    prev_ff: bool,
}

impl<'a> PacketBitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            cur: 0,
            avail: 0,
            prev_ff: false,
        }
    }

    fn load_next(&mut self) -> Result<(), CodecError> {
        if self.pos >= self.data.len() {
            return Err(CodecError::InvalidData(
                "packet header truncated".to_string(),
            ));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        self.cur = b;
        self.avail = if self.prev_ff { 7 } else { 8 };
        self.prev_ff = b == 0xFF;
        Ok(())
    }

    /// Read a single bit (MSB-first).
    pub fn read_bit(&mut self) -> Result<u32, CodecError> {
        if self.avail == 0 {
            self.load_next()?;
        }
        self.avail -= 1;
        Ok(((self.cur >> self.avail) & 1) as u32)
    }

    /// Read `n` bits (MSB-first) into a `u64`. `n` must be `<= 64`.
    pub fn read_bits(&mut self, n: u32) -> Result<u64, CodecError> {
        debug_assert!(n <= 64);
        let mut v = 0u64;
        for _ in 0..n {
            v = (v << 1) | self.read_bit()? as u64;
        }
        Ok(v)
    }

    /// Number of header bytes consumed, applying the B.10.1 end-of-header
    /// alignment: if we stopped exactly at the end of a `0xFF` byte, the
    /// appended `0x00` stuffing byte is part of the header too.
    pub fn consumed(&self) -> Result<usize, CodecError> {
        let mut n = self.pos;
        if self.avail == 0 && self.pos > 0 && self.data[self.pos - 1] == 0xFF {
            // A stuffed 0x00 must follow the trailing 0xFF.
            if self.pos >= self.data.len() {
                return Err(CodecError::InvalidData(
                    "packet header truncated at stuffing byte".to_string(),
                ));
            }
            n += 1;
        }
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Number-of-passes code (T.800 Table B.4)
// ---------------------------------------------------------------------------

fn write_num_passes(bw: &mut PacketBitWriter, n: u32) {
    debug_assert!((1..=164).contains(&n), "num_passes out of range");
    if n == 1 {
        bw.put_bit(0);
    } else if n == 2 {
        bw.put_bits(0b10, 2);
    } else if n <= 5 {
        bw.put_bits(0xc | (n - 3) as u64, 4);
    } else if n <= 36 {
        bw.put_bits(0x1e0 | (n - 6) as u64, 9);
    } else {
        bw.put_bits(0xff80 | (n - 37) as u64, 16);
    }
}

fn read_num_passes(br: &mut PacketBitReader) -> Result<u32, CodecError> {
    if br.read_bit()? == 0 {
        return Ok(1);
    }
    if br.read_bit()? == 0 {
        return Ok(2);
    }
    let f = br.read_bits(2)? as u32;
    if f != 3 {
        return Ok(3 + f);
    }
    let f = br.read_bits(5)? as u32;
    if f != 31 {
        return Ok(6 + f);
    }
    let f = br.read_bits(7)? as u32;
    Ok(37 + f) // max 37 + 127 = 164
}

/// `⌊log2(v)⌋` for `v >= 1`.
fn floor_log2(v: u32) -> u32 {
    debug_assert!(v > 0);
    31 - v.leading_zeros()
}

// ---------------------------------------------------------------------------
// Precinct state
// ---------------------------------------------------------------------------

/// Per-precinct persistent packet-coding state. In our profile there is exactly
/// one precinct per band per resolution, so one `PrecinctState` is kept per
/// [`Band`], sized to that band's code-block grid.
pub struct PrecinctState {
    /// Inclusion tag tree (leaf = first layer the block is included in).
    pub incl: TagTree,
    /// Zero-bit-plane tag tree (leaf = `Mb - num_bitplanes`).
    pub zbp: TagTree,
    /// `Lblock` per code-block (raster order), initialised to 3 (B.10.7.1).
    pub lblock: Vec<u8>,
    /// Whether each code-block has already been included in an earlier layer.
    pub first_layer_included: Vec<bool>,
}

impl PrecinctState {
    /// Build state for a `grid_w × grid_h` code-block grid.
    pub fn new(grid_w: u32, grid_h: u32) -> Self {
        let n = (grid_w as usize) * (grid_h as usize);
        Self {
            incl: TagTree::new(grid_w, grid_h),
            zbp: TagTree::new(grid_w, grid_h),
            lblock: vec![3u8; n],
            first_layer_included: vec![false; n],
        }
    }
}

/// One code-block's contribution recovered from a packet header. The body slice
/// of `len` bytes is taken by the caller from the bytes following the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockContribution {
    /// Number of coding passes signalled for this block.
    pub num_passes: u32,
    /// Number of zero (skipped) most-significant bit-planes (`Mb - num_bitplanes`).
    pub zero_bitplanes: u32,
    /// Length in bytes of this block's body.
    pub len: usize,
}

/// A code-block is included in this (only) layer iff it produced a non-empty
/// coded segment.
fn is_included(cb: &Option<CodedBlock>) -> bool {
    matches!(cb, Some(c) if c.num_bitplanes > 0)
}

// ---------------------------------------------------------------------------
// Packet writer / reader
// ---------------------------------------------------------------------------

/// Write one packet (single layer, single precinct per band) for the given
/// `bands`, appending header then bodies to `out`.
///
/// `states[i]`, `blocks[i]` and `mb_per_band[i]` all correspond to `bands[i]`;
/// `blocks[i]` holds one `Option<CodedBlock>` per code-block in raster order
/// (`None`/empty ⇒ not included). Bodies are appended in the same band-then-
/// raster order in which their headers are coded.
pub fn write_packet(
    states: &mut [PrecinctState],
    bands: &[Band],
    blocks: &[&[Option<CodedBlock>]],
    mb_per_band: &[u32],
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    debug_assert_eq!(states.len(), bands.len());
    debug_assert_eq!(blocks.len(), bands.len());
    debug_assert_eq!(mb_per_band.len(), bands.len());

    let any_included = blocks
        .iter()
        .flat_map(|band_blocks| band_blocks.iter())
        .any(is_included);

    let mut bw = PacketBitWriter::new();

    if !any_included {
        // Empty packet: a single 0 bit, no bodies.
        bw.put_bit(0);
        out.extend_from_slice(&bw.finish());
        return Ok(());
    }

    // Non-empty packet indicator.
    bw.put_bit(1);

    for (bi, band) in bands.iter().enumerate() {
        let st = &mut states[bi];
        let gw = band.cb_grid_w;
        let gh = band.cb_grid_h;
        let mb = mb_per_band[bi];
        let band_blocks = blocks[bi];
        debug_assert_eq!(band_blocks.len(), (gw as usize) * (gh as usize));

        // Set all leaf values before coding any node (tag-tree min-propagation
        // needs the whole leaf grid populated first).
        for cy in 0..gh {
            for cx in 0..gw {
                let k = (cy * gw + cx) as usize;
                let nb = match &band_blocks[k] {
                    Some(c) => c.num_bitplanes,
                    None => 0,
                };
                let included = is_included(&band_blocks[k]);
                st.incl.set(cx, cy, if included { 0 } else { 1 });
                // Every block has a zero-bit-plane count; unincluded blocks are
                // never queried but their leaf keeps the tree's minima honest.
                let zbp = mb.checked_sub(nb).ok_or_else(|| {
                    CodecError::InvalidData(format!(
                        "num_bitplanes {nb} exceeds Mb {mb} (band {:?})",
                        band.kind
                    ))
                })?;
                st.zbp.set(cx, cy, zbp);
            }
        }

        for cy in 0..gh {
            for cx in 0..gw {
                let k = (cy * gw + cx) as usize;
                let cb = &band_blocks[k];
                let included = is_included(cb);
                // Inclusion at layer 0 (threshold 1).
                st.incl.encode(&mut |b| bw.put_bit(b as u8), cx, cy, 1);
                if !included {
                    continue;
                }
                let coded = cb.as_ref().expect("included ⇒ Some");
                let zbp = mb - coded.num_bitplanes;

                // Zero-bit-planes: encode rising thresholds until the value is
                // pinned (established at threshold zbp + 1).
                let mut t = 1u32;
                loop {
                    st.zbp.encode(&mut |b| bw.put_bit(b as u8), cx, cy, t);
                    if zbp < t {
                        break;
                    }
                    t += 1;
                }

                write_num_passes(&mut bw, coded.num_passes);

                // Length (single codeword segment): grow Lblock as needed, code
                // the increment as k ones + a zero, then the length itself.
                let passlog2 = floor_log2(coded.num_passes);
                let len = coded.data.len() as u64;
                let bits_needed = if len == 0 {
                    1
                } else {
                    floor_log2(len as u32) + 1
                };
                let base = st.lblock[k] as u32 + passlog2;
                let increment = bits_needed.saturating_sub(base);
                st.lblock[k] += increment as u8;
                for _ in 0..increment {
                    bw.put_bit(1);
                }
                bw.put_bit(0);
                let field = st.lblock[k] as u32 + passlog2;
                bw.put_bits(len, field);

                st.first_layer_included[k] = true;
            }
        }
    }

    out.extend_from_slice(&bw.finish());

    // Bodies, same band-then-raster order as the header.
    for (bi, band) in bands.iter().enumerate() {
        let band_blocks = blocks[bi];
        for cy in 0..band.cb_grid_h {
            for cx in 0..band.cb_grid_w {
                let k = (cy * band.cb_grid_w + cx) as usize;
                if let Some(c) = &band_blocks[k] {
                    if c.num_bitplanes > 0 {
                        out.extend_from_slice(&c.data);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Read one packet header for `bands`, returning per-band per-block
/// contributions (`None` when a block is not included) and the number of
/// **header** bytes consumed. The caller reads each included block's `len`
/// bytes of body from the bytes immediately following the header, in
/// band-then-raster order.
#[allow(clippy::type_complexity)]
pub fn read_packet(
    states: &mut [PrecinctState],
    bands: &[Band],
    mb_per_band: &[u32],
    data: &[u8],
) -> Result<(Vec<Vec<Option<BlockContribution>>>, usize), CodecError> {
    debug_assert_eq!(states.len(), bands.len());
    debug_assert_eq!(mb_per_band.len(), bands.len());

    let mut br = PacketBitReader::new(data);

    // Non-empty indicator.
    if br.read_bit()? == 0 {
        let contributions = bands
            .iter()
            .map(|b| vec![None; b.cbs.len()])
            .collect::<Vec<_>>();
        return Ok((contributions, br.consumed()?));
    }

    let mut contributions = Vec::with_capacity(bands.len());
    for (bi, band) in bands.iter().enumerate() {
        let st = &mut states[bi];
        let gw = band.cb_grid_w;
        let gh = band.cb_grid_h;
        let mb = mb_per_band[bi];
        let mut band_out: Vec<Option<BlockContribution>> =
            vec![None; (gw as usize) * (gh as usize)];

        for cy in 0..gh {
            for cx in 0..gw {
                let k = (cy * gw + cx) as usize;
                let included = st.incl.decode(&mut || br.read_bit(), cx, cy, 1)?;
                if !included {
                    continue;
                }

                // Zero-bit-planes: rising thresholds until resolved.
                let mut t = 1u32;
                let zbp;
                loop {
                    if st.zbp.decode(&mut || br.read_bit(), cx, cy, t)? {
                        zbp = t - 1;
                        break;
                    }
                    t += 1;
                    if t > mb + 1 {
                        return Err(CodecError::InvalidData(format!(
                            "zero-bit-plane count exceeds Mb {mb} (band {:?})",
                            band.kind
                        )));
                    }
                }

                let num_passes = read_num_passes(&mut br)?;
                let passlog2 = floor_log2(num_passes);

                // Lblock increment (ones terminated by a zero).
                let mut increment = 0u32;
                while br.read_bit()? == 1 {
                    increment += 1;
                    if increment > 32 {
                        return Err(CodecError::InvalidData(
                            "Lblock increment runaway".to_string(),
                        ));
                    }
                }
                st.lblock[k] += increment as u8;
                let field = st.lblock[k] as u32 + passlog2;
                if field > 63 {
                    return Err(CodecError::InvalidData(
                        "length field width exceeds 63 bits".to_string(),
                    ));
                }
                let len = br.read_bits(field)? as usize;
                if len > data.len() {
                    return Err(CodecError::InvalidData(
                        "code-block length exceeds buffer".to_string(),
                    ));
                }

                st.first_layer_included[k] = true;
                band_out[k] = Some(BlockContribution {
                    num_passes,
                    zero_bitplanes: zbp,
                    len,
                });
            }
        }
        contributions.push(band_out);
    }

    Ok((contributions, br.consumed()?))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{build_geometry, Band};
    use proptest::prelude::*;

    // -- stuffing bit I/O ---------------------------------------------------

    #[test]
    fn bit_io_roundtrip_random() {
        let mut rng = 0x1234_5678_9abc_def0u64;
        for _ in 0..500 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let nbits = (rng % 40) as u32 + 1;
            let bits: Vec<u8> = (0..nbits).map(|i| ((rng >> (i % 63)) & 1) as u8).collect();
            let mut bw = PacketBitWriter::new();
            for &b in &bits {
                bw.put_bit(b);
            }
            let bytes = bw.finish();
            // Header never ends with 0xFF.
            assert_ne!(bytes.last(), Some(&0xFF));
            let mut br = PacketBitReader::new(&bytes);
            for (i, &b) in bits.iter().enumerate() {
                assert_eq!(br.read_bit().unwrap(), b as u32, "bit {i}");
            }
        }
    }

    #[test]
    fn stuffing_all_ones_hits_ff_boundary() {
        // A long run of 1 bits forces 0xFF bytes and the 7-bit follow-on.
        for nbits in 1u32..=40 {
            let mut bw = PacketBitWriter::new();
            for _ in 0..nbits {
                bw.put_bit(1);
            }
            let bytes = bw.finish();
            assert_ne!(bytes.last(), Some(&0xFF), "ends with 0xFF at n={nbits}");
            // Every byte following a 0xFF must have its MSB clear.
            for w in bytes.windows(2) {
                if w[0] == 0xFF {
                    assert_eq!(w[1] & 0x80, 0, "stuffed byte MSB set");
                }
            }
            let mut br = PacketBitReader::new(&bytes);
            for _ in 0..nbits {
                assert_eq!(br.read_bit().unwrap(), 1);
            }
        }
    }

    #[test]
    fn reader_truncation_errors() {
        let mut bw = PacketBitWriter::new();
        for _ in 0..20 {
            bw.put_bit(1);
        }
        let bytes = bw.finish();
        // A reader over an empty slice must error rather than panic.
        let mut br = PacketBitReader::new(&[]);
        assert!(br.read_bit().is_err());
        // Reading more bits than the buffer holds errors.
        let mut br = PacketBitReader::new(&bytes);
        let mut errored = false;
        for _ in 0..(bytes.len() * 8 + 16) {
            if br.read_bit().is_err() {
                errored = true;
                break;
            }
        }
        assert!(errored);
    }

    #[test]
    fn num_passes_roundtrip_full_range() {
        for n in 1u32..=164 {
            let mut bw = PacketBitWriter::new();
            write_num_passes(&mut bw, n);
            let bytes = bw.finish();
            let mut br = PacketBitReader::new(&bytes);
            assert_eq!(read_num_passes(&mut br).unwrap(), n, "n={n}");
        }
    }

    // -- packet round-trips -------------------------------------------------

    /// Build the resolution-0 band list (single LL band) plus a matching
    /// grid via geometry, for the given tile.
    fn ll_band(w: u32, h: u32, levels: u8, xcb: u8, ycb: u8) -> Band {
        let g = build_geometry(w, h, levels, xcb, ycb);
        g.resolutions[0].bands[0].clone()
    }

    /// Deterministic synthetic coded block.
    fn synth(num_bitplanes: u32, num_passes: u32, len: usize, fill: u8) -> CodedBlock {
        CodedBlock {
            data: vec![fill; len],
            num_passes,
            num_bitplanes,
        }
    }

    #[test]
    fn empty_packet_roundtrip() {
        let band = ll_band(64, 64, 0, 6, 6); // 64x64 single LL, one code-block
        let bands = [band];
        let mb = [18u32];
        let blocks: Vec<Option<CodedBlock>> = vec![None];
        let mut states = vec![PrecinctState::new(bands[0].cb_grid_w, bands[0].cb_grid_h)];

        let mut out = Vec::new();
        write_packet(&mut states, &bands, &[&blocks], &mb, &mut out).unwrap();
        assert_eq!(out, vec![0x00], "empty packet is a single zero byte");

        let mut rstates = vec![PrecinctState::new(bands[0].cb_grid_w, bands[0].cb_grid_h)];
        let (contribs, consumed) = read_packet(&mut rstates, &bands, &mb, &out).unwrap();
        assert_eq!(consumed, 1);
        assert_eq!(contribs, vec![vec![None]]);
    }

    #[test]
    fn single_block_included_roundtrip() {
        let band = ll_band(64, 64, 0, 6, 6);
        let bands = [band];
        let mb = [18u32];
        let coded = synth(7, 19, 42, 0xAB);
        let blocks = vec![Some(coded.clone())];
        let mut states = vec![PrecinctState::new(bands[0].cb_grid_w, bands[0].cb_grid_h)];

        let mut out = Vec::new();
        write_packet(&mut states, &bands, &[&blocks], &mb, &mut out).unwrap();

        let mut rstates = vec![PrecinctState::new(bands[0].cb_grid_w, bands[0].cb_grid_h)];
        let (contribs, header_len) = read_packet(&mut rstates, &bands, &mb, &out).unwrap();
        let c = contribs[0][0].as_ref().unwrap();
        assert_eq!(c.num_passes, 19);
        assert_eq!(c.zero_bitplanes, 18 - 7);
        assert_eq!(c.len, 42);
        // Body follows the header.
        assert_eq!(&out[header_len..header_len + c.len], coded.data.as_slice());
        assert_eq!(out.len(), header_len + 42);
    }

    /// A multi-block band (small code-blocks so the grid is > 1x1).
    fn grid_band() -> Band {
        // 64x64 tile, 0 levels, 4x4 code-blocks → 16x16 grid on the LL band.
        ll_band(64, 64, 0, 2, 2)
    }

    fn run_packet_case(
        band: &Band,
        mb: u32,
        blocks: &[Option<CodedBlock>],
    ) -> (Vec<Vec<Option<BlockContribution>>>, Vec<u8>) {
        let bands = std::slice::from_ref(band);
        let mbv = [mb];
        let mut states = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
        let mut out = Vec::new();
        write_packet(&mut states, bands, &[blocks], &mbv, &mut out).unwrap();

        let mut rstates = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
        let (contribs, header_len) = read_packet(&mut rstates, bands, &mbv, &out).unwrap();

        // Verify bodies and lengths against the encoder input.
        let mut body_pos = header_len;
        for (k, blk) in blocks.iter().enumerate() {
            match (&contribs[0][k], is_included(blk)) {
                (Some(c), true) => {
                    let coded = blk.as_ref().unwrap();
                    assert_eq!(c.num_passes, coded.num_passes);
                    assert_eq!(c.zero_bitplanes, mb - coded.num_bitplanes);
                    assert_eq!(c.len, coded.data.len());
                    assert_eq!(&out[body_pos..body_pos + c.len], coded.data.as_slice());
                    body_pos += c.len;
                }
                (None, false) => {}
                _ => panic!("inclusion mismatch at block {k}"),
            }
        }
        assert_eq!(body_pos, out.len(), "bodies must fill the packet exactly");
        (contribs, out)
    }

    #[test]
    fn multi_block_mixed_inclusion() {
        let band = grid_band();
        let n = (band.cb_grid_w * band.cb_grid_h) as usize;
        assert!(n > 1);
        let mb = 20u32;
        // Include a scattered subset with varying pass counts / lengths.
        let mut blocks: Vec<Option<CodedBlock>> = vec![None; n];
        blocks[0] = Some(synth(3, 7, 1, 0x01));
        blocks[n / 3] = Some(synth(12, 34, 200, 0x02));
        blocks[n / 2] = Some(synth(1, 1, 5, 0x03));
        blocks[n - 1] = Some(synth(20, 58, 999, 0x04));
        run_packet_case(&band, mb, &blocks);
    }

    #[test]
    fn truncation_at_every_byte_errors() {
        let band = grid_band();
        let n = (band.cb_grid_w * band.cb_grid_h) as usize;
        let mb = 20u32;
        let mut blocks: Vec<Option<CodedBlock>> = vec![None; n];
        blocks[1] = Some(synth(9, 25, 300, 0x55));
        blocks[n - 2] = Some(synth(4, 10, 7, 0xAA));

        let bands = std::slice::from_ref(&band);
        let mbv = [mb];
        let mut states = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
        let mut out = Vec::new();
        write_packet(&mut states, bands, &[&blocks], &mbv, &mut out).unwrap();

        // Header-parse over every truncation must be Ok-or-Err, never panic.
        for cut in 0..out.len() {
            let mut rstates = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
            let _ = read_packet(&mut rstates, bands, &mbv, &out[..cut]);
        }
        // The full buffer parses.
        let mut rstates = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
        assert!(read_packet(&mut rstates, bands, &mbv, &out).is_ok());
    }

    #[test]
    fn sequential_packets_shared_buffer() {
        // Several packets over three different bands, concatenated into one
        // stream and read back in order (mirrors the tile-pipeline layout).
        let g = build_geometry(64, 64, 2, 3, 3);
        let mut stream = Vec::new();
        let mut plan: Vec<(Band, u32, Vec<Option<CodedBlock>>, usize)> = Vec::new();

        for res in &g.resolutions {
            for band in &res.bands {
                if band.rect.is_empty() {
                    continue;
                }
                let n = (band.cb_grid_w * band.cb_grid_h) as usize;
                let mb = 22u32;
                let mut blocks: Vec<Option<CodedBlock>> = vec![None; n];
                // Include the first and last block of each band.
                blocks[0] = Some(synth(6, 16, 40 + n, 0x11));
                if n > 1 {
                    blocks[n - 1] = Some(synth(2, 4, 17, 0x22));
                }
                let bands = std::slice::from_ref(band);
                let mbv = [mb];
                let mut states = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
                let header_start = stream.len();
                write_packet(&mut states, bands, &[&blocks], &mbv, &mut stream).unwrap();
                plan.push((band.clone(), mb, blocks, header_start));
            }
        }

        // Read them all back sequentially.
        let mut pos = 0usize;
        for (band, mb, blocks, header_start) in &plan {
            assert_eq!(*header_start, pos);
            let bands = std::slice::from_ref(band);
            let mbv = [*mb];
            let mut rstates = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
            let (contribs, header_len) =
                read_packet(&mut rstates, bands, &mbv, &stream[pos..]).unwrap();
            let mut body_pos = pos + header_len;
            for (k, blk) in blocks.iter().enumerate() {
                if let Some(coded) = blk {
                    let c = contribs[0][k].as_ref().unwrap();
                    assert_eq!(c.len, coded.data.len());
                    assert_eq!(&stream[body_pos..body_pos + c.len], coded.data.as_slice());
                    body_pos += c.len;
                }
            }
            pos = body_pos;
        }
        assert_eq!(pos, stream.len());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn prop_header_roundtrip(
            seed in any::<u64>(),
            mb in 4u32..=30,
        ) {
            // 32x32 LL band with 4x4 code-blocks → 8x8 grid.
            let band = ll_band(32, 32, 0, 2, 2);
            let n = (band.cb_grid_w * band.cb_grid_h) as usize;
            let mut rng = seed | 1;
            let mut next = || {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                rng >> 33
            };
            let mut blocks: Vec<Option<CodedBlock>> = Vec::with_capacity(n);
            for _ in 0..n {
                if next() % 3 == 0 {
                    blocks.push(None);
                } else {
                    let nb = 1 + (next() as u32 % mb.max(1)); // 1..=mb
                    let nb = nb.min(mb);
                    let max_passes = 3 * nb - 2;
                    let num_passes = 1 + (next() as u32 % max_passes.max(1)).min(163);
                    let len = (next() as usize) % 2000;
                    blocks.push(Some(synth(nb, num_passes.max(1), len, next() as u8)));
                }
            }

            let bands = std::slice::from_ref(&band);
            let mbv = [mb];
            let mut states = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
            let mut out = Vec::new();
            write_packet(&mut states, bands, &[&blocks], &mbv, &mut out).unwrap();

            let mut rstates = vec![PrecinctState::new(band.cb_grid_w, band.cb_grid_h)];
            let (contribs, header_len) = read_packet(&mut rstates, bands, &mbv, &out).unwrap();

            let mut body_pos = header_len;
            for (k, blk) in blocks.iter().enumerate() {
                match blk {
                    Some(coded) if coded.num_bitplanes > 0 => {
                        let c = contribs[0][k].as_ref().unwrap();
                        prop_assert_eq!(c.num_passes, coded.num_passes);
                        prop_assert_eq!(c.zero_bitplanes, mb - coded.num_bitplanes);
                        prop_assert_eq!(c.len, coded.data.len());
                        prop_assert_eq!(&out[body_pos..body_pos + c.len], coded.data.as_slice());
                        body_pos += c.len;
                    }
                    _ => prop_assert!(contribs[0][k].is_none()),
                }
            }
            prop_assert_eq!(body_pos, out.len());
        }
    }
}
