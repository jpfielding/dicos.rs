#![no_main]

//! Fuzz target: the grayscale encoder entry points round-trip, in both
//! profiles.
//!
//! `roundtrip` drives `encode_image`; this drives `encode` and
//! `encode_with_options`, the only way in to the frozen `LegacyGo` encoder and
//! to a precision derived from the data (`precision: None`). Every input is a
//! valid request:
//!
//! - byte 0: flags — bit 0 `LegacyGo` profile; bit 1 explicit precision
//!   (otherwise `None`); bit 2 near-lossless (T87 only);
//! - byte 1: explicit precision `2 + (b % 15)`;
//! - bytes 2, 3: width and height, `1 + (b % 64)`;
//! - byte 4: NEAR, bounded by `min(255, MAXVAL / 2)`;
//! - the rest: sample bytes (little-endian pairs, cycled), masked to MAXVAL
//!   for an explicit precision and left whole for a derived one.
//!
//! A finding is: the encoder rejecting the request; the matching decoder
//! rejecting its output; metadata or samples not surviving; a T87 stream that
//! differs from `encode_image` with one component (documented as identical)
//! or, at the default options, from `encode`; or either decoder panicking on
//! the other profile's stream.

use jpegls::{DecodeOptions, EncodeOptions, Profile};
use jpegls_fuzz::{assert_image_contract, assert_within_near, max_val, near_max, samples_from};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let legacy = data[0] & 1 == 1;
    let explicit = data[0] & 2 == 2;
    let precision: u8 = 2 + data[1] % 15;
    let width = 1 + u32::from(data[2] % 64);
    let height = 1 + u32::from(data[3] % 64);
    let mask = if explicit { max_val(precision) } else { 0xFFFF };
    let pixels = samples_from(&data[5..], (width * height) as usize, mask);

    // A derived precision is the bits the largest sample needs, at least 8.
    let max_pixel = pixels.iter().copied().max().unwrap_or(0);
    let derived = (16 - max_pixel.leading_zeros()).clamp(8, 16) as u8;
    let effective = if explicit { precision } else { derived };
    let near: u8 = if legacy || data[0] & 4 == 0 {
        0
    } else {
        (u16::from(data[4]) % (u16::from(near_max(max_val(effective))) + 1)) as u8
    };

    let mut opts = EncodeOptions::default();
    opts.near = near;
    opts.precision = explicit.then_some(precision);
    opts.profile = if legacy {
        Profile::LegacyGo
    } else {
        Profile::T87
    };
    let mut stream = Vec::new();
    jpegls::encode_with_options(&pixels, width, height, &opts, &mut stream)
        .expect("encode_with_options rejected a valid request");

    if legacy {
        let mut dopts = DecodeOptions::default();
        dopts.profile = Profile::LegacyGo;
        let (decoded, w, h) = jpegls::decode_with_options(&stream, width, height, &dopts)
            .expect("LegacyGo decode rejected LegacyGo encode output");
        assert_eq!((w, h), (width, height));
        assert_eq!(decoded, pixels, "LegacyGo round-trip changed samples");
        // The T87 decoder may reject a legacy stream, but must not panic.
        let _ = jpegls::decode_image(&stream);
        return;
    }

    let img = jpegls::decode_image(&stream).expect("decode_image rejected encode output");
    assert_image_contract(&img, None);
    assert_eq!(
        (
            img.width,
            img.height,
            img.components,
            img.bits_per_sample,
            img.near
        ),
        (width, height, 1, effective, near),
        "frame metadata did not survive the round-trip"
    );
    assert_within_near(&img.samples, &pixels, near);

    let (decoded, _, _) =
        jpegls::decode(&stream, width, height).expect("decode rejected encode output");
    assert_eq!(decoded, img.samples, "decode and decode_image disagree");

    let mut planar = Vec::new();
    jpegls::encode_image(&pixels, width, height, 1, &opts, &mut planar)
        .expect("encode_image rejected a request encode_with_options accepted");
    assert_eq!(
        planar, stream,
        "encode_image(components = 1) != encode_with_options"
    );

    if near == 0 && !explicit {
        let mut plain = Vec::new();
        jpegls::encode(&pixels, width, height, &mut plain)
            .expect("encode rejected a valid request");
        assert_eq!(plain, stream, "encode != encode_with_options(default)");
    }

    // The legacy decoder may reject a T87 stream, but must not panic.
    let mut dopts = DecodeOptions::default();
    dopts.profile = Profile::LegacyGo;
    let _ = jpegls::decode_with_options(&stream, width, height, &dopts);
});
