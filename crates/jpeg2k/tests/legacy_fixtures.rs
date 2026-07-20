//! Legacy-format fixtures frozen from the 1.0.0 codec.
//!
//! Only meaningful when the `legacy-decode` feature is enabled (default), which
//! is what wires up the legacy decode path this suite exercises.
#![cfg(feature = "legacy-decode")]
//!
//! The 1.0.0 `encode` serialised raw DWT coefficients (not conformant
//! T.800 packets). Files in that format exist in the wild (produced by this
//! crate and by the Go dicos codec), so the fixtures checked in under
//! `tests/fixtures/legacy/` pin that format forever: the legacy decode path
//! must keep decoding them to the exact pixels produced by `test_images()`.
//!
//! The fixtures under `tests/fixtures/legacy/` are **permanently frozen**: the
//! conformant T.800 encoder (plan WS1 step 9) has replaced the legacy raw-DWT
//! `encode`, so the crate can no longer regenerate them. They exist only to
//! guarantee the legacy *decode* path (via `LegacyPolicy::Auto`) keeps decoding
//! archived v1.0.0 / Go files. Do not edit the `.j2c` files or the generator.

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

/// The legacy decode path (reached transparently via `decode`'s default
/// `LegacyPolicy::Auto`) must decode the frozen fixtures to the exact generator
/// pixels, forever. This is the v1.0.0 / Go compatibility guarantee.
#[test]
fn legacy_fixtures_decode() {
    for (name, w, h, pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.j2c"));
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("missing frozen fixture {} ({e})", path.display()));
        let (decoded, dw, dh) = jpeg2k::decode(&bytes, w, h)
            .unwrap_or_else(|e| panic!("{name}: legacy decode failed: {e}"));
        assert_eq!((dw, dh), (w, h), "{name}: dimensions");
        assert_eq!(decoded, pixels, "{name}: pixel mismatch");
    }
}

/// `StandardOnly` must refuse the legacy fixtures (all-zero QCD exponents are
/// impossible for a conformant 16-bit stream).
#[test]
fn legacy_fixtures_rejected_by_standard_only() {
    use jpeg2k::{DecodeOptions, LegacyPolicy};
    for (name, w, h, _pixels) in test_images() {
        let path = fixture_dir().join(format!("{name}.j2c"));
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("missing frozen fixture {} ({e})", path.display()));
        let mut opts = DecodeOptions::default();
        opts.legacy = LegacyPolicy::StandardOnly;
        assert!(
            jpeg2k::decode_with_options(&bytes, w, h, opts).is_err(),
            "{name}: StandardOnly must reject the legacy fixture"
        );
    }
}
