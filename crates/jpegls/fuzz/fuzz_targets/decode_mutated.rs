#![no_main]

//! Fuzz target: the decoders on corrupted copies of valid streams.
//!
//! `decode` and `decode_image` start from raw bytes, so without a seed corpus
//! most inputs die in the header parser and the entropy decoder is reached
//! only by luck. This target encodes a fuzz-described image (the
//! `roundtrip` layout, [`jpegls_fuzz::ImageSpec`]) and then applies a few
//! fuzz-chosen edits — overwrite, XOR, insert, delete, truncate, insert a
//! marker-like `0xFF xx` pair — so nearly every input is a well-formed header
//! over a damaged scan, the case a corrupt DICOM fragment presents.
//!
//! Every decoder entry must not panic. An `Ok` from `decode_image` must satisfy
//! the [`jpegls::DecodedImage`] contract and agree with `decode`. If the edits
//! leave the stream unchanged it must still decode to the original samples.
//!
//! Input layout: byte 0 is the edit count (`b % 9`), then 4 bytes per edit —
//! op, position (`u16`, little-endian, reduced modulo the stream length),
//! value — then the image description.

use jpegls::{DecodeOptions, Profile};
use jpegls_fuzz::{
    assert_decode_agrees, assert_image_contract, assert_within_near, ImageSpec, Input,
};
use libfuzzer_sys::fuzz_target;

fn apply(stream: &mut Vec<u8>, op: u8, pos: u16, val: u8) {
    let len = stream.len();
    let at = |n: usize| usize::from(pos) % n.max(1);
    match op % 6 {
        0 if len > 0 => stream[at(len)] = val,
        1 if len > 0 => stream[at(len)] ^= val | 1,
        2 => stream.insert(at(len + 1), val),
        3 if len > 0 => {
            stream.remove(at(len));
        }
        4 => stream.truncate(at(len + 1)),
        5 => {
            let i = at(len + 1);
            stream.splice(i..i, [0xFF, val]);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let edits: Vec<(u8, u16, u8)> = (0..input.u8() % 9)
        .map(|_| (input.u8(), input.u16(), input.u8()))
        .collect();
    let Some(spec) = ImageSpec::from_bytes(input.rest()) else {
        return;
    };
    let original = spec.encode();
    let mut stream = original.clone();
    for &(op, pos, val) in &edits {
        apply(&mut stream, op, pos, val);
    }

    let mut legacy = DecodeOptions::default();
    legacy.profile = Profile::LegacyGo;
    let _ = jpegls::decode(&stream, spec.width, spec.height);
    let _ = jpegls::decode_with_options(&stream, spec.width, spec.height, &legacy);

    let result = jpegls::decode_image(&stream);
    if stream == original {
        let img = result
            .as_ref()
            .expect("decode_image rejected an unmodified stream");
        assert_within_near(&img.samples, &spec.samples, spec.near);
    }
    let Ok(img) = result else {
        return;
    };
    assert_image_contract(&img, None);
    assert_decode_agrees(&stream, &img);
});
