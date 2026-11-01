#![no_main]

//! Fuzz target: decoding under LSE ID=1 coding-parameter presets.
//!
//! The encoder never writes an LSE segment, so no round-trip target reaches
//! the preset path, and blind byte fuzzing rarely assembles a valid SOF55 +
//! LSE + SOS header. This target builds that header from fuzz-chosen fields —
//! MAXVAL, T1–T3 and RESET either in a plausible range or as raw `u16`s, the
//! LSE before or after SOF55, an over-long LSE, an extra APPn and a DRI
//! segment — and feeds fuzz bytes as each component's scan. Any `Err` is an
//! acceptable rejection. A panic, or an `Ok` that breaks one of these, is a
//! finding:
//!
//! - the [`jpegls::DecodedImage`] contract, with no sample above the
//!   *effective* MAXVAL (the preset's, when it sets one);
//! - the frame and scan fields come back as written;
//! - the accepted preset obeys the rules the decoder documents: MAXVAL within
//!   the SOF precision, RESET >= 3, NEAR <= min(255, MAXVAL/2);
//! - a single-component stream decodes identically through `jpegls::decode`.
//!
//! Input layout (missing bytes read as 0):
//!
//! - byte 0: flags — bit 0 three components; bit 1 an APPn segment first;
//!   bit 2 a DRI segment; bit 3 the LSE after SOF55; bit 4 raw LSE fields;
//!   bit 5 an over-long LSE;
//! - byte 1: precision `2 + (b % 15)`;
//! - bytes 2, 3: width and height, `1 + (b % 32)`;
//! - byte 4: NEAR — 0 unless the top bit is set, then the low 7 bits;
//! - bytes 5–14: MAXVAL, T1, T2, T3, RESET (`u16`, little-endian);
//! - bytes 15, 16: where to split the remainder between the component scans;
//! - the rest: scan bytes.

use jpegls_fuzz::{assert_decode_agrees, assert_image_contract, max_val, Input};
use libfuzzer_sys::fuzz_target;

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&(payload.len() as u16 + 2).to_be_bytes());
    out.extend_from_slice(payload);
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let flags = input.u8();
    let nf: u8 = if flags & 1 == 1 { 3 } else { 1 };
    let precision: u8 = 2 + input.u8() % 15;
    let width = 1 + u16::from(input.u8() % 32);
    let height = 1 + u16::from(input.u8() % 32);
    let b = input.u8();
    let near: u8 = if b & 0x80 == 0 { 0 } else { b & 0x7f };
    let raw = [
        input.u16(),
        input.u16(),
        input.u16(),
        input.u16(),
        input.u16(),
    ];
    let splits = [input.u8(), input.u8()];
    let body = input.rest();

    let sof_max = max_val(precision);
    let [maxval, t1, t2, t3, reset] = if flags & 0x10 != 0 {
        raw
    } else {
        // Plausible: MAXVAL within the precision (0 = default), thresholds
        // within MAXVAL (0 = default), RESET in 3..=max(255, MAXVAL).
        let maxval = (u32::from(raw[0]) % (sof_max + 1)) as u16;
        let m = if maxval == 0 {
            sof_max
        } else {
            u32::from(maxval)
        };
        let t = |r: u16| (u32::from(r) % (m + 1)) as u16;
        let reset = 3 + (u32::from(raw[4]) % (m.max(255) - 2)) as u16;
        [maxval, t(raw[1]), t(raw[2]), t(raw[3]), reset]
    };

    let mut lse = vec![1u8];
    for v in [maxval, t1, t2, t3, reset] {
        lse.extend_from_slice(&v.to_be_bytes());
    }
    if flags & 0x20 != 0 {
        lse.extend_from_slice(&[0xA5; 3]);
    }
    let mut sof = vec![precision];
    sof.extend_from_slice(&height.to_be_bytes());
    sof.extend_from_slice(&width.to_be_bytes());
    sof.push(nf);
    for c in 1..=nf {
        sof.extend_from_slice(&[c, 0x11, 0x00]);
    }

    let mut stream = vec![0xFF, 0xD8];
    if flags & 0x02 != 0 {
        segment(&mut stream, 0xE0, b"fuzz");
    }
    if flags & 0x08 == 0 {
        segment(&mut stream, 0xF8, &lse);
    }
    segment(&mut stream, 0xF7, &sof);
    if flags & 0x08 != 0 {
        segment(&mut stream, 0xF8, &lse);
    }
    if flags & 0x04 != 0 {
        segment(&mut stream, 0xDD, &[0x00, 0x00]);
    }

    // Split the body into one scan per component at fuzz-chosen points.
    let mut cuts = vec![0usize];
    for &s in &splits[..usize::from(nf) - 1] {
        let last = *cuts.last().unwrap();
        cuts.push(last + (body.len() - last) * usize::from(s) / 255);
    }
    cuts.push(body.len());
    for c in 0..usize::from(nf) {
        segment(&mut stream, 0xDA, &[1, c as u8 + 1, 0, near, 0, 0]);
        stream.extend_from_slice(&body[cuts[c]..cuts[c + 1]]);
    }
    stream.extend_from_slice(&[0xFF, 0xD9]);

    let Ok(img) = jpegls::decode_image(&stream) else {
        return;
    };

    let effective_max = if maxval == 0 {
        sof_max
    } else {
        u32::from(maxval)
    };
    assert!(
        effective_max <= sof_max,
        "accepted LSE MAXVAL {maxval} above 2^{precision} - 1"
    );
    assert!(reset >= 3, "accepted LSE RESET {reset} < 3");
    assert!(
        u32::from(near) <= (effective_max / 2).min(255),
        "accepted NEAR {near} above min(255, MAXVAL/2) for MAXVAL {effective_max}"
    );
    assert_image_contract(&img, Some(effective_max));
    assert_eq!(
        (
            img.width,
            img.height,
            img.components,
            img.bits_per_sample,
            img.near
        ),
        (u32::from(width), u32::from(height), nf, precision, near),
        "frame or scan fields changed"
    );
    assert_decode_agrees(&stream, &img);
});
