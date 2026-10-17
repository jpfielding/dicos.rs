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

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(img) = jpegls::decode_image(data) else {
        return;
    };

    assert!(
        img.components == 1 || img.components == 3,
        "components = {}",
        img.components
    );
    assert_eq!(img.interleave_mode, 0);
    assert!(
        (2..=16).contains(&img.bits_per_sample),
        "bits_per_sample = {}",
        img.bits_per_sample
    );
    let expected = img.width as usize * img.height as usize * usize::from(img.components);
    assert_eq!(img.samples.len(), expected);
    let max = (1u32 << img.bits_per_sample) - 1;
    assert!(
        img.samples.iter().all(|&s| u32::from(s) <= max),
        "a sample exceeds 2^P - 1 = {max}"
    );

    if img.components == 1 {
        let (samples, w, h) = jpegls::decode(data, img.width, img.height)
            .expect("decode rejected a grayscale stream decode_image accepted");
        assert_eq!((w, h), (img.width, img.height));
        assert_eq!(samples, img.samples, "decode and decode_image disagree");
    }
});
