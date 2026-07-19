//! MQ arithmetic coder state machine (ITU-T T.800 Annex C).
//!
//! Implements the binary adaptive arithmetic coder used by EBCOT
//! for encoding and decoding code-block bit-planes.

// ---------------------------------------------------------------------------
// Probability estimation table (ITU-T T.800 Table C.2)
// ---------------------------------------------------------------------------

struct MqEntry {
    qe: u16,
    nmps: usize,
    nlps: usize,
    swi: bool,
}

static MQ_TABLE: &[MqEntry] = &[
    MqEntry {
        qe: 0x5601,
        nmps: 1,
        nlps: 1,
        swi: true,
    },
    MqEntry {
        qe: 0x3401,
        nmps: 2,
        nlps: 6,
        swi: false,
    },
    MqEntry {
        qe: 0x1801,
        nmps: 3,
        nlps: 9,
        swi: false,
    },
    MqEntry {
        qe: 0x0AC1,
        nmps: 4,
        nlps: 12,
        swi: false,
    },
    MqEntry {
        qe: 0x0521,
        nmps: 5,
        nlps: 29,
        swi: false,
    },
    MqEntry {
        qe: 0x0221,
        nmps: 38,
        nlps: 33,
        swi: false,
    },
    MqEntry {
        qe: 0x5601,
        nmps: 7,
        nlps: 6,
        swi: true,
    },
    MqEntry {
        qe: 0x5401,
        nmps: 8,
        nlps: 14,
        swi: false,
    },
    MqEntry {
        qe: 0x4801,
        nmps: 9,
        nlps: 14,
        swi: false,
    },
    MqEntry {
        qe: 0x3801,
        nmps: 10,
        nlps: 14,
        swi: false,
    },
    MqEntry {
        qe: 0x3001,
        nmps: 11,
        nlps: 17,
        swi: false,
    },
    MqEntry {
        qe: 0x2401,
        nmps: 12,
        nlps: 18,
        swi: false,
    },
    MqEntry {
        qe: 0x1C01,
        nmps: 13,
        nlps: 20,
        swi: false,
    },
    MqEntry {
        qe: 0x1601,
        nmps: 29,
        nlps: 21,
        swi: false,
    },
    MqEntry {
        qe: 0x5601,
        nmps: 15,
        nlps: 14,
        swi: true,
    },
    MqEntry {
        qe: 0x5401,
        nmps: 16,
        nlps: 14,
        swi: false,
    },
    MqEntry {
        qe: 0x5101,
        nmps: 17,
        nlps: 15,
        swi: false,
    },
    MqEntry {
        qe: 0x4801,
        nmps: 18,
        nlps: 16,
        swi: false,
    },
    MqEntry {
        qe: 0x3801,
        nmps: 19,
        nlps: 17,
        swi: false,
    },
    MqEntry {
        qe: 0x3401,
        nmps: 20,
        nlps: 18,
        swi: false,
    },
    MqEntry {
        qe: 0x3001,
        nmps: 21,
        nlps: 19,
        swi: false,
    },
    MqEntry {
        qe: 0x2801,
        nmps: 22,
        nlps: 19,
        swi: false,
    },
    MqEntry {
        qe: 0x2401,
        nmps: 23,
        nlps: 20,
        swi: false,
    },
    MqEntry {
        qe: 0x2201,
        nmps: 24,
        nlps: 21,
        swi: false,
    },
    MqEntry {
        qe: 0x1C01,
        nmps: 25,
        nlps: 22,
        swi: false,
    },
    MqEntry {
        qe: 0x1801,
        nmps: 26,
        nlps: 23,
        swi: false,
    },
    MqEntry {
        qe: 0x1601,
        nmps: 27,
        nlps: 24,
        swi: false,
    },
    MqEntry {
        qe: 0x1401,
        nmps: 28,
        nlps: 25,
        swi: false,
    },
    MqEntry {
        qe: 0x1201,
        nmps: 29,
        nlps: 26,
        swi: false,
    },
    MqEntry {
        qe: 0x1101,
        nmps: 30,
        nlps: 27,
        swi: false,
    },
    MqEntry {
        qe: 0x0AC1,
        nmps: 31,
        nlps: 28,
        swi: false,
    },
    MqEntry {
        qe: 0x09C1,
        nmps: 32,
        nlps: 29,
        swi: false,
    },
    MqEntry {
        qe: 0x08A1,
        nmps: 33,
        nlps: 30,
        swi: false,
    },
    MqEntry {
        qe: 0x0521,
        nmps: 34,
        nlps: 31,
        swi: false,
    },
    MqEntry {
        qe: 0x0441,
        nmps: 35,
        nlps: 32,
        swi: false,
    },
    MqEntry {
        qe: 0x02A1,
        nmps: 36,
        nlps: 33,
        swi: false,
    },
    MqEntry {
        qe: 0x0221,
        nmps: 37,
        nlps: 34,
        swi: false,
    },
    MqEntry {
        qe: 0x0141,
        nmps: 38,
        nlps: 35,
        swi: false,
    },
    MqEntry {
        qe: 0x0111,
        nmps: 39,
        nlps: 36,
        swi: false,
    },
    MqEntry {
        qe: 0x0085,
        nmps: 40,
        nlps: 37,
        swi: false,
    },
    MqEntry {
        qe: 0x0049,
        nmps: 41,
        nlps: 38,
        swi: false,
    },
    MqEntry {
        qe: 0x0025,
        nmps: 42,
        nlps: 39,
        swi: false,
    },
    MqEntry {
        qe: 0x0015,
        nmps: 43,
        nlps: 40,
        swi: false,
    },
    MqEntry {
        qe: 0x0009,
        nmps: 44,
        nlps: 41,
        swi: false,
    },
    MqEntry {
        qe: 0x0005,
        nmps: 45,
        nlps: 42,
        swi: false,
    },
    MqEntry {
        qe: 0x0001,
        nmps: 45,
        nlps: 43,
        swi: false,
    },
    MqEntry {
        qe: 0x5601,
        nmps: 46,
        nlps: 46,
        swi: false,
    },
];

// ---------------------------------------------------------------------------
// Context state
// ---------------------------------------------------------------------------

/// MQ coder context state -- one per context label.
#[derive(Debug, Clone)]
pub struct MqState {
    /// Index into the probability estimation table.
    pub index: usize,
    /// Most probable symbol (0 or 1).
    pub mps: u8,
}

impl MqState {
    pub fn new() -> Self {
        Self { index: 0, mps: 0 }
    }

    /// Create a uniform-distribution context (used for bypass coding).
    pub fn uniform() -> Self {
        Self { index: 46, mps: 0 }
    }
}

// ---------------------------------------------------------------------------
// EBCOT context labels
// ---------------------------------------------------------------------------

/// Total number of MQ contexts used by EBCOT tier-1.
pub const NUM_MQ_CONTEXTS: usize = 19;

/// First zero-coding (significance) context. Contexts `0..=8`.
pub const CTX_ZC_START: usize = 0;

/// First sign-coding context. Contexts `9..=13`.
pub const CTX_SC_START: usize = 9;

/// First magnitude-refinement context. Contexts `14..=16`.
pub const CTX_MR_START: usize = 14;

/// Context index for run-length (cleanup) coding (EBCOT).
pub const CTX_RUN_LENGTH: usize = 17;

/// Context index for the uniform distribution (EBCOT).
pub const CTX_UNIFORM: usize = 18;

/// Allocate and initialize the default EBCOT context array (T.800 Annex D).
///
/// All contexts start at MQ table state `0` with MPS `0`, except:
/// - the all-insignificant zero-coding context (`CTX_ZC_START`) → state `4`;
/// - the run-length context (`CTX_RUN_LENGTH`) → state `3`;
/// - the uniform context (`CTX_UNIFORM`) → state `46`.
pub fn setup_default_contexts() -> Vec<MqState> {
    let mut contexts: Vec<MqState> = (0..NUM_MQ_CONTEXTS).map(|_| MqState::new()).collect();
    contexts[CTX_ZC_START] = MqState { index: 4, mps: 0 };
    contexts[CTX_RUN_LENGTH] = MqState { index: 3, mps: 0 };
    contexts[CTX_UNIFORM] = MqState::uniform();
    contexts
}

// ---------------------------------------------------------------------------
// MQ Encoder
// ---------------------------------------------------------------------------

/// MQ arithmetic encoder.
pub struct MqEncoder {
    output: Vec<u8>,
    a: u32,   // interval size
    c: u32,   // lower bound
    t: i32,   // bit counter
    l: i32,   // output length counter
    temp: u8, // temporary byte
}

impl MqEncoder {
    pub fn new() -> Self {
        Self {
            output: Vec::with_capacity(4096),
            a: 0x8000,
            c: 0,
            t: 12,
            l: -1,
            temp: 0,
        }
    }

    /// Encode a single bit using the given context.
    pub fn encode(&mut self, bit: u8, ctx: &mut MqState) {
        let entry = &MQ_TABLE[ctx.index];
        let qe = entry.qe as u32;
        self.a = self.a.wrapping_sub(qe);

        if bit == ctx.mps {
            if self.a < 0x8000 {
                if self.a < qe {
                    self.c = self.c.wrapping_add(self.a);
                    self.a = qe;
                }
                ctx.index = entry.nmps;
                self.renorm_encode();
            }
        } else {
            if self.a >= qe {
                self.c = self.c.wrapping_add(self.a);
                self.a = qe;
            }
            if entry.swi {
                ctx.mps = 1 - ctx.mps;
            }
            ctx.index = entry.nlps;
            self.renorm_encode();
        }
    }

    fn renorm_encode(&mut self) {
        while self.a < 0x8000 {
            self.a <<= 1;
            self.c <<= 1;
            self.t -= 1;
            if self.t == 0 {
                self.put_byte();
            }
        }
    }

    fn put_byte(&mut self) {
        if self.temp == 0xFF {
            // Previous byte was 0xFF -- byte-stuffing mode (7-bit extraction).
            if self.l >= 0 {
                self.emit(self.temp);
            }
            self.temp = (self.c >> 20) as u8;
            self.c &= 0xF_FFFF;
            self.t = 7;
        } else if (self.c & 0x800_0000) == 0 {
            // No carry.
            if self.l >= 0 {
                self.emit(self.temp);
            }
            self.temp = (self.c >> 19) as u8;
            self.c &= 0x7_FFFF;
            self.t = 8;
        } else {
            // Carry -- propagate into the pending temp byte.
            self.temp = self.temp.wrapping_add(1);
            if self.temp == 0xFF {
                // Carry made temp 0xFF -- switch to 7-bit stuffing mode.
                self.c &= 0x7FF_FFFF;
                if self.l >= 0 {
                    self.emit(self.temp);
                }
                self.temp = (self.c >> 20) as u8;
                self.c &= 0xF_FFFF;
                self.t = 7;
            } else {
                // Normal carry.
                self.c &= 0x7FF_FFFF;
                if self.l >= 0 {
                    self.emit(self.temp);
                }
                self.temp = (self.c >> 19) as u8;
                self.c &= 0x7_FFFF;
                self.t = 8;
            }
        }
        self.l += 1;
    }

    fn emit(&mut self, b: u8) {
        self.output.push(b);
    }

    fn setbits(&mut self) {
        let tempc = self.c.wrapping_add(self.a);
        self.c |= 0xFFFF;
        if self.c >= tempc {
            self.c = self.c.wrapping_sub(0x8000);
        }
    }

    /// Flush the encoder -- must be called after all bits are encoded.
    pub fn flush(&mut self) {
        self.setbits();
        self.c <<= self.t as u32;
        self.put_byte();
        self.c <<= self.t as u32;
        self.put_byte();
        // Emit the final pending byte, unless it is 0xFF (which would
        // require a stuff byte and is not needed at end-of-stream).
        if self.temp != 0xFF {
            self.emit(self.temp);
        }
    }

    /// Return the encoded byte stream.
    pub fn bytes(&self) -> &[u8] {
        &self.output
    }

    /// Consume the encoder and return the output buffer.
    pub fn into_bytes(self) -> Vec<u8> {
        self.output
    }

    /// Reset the encoder for reuse with a new code-block.
    pub fn reset(&mut self) {
        self.output.clear();
        self.a = 0x8000;
        self.c = 0;
        self.t = 12;
        self.l = -1;
        self.temp = 0;
    }
}

// ---------------------------------------------------------------------------
// MQ Decoder
// ---------------------------------------------------------------------------

/// MQ arithmetic decoder.
pub struct MqDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    a: u32,
    c: u32,
    t: i32,
    b: u8,
}

impl<'a> MqDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut dec = Self {
            data,
            pos: 0,
            a: 0x8000,
            c: 0,
            t: 0,
            b: 0,
        };
        dec.init();
        dec
    }

    fn init(&mut self) {
        self.b = self.next_byte();
        self.c = (self.b as u32) << 16;
        self.get_byte();
        self.c <<= 7;
        self.t -= 7;
        self.a = 0x8000;
    }

    /// Read and consume the next byte, or the synthetic `0xFF` past end-of-data
    /// (without advancing the cursor, so it cannot underflow).
    fn next_byte(&mut self) -> u8 {
        if self.pos >= self.data.len() {
            return 0xFF;
        }
        let b = self.data[self.pos];
        self.pos += 1;
        b
    }

    /// Peek the next byte without consuming it (synthetic `0xFF` past EOF).
    fn peek_byte(&self) -> u8 {
        if self.pos >= self.data.len() {
            0xFF
        } else {
            self.data[self.pos]
        }
    }

    /// BYTEIN procedure (T.800 C.3.4, Figure C.19).
    fn get_byte(&mut self) {
        if self.b == 0xFF {
            let b1 = self.peek_byte();
            if b1 > 0x8F {
                // Marker segment reached (or end-of-data, treated identically):
                // the pointer stays at the marker and 1-bits are fed by adding
                // 0xFF00 into C. This is the fix over the previous code, which
                // decremented `pos` (underflowing on empty input) and omitted
                // the `C += 0xFF00` term.
                self.c = self.c.wrapping_add(0xFF00);
                self.t = 8;
            } else {
                // Stuffed byte after 0xFF: only 7 usable bits.
                self.pos += 1;
                self.b = b1;
                self.c = self.c.wrapping_add((b1 as u32) << 9);
                self.t = 7;
            }
        } else {
            let b1 = self.peek_byte();
            // Advance only when a real byte is present; at end-of-data this
            // leaves `pos == len` and feeds the synthetic 0xFF next round.
            if self.pos < self.data.len() {
                self.pos += 1;
            }
            self.b = b1;
            self.c = self.c.wrapping_add((b1 as u32) << 8);
            self.t = 8;
        }
    }

    /// Decode a single bit using the given context.
    pub fn decode(&mut self, ctx: &mut MqState) -> u8 {
        let entry = &MQ_TABLE[ctx.index];
        let qe = entry.qe as u32;
        self.a = self.a.wrapping_sub(qe);

        let chigh = self.c >> 16;
        if chigh < self.a {
            if self.a < 0x8000 {
                self.mps_exchange(ctx, entry, qe)
            } else {
                ctx.mps
            }
        } else {
            self.lps_exchange(ctx, entry, qe)
        }
    }

    fn mps_exchange(&mut self, ctx: &mut MqState, entry: &MqEntry, qe: u32) -> u8 {
        let bit;
        if self.a < qe {
            bit = 1 - ctx.mps;
            if entry.swi {
                ctx.mps = 1 - ctx.mps;
            }
            ctx.index = entry.nlps;
        } else {
            bit = ctx.mps;
            ctx.index = entry.nmps;
        }
        self.renorm_decode();
        bit
    }

    fn lps_exchange(&mut self, ctx: &mut MqState, entry: &MqEntry, qe: u32) -> u8 {
        self.c = self.c.wrapping_sub(self.a << 16);
        let bit;
        if self.a < qe {
            bit = ctx.mps;
            self.a = qe;
            ctx.index = entry.nmps;
        } else {
            bit = 1 - ctx.mps;
            self.a = qe;
            if entry.swi {
                ctx.mps = 1 - ctx.mps;
            }
            ctx.index = entry.nlps;
        }
        self.renorm_decode();
        bit
    }

    fn renorm_decode(&mut self) {
        while self.a < 0x8000 {
            if self.t == 0 {
                self.get_byte();
            }
            self.a <<= 1;
            self.c <<= 1;
            self.t -= 1;
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
    fn mq_encode_decode_all_zeros() {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();

        let n = 100;
        for _ in 0..n {
            enc.encode(0, &mut ctx_enc[0]);
        }
        enc.flush();

        let encoded = enc.into_bytes();
        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);

        for i in 0..n {
            let bit = dec.decode(&mut ctx_dec[0]);
            assert_eq!(bit, 0, "mismatch at bit {i}");
        }
    }

    #[test]
    fn mq_encode_decode_all_ones() {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();

        let n = 100;
        for _ in 0..n {
            enc.encode(1, &mut ctx_enc[0]);
        }
        enc.flush();

        let encoded = enc.into_bytes();
        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);

        for i in 0..n {
            let bit = dec.decode(&mut ctx_dec[0]);
            assert_eq!(bit, 1, "mismatch at bit {i}");
        }
    }

    #[test]
    fn mq_encode_decode_alternating() {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();

        let n = 200;
        let pattern: Vec<u8> = (0..n).map(|i| (i % 2) as u8).collect();
        for &bit in &pattern {
            enc.encode(bit, &mut ctx_enc[0]);
        }
        enc.flush();

        let encoded = enc.into_bytes();
        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);

        for (i, &expected) in pattern.iter().enumerate() {
            let bit = dec.decode(&mut ctx_dec[0]);
            assert_eq!(bit, expected, "mismatch at bit {i}");
        }
    }

    #[test]
    fn mq_encode_decode_multiple_contexts() {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();

        // Encode using different contexts
        let data: Vec<(usize, u8)> = vec![
            (0, 1),
            (1, 0),
            (0, 1),
            (2, 1),
            (1, 0),
            (0, 0),
            (2, 1),
            (0, 1),
            (1, 1),
            (2, 0),
        ];

        for &(ctx_idx, bit) in &data {
            enc.encode(bit, &mut ctx_enc[ctx_idx]);
        }
        enc.flush();

        let encoded = enc.into_bytes();
        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);

        for (i, &(ctx_idx, expected)) in data.iter().enumerate() {
            let bit = dec.decode(&mut ctx_dec[ctx_idx]);
            assert_eq!(bit, expected, "mismatch at step {i}");
        }
    }

    #[test]
    fn mq_encode_decode_random_pattern() {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();

        // Pseudo-random pattern using a simple LCG
        let mut rng = 12345u32;
        let n = 500;
        let mut bits = Vec::with_capacity(n);
        for _ in 0..n {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let bit = ((rng >> 16) & 1) as u8;
            bits.push(bit);
            enc.encode(bit, &mut ctx_enc[0]);
        }
        enc.flush();

        let encoded = enc.into_bytes();
        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);

        for (i, &expected) in bits.iter().enumerate() {
            let bit = dec.decode(&mut ctx_dec[0]);
            assert_eq!(bit, expected, "mismatch at bit {i}");
        }
    }

    #[test]
    fn mq_encoder_reset() {
        let mut enc = MqEncoder::new();
        let mut ctx = setup_default_contexts();

        enc.encode(1, &mut ctx[0]);
        enc.flush();
        assert!(!enc.bytes().is_empty());

        enc.reset();
        assert!(enc.bytes().is_empty());
    }

    #[test]
    fn mq_initial_context_states() {
        // T.800 Annex D initialization.
        let ctx = setup_default_contexts();
        assert_eq!(ctx.len(), NUM_MQ_CONTEXTS);
        for (i, state) in ctx.iter().enumerate() {
            let expected = match i {
                CTX_ZC_START => 4,
                CTX_RUN_LENGTH => 3,
                CTX_UNIFORM => 46,
                _ => 0,
            };
            assert_eq!(state.index, expected, "context {i} initial state");
            assert_eq!(state.mps, 0, "context {i} initial MPS");
        }
    }

    /// Encode `(ctx, bit)` pairs, then decode and compare.
    fn roundtrip(seq: &[(usize, u8)]) {
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();
        for &(c, bit) in seq {
            enc.encode(bit, &mut ctx_enc[c]);
        }
        enc.flush();
        let encoded = enc.into_bytes();

        let mut ctx_dec = setup_default_contexts();
        let mut dec = MqDecoder::new(&encoded);
        for (i, &(c, expected)) in seq.iter().enumerate() {
            assert_eq!(dec.decode(&mut ctx_dec[c]), expected, "mismatch at {i}");
        }
    }

    #[test]
    fn mq_roundtrip_all_zeros_all_contexts() {
        let seq: Vec<(usize, u8)> = (0..NUM_MQ_CONTEXTS)
            .flat_map(|c| (0..500).map(move |_| (c, 0u8)))
            .collect();
        roundtrip(&seq);
    }

    #[test]
    fn mq_roundtrip_all_ones_all_contexts() {
        let seq: Vec<(usize, u8)> = (0..NUM_MQ_CONTEXTS)
            .flat_map(|c| (0..500).map(move |_| (c, 1u8)))
            .collect();
        roundtrip(&seq);
    }

    #[test]
    fn mq_roundtrip_alternating_all_contexts() {
        let seq: Vec<(usize, u8)> = (0..10_000)
            .map(|i| (i % NUM_MQ_CONTEXTS, (i % 2) as u8))
            .collect();
        roundtrip(&seq);
    }

    #[test]
    fn mq_roundtrip_random_10k_all_contexts() {
        // Deterministic LCG over 10k symbols spread across all 19 contexts.
        let mut rng = 0x2545F4914F6CDD1Du64;
        let mut seq = Vec::with_capacity(10_000);
        for _ in 0..10_000 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let c = ((rng >> 40) as usize) % NUM_MQ_CONTEXTS;
            let bit = ((rng >> 33) & 1) as u8;
            seq.push((c, bit));
        }
        roundtrip(&seq);
    }

    #[test]
    fn mq_decode_truncated_stream_no_panic() {
        // Encode a stream, then feed every truncation of it to the decoder and
        // pull more symbols than were encoded. Must never panic or underflow.
        let mut ctx_enc = setup_default_contexts();
        let mut enc = MqEncoder::new();
        let mut rng = 0x1234_5678u32;
        for _ in 0..2_000 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let c = ((rng >> 20) as usize) % NUM_MQ_CONTEXTS;
            let bit = ((rng >> 16) & 1) as u8;
            enc.encode(bit, &mut ctx_enc[c]);
        }
        enc.flush();
        let encoded = enc.into_bytes();

        for cut in 0..=encoded.len() {
            let mut ctx_dec = setup_default_contexts();
            let mut dec = MqDecoder::new(&encoded[..cut]);
            for i in 0..3_000 {
                let _ = dec.decode(&mut ctx_dec[i % NUM_MQ_CONTEXTS]);
            }
        }
    }

    #[test]
    fn mq_decode_empty_stream_no_panic() {
        let mut ctx = setup_default_contexts();
        let mut dec = MqDecoder::new(&[]);
        for i in 0..1_000 {
            let _ = dec.decode(&mut ctx[i % NUM_MQ_CONTEXTS]);
        }
    }
}
