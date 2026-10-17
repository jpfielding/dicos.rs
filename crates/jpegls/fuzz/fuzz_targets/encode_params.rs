#![no_main]

//! Fuzz target: the encoders' parameter validation, against an independent
//! oracle.
//!
//! The round-trip targets only build valid requests. This one hands `encode`,
//! `encode_with_options` and `encode_image` arbitrary parameters — zero, odd
//! and oversized component counts, dimensions of 0 / 65535 / 65536 /
//! `u32::MAX`, precisions outside `2..=16`, any NEAR, a sample buffer of the
//! wrong length, samples above MAXVAL, the `LegacyGo` profile where it is not
//! allowed — and checks the result against the rules the encoder documents:
//!
//! - the encoder accepts exactly the requests the oracle calls valid;
//! - an accepted request decodes back (within ±NEAR) when the decoder supports
//!   its component count, and is rejected as `Unsupported` when it does not;
//! - a writer that fails after a fuzz-chosen fraction of the output turns into
//!   `CodecError::Io` when the output does not fit, and changes nothing when
//!   it does.
//!
//! Input layout (missing bytes read as 0):
//!
//! - byte 0: entry point `b % 3` (`encode`, `encode_with_options`,
//!   `encode_image`); bit 7 the `LegacyGo` profile;
//! - byte 1: component count (for `encode_image`);
//! - byte 2: precision — `None`, a valid `2..=16`, or `0..=63`;
//! - byte 3: NEAR — 0, or any odd value;
//! - bytes 4–7: width and height (selector byte + value byte each);
//! - byte 8: sample-count error — 0, or `-64..=63` against the expected count;
//! - byte 9: bit 0 masks samples to MAXVAL; bits 2+ choose the writer limit;
//! - the rest: sample bytes (little-endian pairs, cycled).

use std::io::{self, Write};

use jpegls::{CodecError, DecodeOptions, EncodeOptions, Profile};
use jpegls_fuzz::{assert_within_near, max_val, near_max, samples_from, Input};
use libfuzzer_sys::fuzz_target;

/// Cap on the sample buffer, so a huge requested image stays a cheap
/// (mismatched) request rather than a huge allocation. Still above a full
/// 65535-wide row pair, so the maximum width is exercised as a valid request.
const MAX_SAMPLES: usize = 1 << 17;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Encode,
    WithOptions,
    Image,
}

/// Mostly a small valid dimension; one time in four an edge value.
fn dim(sel: u8, val: u8) -> u32 {
    if sel % 4 == 0 {
        [0, 65535, 65536, u32::MAX][usize::from(val % 4)]
    } else {
        1 + u32::from(val % 64)
    }
}

/// A sink that accepts at most `limit` bytes, then fails.
struct LimitedWriter {
    buf: Vec<u8>,
    limit: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buf.len() + data.len() > self.limit {
            return Err(io::Error::other("writer limit reached"));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn run(
    entry: Entry,
    pixels: &[u16],
    width: u32,
    height: u32,
    components: u8,
    opts: &EncodeOptions,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    match entry {
        Entry::Encode => jpegls::encode(pixels, width, height, w),
        Entry::WithOptions => jpegls::encode_with_options(pixels, width, height, opts, w),
        Entry::Image => jpegls::encode_image(pixels, width, height, components, opts, w),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let b0 = input.u8();
    let entry = [Entry::Encode, Entry::WithOptions, Entry::Image][usize::from(b0 % 3)];
    let legacy = b0 & 0x80 != 0;
    let b = input.u8();
    let components: u8 = match b % 4 {
        0 => 1,
        1 => 3,
        2 => b >> 2,
        _ => 255 - (b >> 2),
    };
    let b = input.u8();
    let precision: Option<u8> = match b % 4 {
        0 => None,
        1 | 2 => Some(2 + (b >> 2) % 15),
        _ => Some(b >> 2),
    };
    let b = input.u8();
    let near: u8 = if b & 1 == 0 { 0 } else { b };
    let (ws, wv, hs, hv) = (input.u8(), input.u8(), input.u8(), input.u8());
    let (width, height) = (dim(ws, wv), dim(hs, hv));
    let b = input.u8();
    let count_error: i64 = if b & 0x80 == 0 {
        0
    } else {
        i64::from(b & 0x7f) - 64
    };
    let flags = input.u8();
    let body = input.rest();

    // The default options are what `encode` uses; the others carry the fuzz
    // parameters (`encode` ignores them).
    let mut opts = EncodeOptions::default();
    if entry != Entry::Encode {
        opts.near = near;
        opts.precision = precision;
        opts.profile = if legacy {
            Profile::LegacyGo
        } else {
            Profile::T87
        };
    }
    let comps = if entry == Entry::Image { components } else { 1 };
    // u32::MAX * u32::MAX * 255 overflows u64, hence i128.
    let expected = i128::from(width) * i128::from(height) * i128::from(comps);
    let count = (expected + i128::from(count_error)).clamp(0, MAX_SAMPLES as i128) as usize;
    let mask = match (flags & 1 == 1, opts.precision) {
        (true, Some(p @ 2..=16)) => max_val(p),
        _ => 0xFFFF,
    };
    let pixels = samples_from(body, count, mask);

    // ---- the oracle: every rule the encoders document ----
    let max_pixel = pixels.iter().copied().max().unwrap_or(0);
    let effective: Option<u8> = match opts.precision {
        Some(p) => (2..=16).contains(&p).then_some(p),
        None => Some((16 - max_pixel.leading_zeros()).clamp(8, 16) as u8),
    };
    let valid = (entry != Entry::Image || (comps >= 1 && opts.profile == Profile::T87))
        && (1..=65535).contains(&width)
        && (1..=65535).contains(&height)
        && count as i128 == expected
        && effective.is_some_and(|p| {
            u32::from(max_pixel) <= max_val(p) && opts.near <= near_max(max_val(p))
        })
        && (opts.profile == Profile::T87 || opts.near == 0);

    let mut stream = Vec::new();
    let result = run(entry, &pixels, width, height, comps, &opts, &mut stream);
    assert_eq!(
        result.is_ok(),
        valid,
        "encoder and oracle disagree: {result:?} (entry {}, {width}x{height}x{comps}, \
         {} samples, precision {:?}, near {}, {:?})",
        b0 % 3,
        pixels.len(),
        opts.precision,
        opts.near,
        opts.profile
    );
    if !valid {
        return;
    }

    // ---- an accepted request decodes back ----
    if opts.profile == Profile::LegacyGo {
        let mut dopts = DecodeOptions::default();
        dopts.profile = Profile::LegacyGo;
        let (decoded, _, _) = jpegls::decode_with_options(&stream, width, height, &dopts)
            .expect("LegacyGo decode rejected LegacyGo encode output");
        assert_eq!(decoded, pixels, "LegacyGo round-trip changed samples");
    } else if comps == 1 || comps == 3 {
        let img = jpegls::decode_image(&stream).expect("decode_image rejected encoder output");
        assert_eq!(
            (img.width, img.height, img.components, img.near),
            (width, height, comps, opts.near),
            "frame metadata did not survive the round-trip"
        );
        assert_eq!(Some(img.bits_per_sample), effective);
        assert_within_near(&img.samples, &pixels, opts.near);
    } else {
        assert!(
            matches!(
                jpegls::decode_image(&stream),
                Err(CodecError::Unsupported(_))
            ),
            "a {comps}-component stream should be Unsupported by the decoder"
        );
    }

    // ---- a writer that fails part-way ----
    let limit = stream.len() * usize::from(flags >> 2) / 63;
    let mut sink = LimitedWriter {
        buf: Vec::new(),
        limit,
    };
    match run(entry, &pixels, width, height, comps, &opts, &mut sink) {
        Ok(()) => {
            assert!(limit >= stream.len(), "Ok although the output does not fit");
            assert_eq!(sink.buf, stream, "a second encode produced different bytes");
        }
        Err(CodecError::Io(_)) => {
            assert!(limit < stream.len(), "Io error although the output fits");
        }
        Err(e) => panic!("a failing writer surfaced as {e:?}, not CodecError::Io"),
    }
});
