//! Legacy-format fixtures frozen from the 1.0.0 codec.
//!
//! The 1.0.0 `encode` serialised raw DWT coefficients (not conformant
//! T.800 packets). Files in that format exist in the wild (produced by this
//! crate and by the Go dicos codec), so the fixtures checked in under
//! `tests/fixtures/legacy/` pin that format forever: the legacy decode path
//! must keep decoding them to the exact pixels produced by `test_images()`.
//!
//! Regenerating (only valid while the 1.0.0 encoder is still in tree):
//! `cargo test -p pure_jpeg2k --test legacy_fixtures -- --ignored regenerate`

use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy")
}

/// Deterministic pixel generator: fixtures must be reproducible without
/// binary reference files. Do not change these images or the LCG.
fn lcg_pixels(seed: u64, n: usize, max: u16) -> Vec<u16> {
    let mut state = seed;
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as u16) % (max.saturating_add(1)).max(1)
        })
        .collect()
}

fn test_images() -> Vec<(&'static str, u32, u32, Vec<u16>)> {
    let gradient = |w: u32, h: u32| -> Vec<u16> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| ((x * 65535 / w.max(1)) + y * 7) as u16))
            .collect()
    };
    vec![
        ("gradient-16x16", 16, 16, gradient(16, 16)),
        (
            "random-32x32",
            32,
            32,
            lcg_pixels(0xD1C05, 32 * 32, u16::MAX),
        ),
        ("odd-13x7", 13, 7, lcg_pixels(0x0DD5, 13 * 7, 4095)),
        ("flat-8x8", 8, 8, vec![1234u16; 64]),
        ("tall-1x16", 1, 16, lcg_pixels(0x7A11, 16, u16::MAX)),
    ]
}

fn encode_current(pixels: &[u16], w: u32, h: u32) -> Vec<u8> {
    let opts = jpeg2k::Jpeg2kOptions::default();
    let mut out = Vec::new();
    jpeg2k::encode(pixels, w, h, &opts, &mut out).expect("legacy encode");
    out
}

/// One-time generator. Run manually; never in CI.
#[test]
#[ignore]
fn regenerate() {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for (name, w, h, pixels) in test_images() {
        let bytes = encode_current(&pixels, w, h);
        std::fs::write(dir.join(format!("{name}.j2c")), &bytes).unwrap();
    }
}

/// The legacy decode path must decode the frozen fixtures to the exact
/// generator pixels, forever.
#[test]
fn legacy_fixtures_decode() {
    for (name, w, h, pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.j2c"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "missing fixture {} ({e}) — run the regenerate test",
                path.display()
            )
        });
        let (decoded, dw, dh) = jpeg2k::decode(&bytes, w, h)
            .unwrap_or_else(|e| panic!("{name}: legacy decode failed: {e}"));
        assert_eq!((dw, dh), (w, h), "{name}: dimensions");
        assert_eq!(decoded, pixels, "{name}: pixel mismatch");
    }
}

/// Byte-identity of the legacy ENCODER against the frozen fixtures.
/// NOTE: valid only while the 1.0.0 raw-DWT encoder is the active `encode`
/// path. When the conformant T.800 encoder lands (plan WS1 step 9), delete
/// this test — the legacy format becomes decode-only for jpeg2k.
#[test]
fn legacy_fixtures_encode_identity() {
    for (name, w, h, pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.j2c"));
        let expected = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "missing fixture {} ({e}) — run the regenerate test",
                path.display()
            )
        });
        let actual = encode_current(&pixels, w, h);
        assert_eq!(
            actual, expected,
            "{name}: encoder output drifted from 1.0.0 fixture"
        );
    }
}
