//! LegacyGo-profile fixtures frozen from the 1.0.0 codec.
//!
//! The 1.0.0 bitstream is NOT ITU-T T.87: it uses T.81-style 0xFF00 byte
//! stuffing, no run mode, and an uncapped Golomb coder — byte-compatible
//! with the Go dicos codec. Files in that format exist in the wild, so the
//! fixtures under `tests/fixtures/legacy/` pin it forever. After the T.87
//! rewrite, the `Profile::LegacyGo` encode/decode paths must stay
//! byte-identical to these fixtures.
//!
//! Regenerating (only valid while the 1.0.0 coder is the active path):
//! `cargo test -p pure_jpegls --test legacy_fixtures -- --ignored regenerate`

use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy")
}

/// Deterministic pixel generator; do not change (see module docs).
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
            lcg_pixels(0x15D1C05, 32 * 32, u16::MAX),
        ),
        ("odd-13x7", 13, 7, lcg_pixels(0x10DD5, 13 * 7, 4095)),
        // Flat image: exactly the case where conformant T.87 uses run mode
        // and the legacy format does not — the highest-value freeze.
        ("flat-24x8", 24, 8, vec![777u16; 24 * 8]),
        ("tall-1x16", 1, 16, lcg_pixels(0x17A11, 16, u16::MAX)),
    ]
}

fn encode_current(pixels: &[u16], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    jpegls::encode(pixels, w, h, &mut out).expect("legacy encode");
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
        std::fs::write(dir.join(format!("{name}.jls")), &bytes).unwrap();
    }
}

/// The legacy decode path must decode the frozen fixtures to the exact
/// generator pixels, forever. (Post-rewrite: pass Profile::LegacyGo.)
#[test]
fn legacy_fixtures_decode() {
    for (name, w, h, pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.jls"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "missing fixture {} ({e}) — run the regenerate test",
                path.display()
            )
        });
        let (decoded, dw, dh) = jpegls::decode(&bytes, w, h)
            .unwrap_or_else(|e| panic!("{name}: legacy decode failed: {e}"));
        assert_eq!((dw, dh), (w, h), "{name}: dimensions");
        assert_eq!(decoded, pixels, "{name}: pixel mismatch");
    }
}

/// Byte-identity of the LegacyGo encoder against the frozen fixtures,
/// forever. (Post-rewrite: pass Profile::LegacyGo to encode.)
#[test]
fn legacy_fixtures_encode_identity() {
    for (name, w, h, pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.jls"));
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
