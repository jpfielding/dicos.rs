#![no_main]

//! Fuzz target: the DICOM RLE decoder must never panic on arbitrary input.
//!
//! The first 4 bytes are consumed as two little-endian `u16` dimensions, each
//! clamped to `1..=512`, and the remainder is fed to `jpegrle::decode` at those
//! dimensions. Any `Err` is an acceptable, well-typed rejection; only a panic
//! is a finding.

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
    let _ = jpegrle::decode(&data[4..], width, height);
});
