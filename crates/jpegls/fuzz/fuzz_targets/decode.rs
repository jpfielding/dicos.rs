#![no_main]

//! Fuzz target: the JPEG-LS decoder must never panic on arbitrary input.
//!
//! The first 4 bytes are consumed as two little-endian `u16` dimensions, each
//! clamped to `1..=512`, and the remainder is fed to `jpegls::decode` at those
//! dimensions. When the first fuzz byte is odd the same body is additionally
//! decoded under [`jpegls::Profile::LegacyGo`], so a single target exercises
//! both the T.87 and the frozen legacy bitstream paths. Any `Err` is an
//! acceptable rejection; only a panic is a finding.

use libfuzzer_sys::fuzz_target;

fn clamp_dim(bytes: [u8; 2]) -> u32 {
    u32::from(u16::from_le_bytes(bytes)).clamp(1, 512)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let width = clamp_dim([data[0], data[1]]);
    let height = clamp_dim([data[2], data[3]]);
    let body = &data[4..];

    let _ = jpegls::decode(body, width, height);

    // Odd selector byte -> also drive the frozen Go-compatible legacy path.
    if data[0] & 1 == 1 {
        let mut opts = jpegls::DecodeOptions::default();
        opts.profile = jpegls::Profile::LegacyGo;
        let _ = jpegls::decode_with_options(body, width, height, &opts);
    }
});
