//! Huffman table construction and lookup for JPEG Lossless encoding/decoding.
//!
//! Implements the Huffman code generation algorithm from ITU-T T.81 Annex C,
//! including a fast 8-bit lookup table for decoding.

/// A Huffman table used for encoding and decoding JPEG lossless data.
///
/// The table stores both the canonical representation (bits/values) and
/// pre-computed codes, sizes, and a fast 8-bit lookup table.
#[derive(Debug, Clone)]
pub(crate) struct HuffmanTable {
    /// Number of codes of each length (1-indexed: bits[i] = count of i-bit codes).
    pub bits: [u8; 17],
    /// Symbol values in code-length order.
    pub values: Vec<u8>,
    /// Computed Huffman codes (parallel to `values`).
    pub codes: Vec<u16>,
    /// Computed code sizes in bits (parallel to `values`).
    pub sizes: Vec<u8>,
    /// Fast 8-bit lookup table. Each entry packs (size << 8 | value).
    /// A value of -1 means "not found, use slow path".
    pub lookup: [i16; 256],
}

impl HuffmanTable {
    /// Creates an empty Huffman table with no codes.
    pub fn new() -> Self {
        Self {
            bits: [0; 17],
            values: Vec::new(),
            codes: Vec::new(),
            sizes: Vec::new(),
            lookup: [-1; 256],
        }
    }

    /// Builds a Huffman table from `bits` (count of codes per length) and `values`.
    ///
    /// This generates the canonical Huffman codes and populates the fast lookup table.
    pub fn from_bits_values(bits: [u8; 17], values: Vec<u8>) -> Self {
        let mut ht = Self {
            bits,
            values,
            codes: Vec::new(),
            sizes: Vec::new(),
            lookup: [-1; 256],
        };
        ht.generate_codes();
        ht.build_lookup();
        ht
    }

    /// Generate Huffman codes and sizes from the bits/values representation.
    ///
    /// Implements the GENERATE_SIZE_TABLE and GENERATE_CODE_TABLE procedures
    /// from ITU-T T.81 Annex C.
    fn generate_codes(&mut self) {
        let total: usize = self.bits[1..=16].iter().map(|&b| b as usize).sum();
        self.codes = vec![0u16; total];
        self.sizes = vec![0u8; total];

        // GENERATE_SIZE_TABLE: assign code lengths
        let mut k = 0usize;
        for i in 1u8..=16 {
            for _ in 0..self.bits[i as usize] {
                self.sizes[k] = i;
                k += 1;
            }
        }

        if total == 0 {
            return;
        }

        // GENERATE_CODE_TABLE: assign codes
        let mut code: u16 = 0;
        let mut si = self.sizes[0];
        for k in 0..total {
            while self.sizes[k] > si {
                code <<= 1;
                si += 1;
            }
            self.codes[k] = code;
            // Every *assigned* canonical code fits in u16 under a valid (Kraft)
            // `bits` table, but a complete length-16 code assigns 0xFFFF to its
            // last symbol; the trailing increment past it is unused yet would
            // overflow. `wrapping_add` keeps that final, discarded step panic-free.
            code = code.wrapping_add(1);
        }
    }

    /// Build the fast 8-bit lookup table for decoding.
    ///
    /// For codes that fit in 8 bits or fewer, all possible byte-aligned
    /// representations are stored so that a single array index gives us
    /// the decoded symbol and its code length.
    fn build_lookup(&mut self) {
        self.lookup = [-1; 256];
        let total = self.codes.len();
        for k in 0..total {
            let size = self.sizes[k] as u32;
            if size <= 8 {
                let base = (self.codes[k] as u32) << (8 - size);
                let count = 1u32 << (8 - size);
                for i in 0..count {
                    let idx = (base + i) as usize;
                    // Pack size in high byte, value in low byte
                    self.lookup[idx] = ((size as i16) << 8) | (self.values[k] as i16);
                }
            }
        }
    }

    /// Look up a symbol using the fast 8-bit table.
    ///
    /// Returns `Some((code_size, symbol_value))` if the code fits in 8 bits,
    /// or `None` if the slow path must be used.
    #[inline]
    pub fn fast_lookup(&self, byte_val: u8) -> Option<(u8, u8)> {
        let entry = self.lookup[byte_val as usize];
        if entry >= 0 {
            let size = (entry >> 8) as u8;
            let value = (entry & 0xFF) as u8;
            Some((size, value))
        } else {
            None
        }
    }

    /// Find the code and size for a given symbol value (used during encoding).
    ///
    /// Returns `Some((code, size))` or `None` if the symbol is not in the table.
    pub fn encode_symbol(&self, symbol: u8) -> Option<(u16, u8)> {
        for (i, &val) in self.values.iter().enumerate() {
            if val == symbol {
                return Some((self.codes[i], self.sizes[i]));
            }
        }
        None
    }

    /// Slow-path decode: given a sequence of bits accumulated so far and the
    /// current bit length, check if there is a matching code.
    ///
    /// Returns `Some(symbol_value)` if found.
    pub fn decode_slow(&self, code: u16, size: u8) -> Option<u8> {
        // Find the starting index for codes of this size
        let mut idx = 0usize;
        for i in 1..size {
            idx += self.bits[i as usize] as usize;
        }
        let count = self.bits[size as usize] as usize;
        for i in 0..count {
            if self.codes[idx + i] == code {
                return Some(self.values[idx + i]);
            }
        }
        None
    }
}

/// Returns `true` if `bits` (count of Huffman codes per length 1..=16) forms a
/// valid, non-oversubscribed code assignment (ITU-T T.81 Annex C / the Kraft
/// inequality).
///
/// At each length `L` the running code value -- the counts accumulated so far,
/// shifted left once per length -- must never exceed `2^L`. An oversubscribed
/// table requests more codes than the prefix code space allows, which would
/// make canonical code generation emit codes that alias and overflow the 8-bit
/// `build_lookup` index (an out-of-bounds panic). This must be rejected before
/// [`HuffmanTable::from_bits_values`] builds the table.
pub(crate) fn bits_valid(bits: &[u8; 17]) -> bool {
    let mut code: u32 = 0;
    for (len, &count) in bits.iter().enumerate().take(17).skip(1) {
        code += count as u32;
        if code > (1u32 << len) {
            return false;
        }
        code <<= 1;
    }
    true
}

/// Returns the SSSS category (number of bits needed) for a difference value.
///
/// For JPEG lossless, the category indicates how many additional bits are
/// needed to represent the magnitude of the difference.
pub(crate) fn categorize(diff: i32) -> u8 {
    let abs = diff.unsigned_abs();
    if abs == 0 {
        0
    } else {
        32 - abs.leading_zeros() as u8
    }
}

/// Extends a partial bit value to a signed difference.
///
/// Implements the EXTEND procedure from ITU-T T.81 Table F.12.
/// If the high bit of the `ssss`-bit value is 0, the value is negative.
pub(crate) fn extend(bits: u32, ssss: u8) -> i32 {
    if ssss == 0 {
        return 0;
    }
    let vt = 1u32 << (ssss - 1);
    if bits < vt {
        // Negative: bits - (2^ssss - 1)
        bits as i32 - ((1i32 << ssss) - 1)
    } else {
        bits as i32
    }
}

/// Build the default Huffman table for 16-bit lossless JPEG encoding.
///
/// This table covers SSSS categories 0-16, which is sufficient for all
/// possible difference values in a 16-bit image.
pub(crate) fn build_default_table() -> HuffmanTable {
    // Fixed distribution covering 17 symbols (SSSS 0..=16):
    // bits[2]=1, bits[3]=5, bits[4..=14]=1 each => 1+5+11 = 17
    let mut bits = [0u8; 17];
    bits[2] = 1;
    bits[3] = 5;
    bits[4] = 1;
    bits[5] = 1;
    bits[6] = 1;
    bits[7] = 1;
    bits[8] = 1;
    bits[9] = 1;
    bits[10] = 1;
    bits[11] = 1;
    bits[12] = 1;
    bits[13] = 1;
    bits[14] = 1;

    let values: Vec<u8> = (0..=16).collect();
    HuffmanTable::from_bits_values(bits, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorize_zero() {
        assert_eq!(categorize(0), 0);
    }

    #[test]
    fn categorize_positive() {
        assert_eq!(categorize(1), 1);
        assert_eq!(categorize(2), 2);
        assert_eq!(categorize(3), 2);
        assert_eq!(categorize(4), 3);
        assert_eq!(categorize(7), 3);
        assert_eq!(categorize(8), 4);
        assert_eq!(categorize(255), 8);
        assert_eq!(categorize(256), 9);
        assert_eq!(categorize(32767), 15);
        assert_eq!(categorize(65535), 16);
    }

    #[test]
    fn categorize_negative() {
        assert_eq!(categorize(-1), 1);
        assert_eq!(categorize(-2), 2);
        assert_eq!(categorize(-3), 2);
        assert_eq!(categorize(-128), 8);
        assert_eq!(categorize(-32768), 16);
    }

    #[test]
    fn extend_zero() {
        assert_eq!(extend(0, 0), 0);
    }

    #[test]
    fn extend_positive() {
        // ssss=1: vt=1, bits=1 >= vt => +1
        assert_eq!(extend(1, 1), 1);
        // ssss=2: vt=2, bits=2 => +2, bits=3 => +3
        assert_eq!(extend(2, 2), 2);
        assert_eq!(extend(3, 2), 3);
        // ssss=8: vt=128, bits=200 => +200
        assert_eq!(extend(200, 8), 200);
    }

    #[test]
    fn extend_negative() {
        // ssss=1: vt=1, bits=0 < vt => 0 - (2^1 - 1) = -1
        assert_eq!(extend(0, 1), -1);
        // ssss=2: vt=2, bits=0 => 0 - 3 = -3, bits=1 => 1 - 3 = -2
        assert_eq!(extend(0, 2), -3);
        assert_eq!(extend(1, 2), -2);
        // ssss=8: vt=128, bits=0 => 0 - 255 = -255
        assert_eq!(extend(0, 8), -255);
        assert_eq!(extend(127, 8), -128);
    }

    #[test]
    fn extend_roundtrip() {
        // Verify that categorize + extend round-trips correctly
        for diff in -1000..=1000 {
            let ssss = categorize(diff);
            let additional = if diff < 0 {
                (diff + (1 << ssss) - 1) as u32
            } else if diff > 0 {
                diff as u32
            } else {
                continue; // ssss=0, no additional bits
            };
            let recovered = extend(additional, ssss);
            assert_eq!(recovered, diff, "roundtrip failed for diff={diff}");
        }
    }

    #[test]
    fn default_table_has_17_symbols() {
        let ht = build_default_table();
        assert_eq!(ht.values.len(), 17);
        assert_eq!(ht.codes.len(), 17);
        assert_eq!(ht.sizes.len(), 17);
        for i in 0..=16u8 {
            assert_eq!(ht.values[i as usize], i);
        }
    }

    #[test]
    fn default_table_codes_are_prefix_free() {
        let ht = build_default_table();
        // No code should be a prefix of another. Since these are canonical
        // Huffman codes, we verify that codes of the same length are distinct.
        for i in 0..ht.codes.len() {
            for j in (i + 1)..ht.codes.len() {
                if ht.sizes[i] == ht.sizes[j] {
                    assert_ne!(
                        ht.codes[i], ht.codes[j],
                        "duplicate code at indices {i} and {j}"
                    );
                }
            }
        }
    }

    #[test]
    fn fast_lookup_covers_short_codes() {
        let ht = build_default_table();
        // The shortest code should be 2 bits (bits[2]=1).
        // Verify the fast lookup finds it.
        let (code, size) = ht.encode_symbol(0).unwrap();
        assert_eq!(size, 2);
        // The 2-bit code extended to 8 bits should all map to symbol 0
        let base = (code as u8) << (8 - size);
        for i in 0..(1u8 << (8 - size)) {
            let (sz, val) = ht.fast_lookup(base + i).unwrap();
            assert_eq!(sz, 2);
            assert_eq!(val, 0);
        }
    }

    #[test]
    fn encode_symbol_all_present() {
        let ht = build_default_table();
        for sym in 0..=16u8 {
            let result = ht.encode_symbol(sym);
            assert!(result.is_some(), "symbol {sym} not found in default table");
        }
    }

    #[test]
    fn encode_symbol_missing() {
        let ht = build_default_table();
        assert!(ht.encode_symbol(17).is_none());
        assert!(ht.encode_symbol(255).is_none());
    }

    #[test]
    fn decode_slow_all_symbols() {
        let ht = build_default_table();
        for (i, &val) in ht.values.iter().enumerate() {
            let code = ht.codes[i];
            let size = ht.sizes[i];
            let decoded = ht.decode_slow(code, size);
            assert_eq!(decoded, Some(val), "slow decode failed for symbol {val}");
        }
    }

    #[test]
    fn decode_slow_invalid_code() {
        let ht = build_default_table();
        // An impossible code of size 1 should not match anything
        assert!(ht.decode_slow(0xFFFF, 1).is_none());
    }

    #[test]
    fn from_bits_values_empty() {
        let ht = HuffmanTable::from_bits_values([0; 17], Vec::new());
        assert!(ht.codes.is_empty());
        assert!(ht.sizes.is_empty());
        assert!(ht.lookup.iter().all(|&v| v == -1));
    }

    #[test]
    fn single_symbol_table() {
        let mut bits = [0u8; 17];
        bits[1] = 1; // one 1-bit code
        let ht = HuffmanTable::from_bits_values(bits, vec![42]);
        assert_eq!(ht.codes.len(), 1);
        assert_eq!(ht.codes[0], 0); // code is "0"
        assert_eq!(ht.sizes[0], 1);
        assert_eq!(ht.encode_symbol(42), Some((0, 1)));
        // Fast lookup: 0b0xxxxxxx should all resolve to symbol 42
        for byte_val in 0..128u8 {
            let (sz, val) = ht.fast_lookup(byte_val).unwrap();
            assert_eq!(sz, 1);
            assert_eq!(val, 42);
        }
    }

    // Regression: a *complete* code (one symbol at each length 1..=15, two at
    // length 16) is Kraft-valid, so it passes `bits_valid` and reaches
    // `from_bits_values`. Its final canonical code is exactly 0xFFFF, and the
    // trailing `code += 1` used to overflow u16 and panic. A libFuzzer decode
    // crash (crates/jpegli/fuzz decode target) minimized to this table.
    #[test]
    fn complete_length16_code_does_not_overflow() {
        let mut bits = [0u8; 17];
        for len in 1..=15 {
            bits[len] = 1;
        }
        bits[16] = 2;
        assert!(bits_valid(&bits), "complete code must be Kraft-valid");
        let values: Vec<u8> = (0..17).collect();
        let ht = HuffmanTable::from_bits_values(bits, values);
        assert_eq!(ht.codes.len(), 17);
        assert_eq!(*ht.codes.last().unwrap(), 0xFFFF);
    }
}
