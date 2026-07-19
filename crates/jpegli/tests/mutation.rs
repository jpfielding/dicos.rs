//! Mutation / robustness tests.
//!
//! Property under test: no matter how the encoded stream is corrupted, the
//! decoder must **never panic** and must **never return a wrong-sized `Ok`**.
//! Every decode of a mutated stream is therefore either an `Err` (a well-typed
//! rejection) or an `Ok` holding exactly `W * H` samples with matching
//! dimensions. A panic fails the test naturally (no `catch_unwind`).
//!
//! The base stream is produced with `restart_interval_rows = 1` so RST markers
//! are present in the entropy-coded scan (exercises the restart machinery).

use jpegli::error::CodecError;
use jpegli::{decode, encode_with_options, EncodeOptions};

const W: u32 = 16;
const H: u32 = 16;

fn sample_image() -> Vec<u16> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| ((x * 7 + y * 13) & 0xFF) as u16))
        .collect()
}

fn base_stream() -> Vec<u8> {
    let mut opts = EncodeOptions::default();
    opts.precision = 8; // matches the <= 255 sample values
    opts.restart_interval_rows = 1; // one restart per MCU row -> RST markers exist
    let mut buf = Vec::new();
    encode_with_options(&sample_image(), W, H, &opts, &mut buf).expect("encode base image");
    buf
}

fn assert_wellformed(res: Result<(Vec<u16>, u32, u32), CodecError>) {
    if let Ok((px, dw, dh)) = res {
        assert_eq!((dw, dh), (W, H), "Ok returned wrong dimensions");
        assert_eq!(px.len(), (W * H) as usize, "Ok returned wrong sample count");
    }
}

/// Byte offsets of the low byte of each `RST0..RST7` marker (`0xFF 0xD0..=0xD7`).
/// Entropy `0xFF` bytes are always stuffed as `0xFF 0x00`, so `0xFF Dn` is
/// unambiguously a restart marker.
fn restart_marker_positions(buf: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == 0xFF && (0xD0..=0xD7).contains(&buf[i + 1]) {
            out.push(i + 1);
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// (a) Every truncation prefix decodes to `Err` or a correctly-sized `Ok`.
#[test]
fn truncation_prefixes_never_panic() {
    let base = base_stream();
    for len in 0..base.len() {
        assert_wellformed(decode(&base[..len], W, H));
    }
}

/// (b) Every single-bit flip in the first 64 and last 16 bytes is safe.
///
/// Regression coverage for a bug this suite originally found: a single-bit
/// flip in the DHT symbol-value table could make `decode_huffman` return an
/// SSSS magnitude category outside the legal T.81 range `0..=16`, which was
/// passed unchecked to `BitReader::read_bits` — panicking in debug builds and
/// silently reading a wrong bit count in release. The scan loop now rejects
/// SSSS > 16 with an error before reading appended bits.
#[test]
fn bit_flips_never_panic() {
    let base = base_stream();
    let len = base.len();

    let head = 64.min(len);
    for byte in 0..head {
        for bit in 0..8u8 {
            let mut m = base.clone();
            m[byte] ^= 1 << bit;
            assert_wellformed(decode(&m, W, H));
        }
    }

    let tail_start = len.saturating_sub(16);
    for byte in tail_start..len {
        for bit in 0..8u8 {
            let mut m = base.clone();
            m[byte] ^= 1 << bit;
            assert_wellformed(decode(&m, W, H));
        }
    }
}

/// (c) Chop the trailing EOI marker.
///
/// The plan expected jpegli to *require* the trailing EOI (as jpegls/jpeg2k do)
/// and reject a stream without it. It does NOT: the scan decoder zero-pads at
/// EOF ("match Go behavior", `scan.rs:167`), so once every sample has been
/// decoded from the entropy bytes the EOI is never consulted and the decode
/// still returns a correctly-sized `Ok`. This is a deliberate lenient/Go-compat
/// design choice, not a defect — so the robustness property here is only that
/// the decode stays well-formed (Err or correctly-sized Ok), never a wrong-sized
/// Ok or a panic. (Divergence from the plan's assumption is intentional and
/// documented rather than forced to `Err`.)
#[test]
fn chopped_trailing_marker_still_wellformed() {
    let base = base_stream();
    assert_eq!(
        &base[base.len() - 2..],
        &[0xFF, 0xD9],
        "expected the stream to end with the EOI marker (FF D9)"
    );
    let chopped = &base[..base.len() - 2];
    assert_wellformed(decode(chopped, W, H));
}

/// (d) Restart markers must appear in strictly increasing mod-8 order.
/// Swapping the first two (RST0 <-> RST1) violates the sequence and must
/// produce an `Err`, not a mis-decode or a panic.
#[test]
fn swapped_restart_markers_is_err() {
    let base = base_stream();
    let positions = restart_marker_positions(&base);
    assert!(
        positions.len() >= 2,
        "expected at least two restart markers in the base stream, found {}",
        positions.len()
    );
    let mut m = base.clone();
    m.swap(positions[0], positions[1]);
    assert!(
        decode(&m, W, H).is_err(),
        "decode must reject a stream with out-of-order restart markers"
    );
}
