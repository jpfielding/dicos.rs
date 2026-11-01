#![no_main]

//! Fuzz target: `encode_image` → `decode_image` round-trips.
//!
//! The input is interpreted as an image description
//! ([`jpegls_fuzz::ImageSpec`]), so every input is a valid encode request.
//! The encoder must accept the request, the decoder must accept the encoder's
//! output, the geometry must survive, and every sample must come back exactly
//! (lossless) or within ±NEAR (near-lossless). Anything else is a finding.

use jpegls_fuzz::{assert_within_near, ImageSpec};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(spec) = ImageSpec::from_bytes(data) else {
        return;
    };
    let stream = spec.encode();

    let img = jpegls::decode_image(&stream).expect("decode_image rejected encode_image output");
    assert_eq!(
        (
            img.width,
            img.height,
            img.components,
            img.bits_per_sample,
            img.near
        ),
        (
            spec.width,
            spec.height,
            spec.components,
            spec.precision,
            spec.near
        ),
        "frame metadata did not survive the round-trip"
    );
    assert_within_near(&img.samples, &spec.samples, spec.near);
});
