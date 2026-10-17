#![no_main]

//! Fuzz target: `encode_image` → `decode_image` round-trips.
//!
//! The input is interpreted as an image description, so every input is a
//! valid encode request:
//!
//! - byte 0: component count (bit 0: 1 or 3);
//! - byte 1: precision `2 + (b % 15)`, i.e. `2..=16`;
//! - bytes 2, 3: width and height, `1 + (b % 64)`;
//! - byte 4: NEAR — 0 unless the top bit is set, then bounded by
//!   `min(255, MAXVAL / 2)`;
//! - the rest: sample bytes (little-endian pairs, masked to MAXVAL), cycled to
//!   fill the image.
//!
//! The encoder must accept the request, the decoder must accept the encoder's
//! output, the geometry must survive, and every sample must come back exactly
//! (lossless) or within ±NEAR (near-lossless). Anything else is a finding.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let components: u8 = if data[0] & 1 == 1 { 3 } else { 1 };
    let precision: u8 = 2 + data[1] % 15;
    let width = 1 + u32::from(data[2] % 64);
    let height = 1 + u32::from(data[3] % 64);
    let max_val = (1u32 << precision) - 1;
    let near: u8 = if data[4] & 0x80 == 0 {
        0
    } else {
        let max_near = (max_val / 2).min(255);
        (u32::from(data[4] & 0x7f) % (max_near + 1)) as u8
    };

    let n = width as usize * height as usize * usize::from(components);
    let body = &data[5..];
    let samples: Vec<u16> = (0..n)
        .map(|i| {
            if body.is_empty() {
                return 0;
            }
            let lo = body[(2 * i) % body.len()];
            let hi = body[(2 * i + 1) % body.len()];
            (u32::from(u16::from_le_bytes([lo, hi])) & max_val) as u16
        })
        .collect();

    let mut opts = jpegls::EncodeOptions::default();
    opts.near = near;
    opts.precision = Some(precision);
    let mut stream = Vec::new();
    jpegls::encode_image(&samples, width, height, components, &opts, &mut stream)
        .expect("encode_image rejected a valid request");

    let img = jpegls::decode_image(&stream).expect("decode_image rejected encode_image output");
    assert_eq!(
        (img.width, img.height, img.components, img.bits_per_sample, img.near),
        (width, height, components, precision, near),
        "frame metadata did not survive the round-trip"
    );
    if near == 0 {
        assert_eq!(img.samples, samples, "lossless round-trip changed samples");
    } else {
        for (i, (&d, &o)) in img.samples.iter().zip(&samples).enumerate() {
            let err = (i32::from(d) - i32::from(o)).unsigned_abs();
            assert!(
                err <= u32::from(near),
                "sample {i}: |{d} - {o}| = {err} > NEAR = {near}"
            );
        }
    }
});
