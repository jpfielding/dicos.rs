#![no_main]

//! Fuzz target: `jpegls::decode_image` must never panic on arbitrary input,
//! and whatever it accepts must satisfy the [`jpegls::DecodedImage`] contract.
//!
//! The whole input is the codestream (geometry comes from the stream). Any
//! `Err` is an acceptable rejection. A panic, or an `Ok` that breaks one of
//! the invariants below, is a finding:
//!
//! - 1 or 3 components, interleave mode 0, precision `2..=16`;
//! - exactly `width * height * components` samples, none above `2^P - 1`;
//! - a single-component stream decodes identically through `jpegls::decode`.

use jpegls_fuzz::{assert_decode_agrees, assert_image_contract};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(img) = jpegls::decode_image(data) else {
        return;
    };
    assert_image_contract(&img, None);
    assert_decode_agrees(data, &img);
});
