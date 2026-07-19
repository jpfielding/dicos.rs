//! Tag-tree (quad-tree) codec for JPEG 2000 packet headers (ITU-T T.800 B.10.2).
//!
//! A tag tree is a hierarchical description of a 2-D array of non-negative
//! integers where each interior node holds the minimum of its four children.
//! Packet headers use two tag trees per precinct — one for code-block inclusion
//! and one for zero-bit-plane counts (B.10.7.2 / B.10.7.5). Values are coded
//! *incrementally*: repeated queries at rising thresholds emit / consume only
//! the bits needed to refine what the decoder already knows, and per-node state
//! persists across queries (across code-blocks and thresholds within a packet
//! sequence).
//!
//! The encode/decode routines take bit callbacks rather than a concrete
//! bit-stream reader/writer, so this module has no dependency on the (future)
//! packet layer: `packet.rs` will pass closures wired to its FF-stuffing bit
//! I/O. The algorithm mirrors the canonical Taubman / OpenJPEG `tgt_encode` /
//! `tgt_decode` procedures.

use crate::error::CodecError;

/// Sentinel meaning "value not yet known" (decoder upper bound before any bit
/// establishes it, and the identity for the encoder's min-propagation).
const UNKNOWN: u32 = u32::MAX;

#[derive(Clone)]
struct Node {
    /// Encoder: the (min-propagated) node value. Decoder: current upper bound,
    /// lowered to the exact value once a `1` bit is read.
    value: u32,
    /// Coding-state lower bound `low` for this node (starts at 0).
    low: u32,
    /// Encoder only: a `1` has already been emitted at this node.
    known: bool,
}

impl Node {
    fn new() -> Self {
        Node {
            value: UNKNOWN,
            low: 0,
            known: false,
        }
    }
}

struct Level {
    w: u32,
    h: u32,
    nodes: Vec<Node>,
}

impl Level {
    #[inline]
    fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.w + x) as usize
    }
}

/// A tag tree over a `leaves_w × leaves_h` grid of non-negative values.
pub struct TagTree {
    /// `levels[0]` is the leaf grid; each subsequent level halves (ceil) both
    /// dimensions; the last level is `1×1` (the root). Empty when there are no
    /// leaves.
    levels: Vec<Level>,
    leaves_w: u32,
    leaves_h: u32,
}

impl TagTree {
    /// Build a tag tree with the given leaf-grid dimensions. A zero dimension
    /// (no leaves) yields an empty tree that ignores `set`/`encode` and decodes
    /// as "not established".
    pub fn new(leaves_w: u32, leaves_h: u32) -> Self {
        let mut levels = Vec::new();
        if leaves_w != 0 && leaves_h != 0 {
            let (mut w, mut h) = (leaves_w, leaves_h);
            loop {
                let nodes = vec![Node::new(); (w as usize) * (h as usize)];
                levels.push(Level { w, h, nodes });
                if w == 1 && h == 1 {
                    break;
                }
                w = w.div_ceil(2);
                h = h.div_ceil(2);
            }
        }
        TagTree {
            levels,
            leaves_w,
            leaves_h,
        }
    }

    #[inline]
    fn in_range(&self, x: u32, y: u32) -> bool {
        !self.levels.is_empty() && x < self.leaves_w && y < self.leaves_h
    }

    /// Set a leaf value (encoder side). Propagates the running minimum up to the
    /// root so every interior node holds the minimum of its descendant leaves.
    pub fn set(&mut self, x: u32, y: u32, value: u32) {
        if !self.in_range(x, y) {
            return;
        }
        let (mut cx, mut cy) = (x, y);
        for level in &mut self.levels {
            let i = level.idx(cx, cy);
            if value < level.nodes[i].value {
                level.nodes[i].value = value;
            }
            cx /= 2;
            cy /= 2;
        }
    }

    /// Clear coding state (`low` and the emitted-bit flags) while keeping the
    /// leaf/interior values. Lets one tree be re-coded from scratch.
    pub fn reset(&mut self) {
        for level in &mut self.levels {
            for node in &mut level.nodes {
                node.low = 0;
                node.known = false;
            }
        }
    }

    /// Path of `(level, x, y)` triples from leaf (`index 0`) up to root.
    fn path(&self, x: u32, y: u32) -> Vec<(usize, u32, u32)> {
        let mut path = Vec::with_capacity(self.levels.len());
        let (mut cx, mut cy) = (x, y);
        for l in 0..self.levels.len() {
            path.push((l, cx, cy));
            cx /= 2;
            cy /= 2;
        }
        path
    }

    /// Encode the information needed to establish whether leaf `(x, y)` has value
    /// `< threshold`, emitting each bit through `out`. Walks root → leaf; per
    /// node it emits `0` while the lower bound is below both the threshold and
    /// the node value, and a single `1` when the value is reached below the
    /// threshold. State persists for later (higher-threshold) queries.
    pub fn encode(&mut self, out: &mut dyn FnMut(u32), x: u32, y: u32, threshold: u32) {
        if !self.in_range(x, y) {
            return;
        }
        let path = self.path(x, y);
        let mut low = 0u32;
        // Root (last in path) down to leaf (first).
        for &(l, nx, ny) in path.iter().rev() {
            let level = &mut self.levels[l];
            let i = level.idx(nx, ny);
            let node = &mut level.nodes[i];

            // Carry the running lower bound down; a child can never sit below
            // its parent's established lower bound.
            if low > node.low {
                node.low = low;
            } else {
                low = node.low;
            }

            while low < threshold {
                if low >= node.value {
                    if !node.known {
                        out(1);
                        node.known = true;
                    }
                    break;
                }
                out(0);
                low += 1;
            }
            node.low = low;
        }
    }

    /// Decode the mirror of [`encode`], pulling bits from `next_bit`. Returns
    /// `Ok(true)` when leaf `(x, y)` is established to have value `< threshold`.
    /// A bit source that runs out surfaces as `Err`, never a panic.
    pub fn decode(
        &mut self,
        next_bit: &mut dyn FnMut() -> Result<u32, CodecError>,
        x: u32,
        y: u32,
        threshold: u32,
    ) -> Result<bool, CodecError> {
        if !self.in_range(x, y) {
            return Ok(false);
        }
        let path = self.path(x, y);
        let mut low = 0u32;
        for &(l, nx, ny) in path.iter().rev() {
            let level = &mut self.levels[l];
            let i = level.idx(nx, ny);
            let node = &mut level.nodes[i];

            if low > node.low {
                node.low = low;
            } else {
                low = node.low;
            }

            while low < threshold && low < node.value {
                if next_bit()? != 0 {
                    node.value = low; // value established at this bound
                } else {
                    low += 1;
                }
            }
            node.low = low;
        }

        // Leaf is path[0]; its value is now known iff below the threshold.
        let (l, lx, ly) = path[0];
        let level = &self.levels[l];
        Ok(level.nodes[level.idx(lx, ly)].value < threshold)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a full incremental query sweep and return the emitted bits.
    fn encode_sweep(vals: &[Vec<u32>], w: u32, h: u32, max_t: u32) -> Vec<u32> {
        let mut tree = TagTree::new(w, h);
        for (y, row) in vals.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                tree.set(x as u32, y as u32, v);
            }
        }
        let mut bits = Vec::new();
        for t in 1..=max_t {
            for y in 0..h {
                for x in 0..w {
                    tree.encode(&mut |b| bits.push(b), x, y, t);
                }
            }
        }
        bits
    }

    /// Decode the same sweep, asserting each query and exact bit consumption.
    fn decode_sweep_and_check(vals: &[Vec<u32>], w: u32, h: u32, max_t: u32, bits: &[u32]) {
        let mut tree = TagTree::new(w, h);
        let mut pos = 0usize;
        for t in 1..=max_t {
            for y in 0..h {
                for x in 0..w {
                    let got = tree
                        .decode(
                            &mut || {
                                let b = *bits.get(pos).expect("ran out of bits");
                                pos += 1;
                                Ok(b)
                            },
                            x,
                            y,
                            t,
                        )
                        .expect("decode ok");
                    let want = vals[y as usize][x as usize] < t;
                    assert_eq!(got, want, "leaf ({x},{y}) @ t={t}");
                }
            }
        }
        assert_eq!(pos, bits.len(), "decoder must consume every emitted bit");
    }

    fn roundtrip(vals: &[Vec<u32>]) {
        let h = vals.len() as u32;
        let w = vals[0].len() as u32;
        let max = vals.iter().flatten().copied().max().unwrap_or(0);
        let max_t = max + 1;
        let bits = encode_sweep(vals, w, h, max_t);
        decode_sweep_and_check(vals, w, h, max_t, &bits);
    }

    // (a) Worked-example-shaped 3×2 tree. We verify by exhaustive round-trip
    // (per the plan's guidance) rather than asserting an invented bitstring.
    #[test]
    fn worked_example_3x2() {
        let vals = vec![vec![1u32, 3, 2], vec![2, 2, 3]];
        roundtrip(&vals);
    }

    // (c) 1×1 tree.
    #[test]
    fn single_leaf() {
        roundtrip(&[vec![0]]);
        roundtrip(&[vec![5]]);
        roundtrip(&[vec![1]]);
    }

    #[test]
    fn all_zero_and_flat() {
        roundtrip(&[vec![0, 0, 0], vec![0, 0, 0]]);
        roundtrip(&[vec![7, 7], vec![7, 7], vec![7, 7]]);
    }

    #[test]
    fn threshold_learns_value_exactly() {
        // A single leaf of value v is "< t" for t in v+1.. and not before; the
        // exact value is pinned at t = v+1.
        let mut enc = TagTree::new(1, 1);
        enc.set(0, 0, 3);
        let mut bits = Vec::new();
        for t in 1..=5 {
            enc.encode(&mut |b| bits.push(b), 0, 0, t);
        }
        let mut dec = TagTree::new(1, 1);
        let mut pos = 0;
        for t in 1..=5 {
            let got = dec
                .decode(
                    &mut || {
                        let b = bits[pos];
                        pos += 1;
                        Ok(b)
                    },
                    0,
                    0,
                    t,
                )
                .unwrap();
            assert_eq!(got, 3 < t, "t={t}");
        }
        assert_eq!(pos, bits.len());
    }

    #[test]
    fn reset_keeps_values() {
        let vals = vec![vec![1u32, 4], vec![2, 0]];
        let bits_a = encode_sweep(&vals, 2, 2, 5);

        // Encode once, reset, encode again: values survive so bits match.
        let mut tree = TagTree::new(2, 2);
        for (y, row) in vals.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                tree.set(x as u32, y as u32, v);
            }
        }
        for t in 1..=5 {
            for y in 0..2 {
                for x in 0..2 {
                    tree.encode(&mut |_| {}, x, y, t);
                }
            }
        }
        tree.reset();
        let mut bits_b = Vec::new();
        for t in 1..=5 {
            for y in 0..2 {
                for x in 0..2 {
                    tree.encode(&mut |b| bits_b.push(b), x, y, t);
                }
            }
        }
        assert_eq!(bits_a, bits_b, "reset must preserve values");
    }

    // Empty tree (0-size) is graceful.
    #[test]
    fn empty_tree_graceful() {
        let mut t = TagTree::new(0, 5);
        t.set(0, 0, 3); // ignored, no panic
        t.encode(&mut |_| panic!("no bits for empty tree"), 0, 0, 4);
        let got = t
            .decode(&mut || panic!("no reads for empty tree"), 0, 0, 4)
            .unwrap();
        assert!(!got);

        let mut t2 = TagTree::new(4, 0);
        t2.encode(&mut |_| panic!("no bits"), 1, 0, 2);
        assert!(!t2.decode(&mut || panic!("no reads"), 1, 0, 2).unwrap());
    }

    // (d) Out-of-bits decode → Err, not panic.
    #[test]
    fn decode_out_of_bits_errors() {
        let mut enc = TagTree::new(2, 2);
        enc.set(0, 0, 2);
        enc.set(1, 0, 2);
        enc.set(0, 1, 2);
        enc.set(1, 1, 2);
        let mut bits = Vec::new();
        enc.encode(&mut |b| bits.push(b), 0, 0, 3);

        // Feed a source that yields nothing.
        let mut dec = TagTree::new(2, 2);
        let res = dec.decode(
            &mut || Err(CodecError::InvalidData("out of bits".into())),
            0,
            0,
            3,
        );
        assert!(res.is_err(), "exhausted bit source must Err");
    }

    // (b) Property test: random grids, full incremental sweep, exact match.
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]
        #[test]
        fn prop_incremental_roundtrip(
            w in 1u32..=17,
            h in 1u32..=13,
            seed in any::<u64>(),
        ) {
            // Deterministic pseudo-random values in 0..=20 from the seed.
            let mut state = seed | 1;
            let mut next = || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % 21) as u32
            };
            let vals: Vec<Vec<u32>> = (0..h)
                .map(|_| (0..w).map(|_| next()).collect())
                .collect();

            let max = vals.iter().flatten().copied().max().unwrap_or(0);
            let max_t = max + 1;
            let bits = encode_sweep(&vals, w, h, max_t);
            decode_sweep_and_check(&vals, w, h, max_t, &bits);
        }
    }
}
