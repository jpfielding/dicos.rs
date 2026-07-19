//! Codec registry for DICOS pixel data compression.
//!
//! Provides lookup functions that map codec names and transfer syntax UIDs
//! to the appropriate [`Codec`] implementations. Codecs are registered at
//! compile time via feature flags:
//!
//! - `rle` -- RLE PackBits
//! - `jpegls` -- JPEG-LS
//! - `jpegli` -- JPEG Lossless (Process 14, SV1)
//! - `jpeg2k` -- JPEG 2000
//!
//! # UID binding invariant
//!
//! Registry encodes always use each codec's **defaults** (predictor 1 for
//! JPEG Lossless, the T.87 profile for JPEG-LS, no restart intervals, the
//! default JPEG 2000 options). A DICOM Transfer Syntax UID encodes a specific
//! compression mode -- for example `1.2.840.10008.1.2.4.70` (JPEG Lossless
//! SV1) means predictor 1 specifically -- so pinning the registry to defaults
//! keeps every [`Codec::transfer_syntax_uid`] value truthful for the bytes it
//! actually produces. Callers that need non-default codec options must use the
//! individual codec crates directly and take responsibility for advertising a
//! matching transfer syntax.

use crate::codec::Codec;

#[cfg(any(
    feature = "rle",
    feature = "jpegls",
    feature = "jpegli",
    feature = "jpeg2k"
))]
use crate::transfer;

// ---------------------------------------------------------------------------
// Codec adapter macro -- bridges a raw codec crate API to the `Codec` trait.
//
// `encode` is a parameterized arm so that backends with extra arguments (such
// as jpeg2k's options) can be expressed inline. Backend errors map to
// `CodecError::Backend { codec, source }`, preserving the original error via
// `std::error::Error::source` instead of flattening to a string. The decoded
// pixel count is captured **before** the `GrayImage::from_data` move so a
// `DimensionMismatch` reports the real length.
// ---------------------------------------------------------------------------

macro_rules! codec_adapter {
    (
        struct $adapter:ident;
        display = $name:literal;
        transfer_syntax = $ts:expr;
        encode = |$pixels:ident, $width:ident, $height:ident, $writer:ident| $encode:expr;
        decode = $decode:path;
    ) => {
        struct $adapter;

        impl crate::codec::Codec for $adapter {
            fn encode(
                &self,
                img: &crate::img::GrayImage<u16>,
                w: &mut dyn std::io::Write,
            ) -> Result<(), crate::error::CodecError> {
                let $pixels = img.data();
                let $width = img.width();
                let $height = img.height();
                let $writer: &mut dyn std::io::Write = w;
                $encode.map_err(|e| crate::error::CodecError::Backend {
                    codec: $name,
                    source: Box::new(e),
                })
            }

            fn decode(
                &self,
                data: &[u8],
                width: u32,
                height: u32,
            ) -> Result<crate::img::GrayImage<u16>, crate::error::CodecError> {
                let (pixels, out_w, out_h) = $decode(data, width, height).map_err(|e| {
                    crate::error::CodecError::Backend {
                        codec: $name,
                        source: Box::new(e),
                    }
                })?;
                // Capture the real length before `pixels` is moved into the image.
                let actual = pixels.len();
                crate::img::GrayImage::from_data(out_w, out_h, pixels).ok_or(
                    crate::error::CodecError::DimensionMismatch {
                        expected: (out_w as usize) * (out_h as usize),
                        actual,
                    },
                )
            }

            fn name(&self) -> &str {
                $name
            }

            fn transfer_syntax_uid(&self) -> &str {
                $ts
            }
        }
    };
}

#[cfg(feature = "rle")]
codec_adapter! {
    struct RleAdapter;
    display = "RLE";
    transfer_syntax = jpegrle::TRANSFER_SYNTAX_UID;
    encode = |pixels, width, height, writer| jpegrle::encode(pixels, width, height, writer);
    decode = jpegrle::decode;
}

#[cfg(feature = "jpegls")]
codec_adapter! {
    struct JpegLsAdapter;
    display = "JPEG-LS";
    transfer_syntax = jpegls::TRANSFER_SYNTAX_UID;
    encode = |pixels, width, height, writer| jpegls::encode(pixels, width, height, writer);
    decode = jpegls::decode;
}

#[cfg(feature = "jpegli")]
codec_adapter! {
    struct JpegLiAdapter;
    display = "JPEG Lossless";
    transfer_syntax = jpegli::TRANSFER_SYNTAX_UID;
    encode = |pixels, width, height, writer| jpegli::encode(pixels, width, height, writer);
    decode = jpegli::decode;
}

/// Registry decodes of transfer-syntax-tagged pixel data are untrusted input,
/// so the legacy raw-DWT fallback is disabled here (`StandardOnly`): a
/// corrupted conformant stream must not be reinterpreted as legacy
/// coefficients under the same UID. Callers with pre-2.0 archives should
/// decode those frames directly via `jpeg2k::decode` (its default `Auto`
/// policy fingerprints the legacy format).
#[cfg(feature = "jpeg2k")]
fn jpeg2k_decode_standard(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<(Vec<u16>, u32, u32), jpeg2k::error::CodecError> {
    let mut opts = jpeg2k::DecodeOptions::default();
    opts.legacy = jpeg2k::LegacyPolicy::StandardOnly;
    jpeg2k::decode_with_options(data, width, height, opts)
}

#[cfg(feature = "jpeg2k")]
codec_adapter! {
    struct Jpeg2kAdapter;
    display = "JPEG 2000";
    transfer_syntax = jpeg2k::TRANSFER_SYNTAX_UID;
    encode = |pixels, width, height, writer| {
        let opts = jpeg2k::Jpeg2kOptions::default();
        jpeg2k::encode(pixels, width, height, &opts, writer)
    };
    decode = jpeg2k_decode_standard;
}

// ---------------------------------------------------------------------------
// Static codec instances
// ---------------------------------------------------------------------------

#[cfg(feature = "rle")]
static RLE_CODEC: RleAdapter = RleAdapter;

#[cfg(feature = "jpegls")]
static JPEGLS_CODEC: JpegLsAdapter = JpegLsAdapter;

#[cfg(feature = "jpegli")]
static JPEGLI_CODEC: JpegLiAdapter = JpegLiAdapter;

#[cfg(feature = "jpeg2k")]
static JPEG2K_CODEC: Jpeg2kAdapter = Jpeg2kAdapter;

/// Returns a codec by human-readable name (case-insensitive).
///
/// Recognized names include:
/// - `"rle"` -- RLE PackBits
/// - `"jpeg-ls"`, `"jpegls"` -- JPEG-LS
/// - `"jpeg-li"`, `"jpegli"`, `"jpeg-lossless"` -- JPEG Lossless
/// - `"jpeg-2000"`, `"jpeg2000"`, `"j2k"` -- JPEG 2000
///
/// Returns `None` if the name is unrecognized or the corresponding feature
/// is not enabled.
pub fn codec_by_name(name: &str) -> Option<&'static dyn Codec> {
    match name.to_ascii_lowercase().as_str() {
        #[cfg(feature = "rle")]
        "rle" => Some(&RLE_CODEC),

        #[cfg(feature = "jpegls")]
        "jpeg-ls" | "jpegls" => Some(&JPEGLS_CODEC),

        #[cfg(feature = "jpegli")]
        "jpeg-li" | "jpegli" | "jpeg-lossless" => Some(&JPEGLI_CODEC),

        #[cfg(feature = "jpeg2k")]
        "jpeg-2000" | "jpeg2000" | "j2k" => Some(&JPEG2K_CODEC),

        _ => None,
    }
}

/// Returns a codec for the given DICOM Transfer Syntax UID.
///
/// Returns `None` if the transfer syntax is uncompressed, unrecognized,
/// or the corresponding feature is not enabled.
pub fn codec_for_transfer_syntax(ts_uid: &str) -> Option<&'static dyn Codec> {
    match ts_uid {
        #[cfg(feature = "rle")]
        transfer::RLE_LOSSLESS => Some(&RLE_CODEC),

        #[cfg(feature = "jpegls")]
        transfer::JPEG_LS_LOSSLESS | transfer::JPEG_LS_NEAR_LOSSLESS => Some(&JPEGLS_CODEC),

        #[cfg(feature = "jpegli")]
        transfer::JPEG_LOSSLESS | transfer::JPEG_LOSSLESS_FIRST_ORDER => Some(&JPEGLI_CODEC),

        #[cfg(feature = "jpeg2k")]
        transfer::JPEG_2000_LOSSLESS | transfer::JPEG_2000 => Some(&JPEG2K_CODEC),

        _ => None,
    }
}

/// Attempts to identify the codec from the leading bytes of compressed data.
///
/// Signature-based detection only:
/// - JPEG 2000: the codestream SOC marker `FF 4F`. The JP2 box format is
///   intentionally **not** recognized -- the decoder is codestream-only, so
///   sniffing a JP2 box would select a decoder that cannot decode it.
/// - JPEG-LS / JPEG Lossless: a structured JPEG marker walk from the `FF D8`
///   SOI to the first Start-Of-Frame marker (`FF F7` SOF55 → JPEG-LS,
///   `FF C3` SOF3 → JPEG Lossless). The walk skips length-prefixed segments
///   (APPn, COM, etc.) and stops at SOS; malformed structure yields `None`.
///
/// RLE has no reliable signature and is therefore never sniffed -- it must be
/// selected via its transfer syntax.
///
/// Returns `None` if the data cannot be identified or the corresponding
/// feature is not enabled.
pub fn sniff_codec(data: &[u8]) -> Option<&'static dyn Codec> {
    if data.len() < 2 {
        return None;
    }

    // JPEG 2000 codestream: SOC marker FF 4F.
    #[cfg(feature = "jpeg2k")]
    if data[0] == 0xFF && data[1] == 0x4F {
        return Some(&JPEG2K_CODEC);
    }

    // JPEG family (SOI = FF D8): structured marker walk to the first SOF.
    #[cfg(any(feature = "jpegls", feature = "jpegli"))]
    if data[0] == 0xFF && data[1] == 0xD8 {
        return sniff_jpeg_sof(data);
    }

    None
}

/// Walks JPEG marker segments from the SOI to the first Start-Of-Frame marker.
///
/// Returns the codec matching the SOF (`FF F7` → JPEG-LS, `FF C3` → JPEG
/// Lossless), or `None` if SOS is reached first or the marker structure is
/// malformed. The walk is bounded by the buffer length; it does not scan the
/// entropy-coded body.
#[cfg(any(feature = "jpegls", feature = "jpegli"))]
fn sniff_jpeg_sof(data: &[u8]) -> Option<&'static dyn Codec> {
    // Start just past the SOI marker.
    let mut i = 2;
    loop {
        // Every segment begins with a 0xFF marker prefix (possibly repeated as
        // fill bytes). Anything else means the structure is malformed.
        if i >= data.len() || data[i] != 0xFF {
            return None;
        }
        while i < data.len() && data[i] == 0xFF {
            i += 1;
        }
        if i >= data.len() {
            return None;
        }
        let marker = data[i];
        i += 1;

        match marker {
            // Standalone markers with no length payload: TEM, RSTn, SOI, EOI.
            0x01 | 0xD0..=0xD9 => continue,
            // Start of Scan: the frame header is behind us; no SOF was found.
            0xDA => return None,
            // SOF3 -- JPEG Lossless.
            #[cfg(feature = "jpegli")]
            0xC3 => return Some(&JPEGLI_CODEC),
            // SOF55 -- JPEG-LS.
            #[cfg(feature = "jpegls")]
            0xF7 => return Some(&JPEGLS_CODEC),
            // Any other marker carries a 2-byte big-endian length (which
            // includes the two length bytes). Skip the whole segment.
            _ => {
                if i + 1 >= data.len() {
                    return None;
                }
                let seg_len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                if seg_len < 2 {
                    return None;
                }
                i = match i.checked_add(seg_len) {
                    Some(n) if n <= data.len() => n,
                    _ => return None,
                };
            }
        }
    }
}

/// Decode a single compressed frame, selecting the codec by transfer syntax.
///
/// An unknown or unsupported transfer syntax always yields
/// [`DicosError::UnsupportedTransferSyntax`](crate::error::DicosError::UnsupportedTransferSyntax)
/// -- there is no signature-sniffing fallback. Use [`decode_frame_sniffed`]
/// when the transfer syntax is unavailable and content detection is desired.
///
/// Returns the decoded pixel data as a `Vec<u16>`.
pub fn decode_frame(
    data: &[u8],
    width: u32,
    height: u32,
    transfer_syntax_uid: &str,
) -> Result<Vec<u16>, crate::error::DicosError> {
    let codec = codec_for_transfer_syntax(transfer_syntax_uid).ok_or_else(|| {
        crate::error::DicosError::UnsupportedTransferSyntax(transfer_syntax_uid.to_string())
    })?;
    let img = codec
        .decode(data, width, height)
        .map_err(crate::error::DicosError::Codec)?;
    Ok(img.into_data())
}

/// Decode a single compressed frame, selecting the codec by sniffing the data.
///
/// Uses [`sniff_codec`] to identify the codec from the leading bytes. Returns
/// [`DicosError::UnsupportedTransferSyntax`](crate::error::DicosError::UnsupportedTransferSyntax)
/// if the content cannot be identified. Prefer [`decode_frame`] whenever the
/// transfer syntax is known.
pub fn decode_frame_sniffed(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u16>, crate::error::DicosError> {
    let codec = sniff_codec(data).ok_or_else(|| {
        crate::error::DicosError::UnsupportedTransferSyntax(
            "unable to identify codec from data signature".to_string(),
        )
    })?;
    let img = codec
        .decode(data, width, height)
        .map_err(crate::error::DicosError::Codec)?;
    Ok(img.into_data())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer;

    #[test]
    fn unknown_codec_name_returns_none() {
        assert!(codec_by_name("unknown-codec").is_none());
        assert!(codec_by_name("").is_none());
    }

    #[test]
    fn unknown_transfer_syntax_returns_none() {
        assert!(codec_for_transfer_syntax("1.2.3.4.5.999").is_none());
        assert!(codec_for_transfer_syntax("").is_none());
    }

    #[test]
    fn uncompressed_transfer_syntax_returns_none() {
        assert!(codec_for_transfer_syntax(transfer::IMPLICIT_VR_LITTLE_ENDIAN).is_none());
        assert!(codec_for_transfer_syntax(transfer::EXPLICIT_VR_LITTLE_ENDIAN).is_none());
    }

    #[test]
    fn sniff_empty_data_returns_none() {
        assert!(sniff_codec(&[]).is_none());
        assert!(sniff_codec(&[0xFF]).is_none());
    }

    #[test]
    fn sniff_random_data_returns_none() {
        assert!(sniff_codec(&[0x00, 0x00]).is_none());
    }

    #[test]
    fn decode_frame_unknown_ts_is_unsupported_even_for_rle_shaped_data() {
        // Leading u32 LE = 1 looks like an RLE segment count, but with the RLE
        // sniff guess removed an unknown transfer syntax must never fall back.
        let rle_shaped = vec![0x01, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD];
        let err = decode_frame(&rle_shaped, 2, 2, "1.2.3.4.5.unknown").unwrap_err();
        assert!(matches!(
            err,
            crate::error::DicosError::UnsupportedTransferSyntax(_)
        ));
    }

    // -----------------------------------------------------------------------
    // Honest DimensionMismatch: exercise the macro-generated decode arm with a
    // mock backend that returns fewer pixels than its declared dimensions.
    // -----------------------------------------------------------------------

    mod mock_backend {
        use std::fmt;

        #[derive(Debug)]
        pub struct MockError;

        impl fmt::Display for MockError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("mock backend error")
            }
        }

        impl std::error::Error for MockError {}

        pub const TRANSFER_SYNTAX_UID: &str = "1.2.3.mock";

        pub fn encode(
            _pixels: &[u16],
            _width: u32,
            _height: u32,
            _w: &mut dyn std::io::Write,
        ) -> Result<(), MockError> {
            Ok(())
        }

        /// Declares a 4x4 (16-pixel) frame but returns only 9 pixels.
        pub fn decode(
            _data: &[u8],
            _width: u32,
            _height: u32,
        ) -> Result<(Vec<u16>, u32, u32), MockError> {
            Ok((vec![0u16; 9], 4, 4))
        }
    }

    codec_adapter! {
        struct MockAdapter;
        display = "MOCK";
        transfer_syntax = mock_backend::TRANSFER_SYNTAX_UID;
        encode = |pixels, width, height, writer| mock_backend::encode(pixels, width, height, writer);
        decode = mock_backend::decode;
    }

    #[test]
    fn dimension_mismatch_reports_real_decoded_length() {
        use crate::codec::Codec;

        let err = MockAdapter.decode(&[0u8; 4], 4, 4).unwrap_err();
        match err {
            crate::error::CodecError::DimensionMismatch { expected, actual } => {
                assert_eq!(expected, 16, "4x4 declares 16 pixels");
                assert_eq!(actual, 9, "actual must be the real decoded length");
            }
            other => panic!("expected DimensionMismatch, got {other:?}"),
        }
    }

    // Feature-gated tests: only run when the codec feature is enabled.

    #[cfg(feature = "rle")]
    mod rle_tests {
        use super::*;

        #[test]
        fn codec_by_name_rle() {
            let c = codec_by_name("rle").expect("rle should be available");
            assert_eq!(c.name(), "RLE");
        }

        #[test]
        fn codec_for_transfer_syntax_rle() {
            let c = codec_for_transfer_syntax(transfer::RLE_LOSSLESS)
                .expect("RLE transfer syntax should resolve");
            assert_eq!(c.transfer_syntax_uid(), transfer::RLE_LOSSLESS);
        }
    }

    #[cfg(feature = "jpegls")]
    mod jpegls_tests {
        use super::*;

        #[test]
        fn codec_by_name_jpegls() {
            let c = codec_by_name("jpeg-ls").expect("jpeg-ls should be available");
            assert_eq!(c.name(), "JPEG-LS");
            let c2 = codec_by_name("jpegls").expect("jpegls alias should work");
            assert_eq!(c2.name(), "JPEG-LS");
        }

        #[test]
        fn codec_for_transfer_syntax_jpegls() {
            let c = codec_for_transfer_syntax(transfer::JPEG_LS_LOSSLESS)
                .expect("JPEG-LS transfer syntax should resolve");
            assert_eq!(c.transfer_syntax_uid(), transfer::JPEG_LS_LOSSLESS);
        }

        #[test]
        fn sniff_walks_past_appn_segment_to_find_sof55() {
            // SOI, APP0 (length 4 covering two data bytes), then SOF55.
            let data = [
                0xFF, 0xD8, // SOI
                0xFF, 0xE0, // APP0
                0x00, 0x04, // length = 4 (2 length bytes + 2 data bytes)
                0xAA, 0xBB, // APP0 payload
                0xFF, 0xF7, // SOF55
                0x00, 0x0B, // SOF55 length (unused by sniffing)
            ];
            let c = sniff_codec(&data).expect("should sniff JPEG-LS past APP0");
            assert_eq!(c.name(), "JPEG-LS");
        }

        #[test]
        fn decode_frame_sniffed_decodes_jpegls_frame() {
            // Round-trip a small frame through the real JPEG-LS backend, then
            // decode it via signature sniffing (no transfer syntax provided).
            let pixels: Vec<u16> = vec![10, 20, 30, 40, 50, 60];
            let mut encoded = Vec::new();
            jpegls::encode(&pixels, 3, 2, &mut encoded).expect("jpegls encode");

            let decoded = decode_frame_sniffed(&encoded, 3, 2).expect("sniffed decode");
            assert_eq!(decoded, pixels);
        }
    }

    #[cfg(feature = "jpegli")]
    mod jpegli_tests {
        use super::*;

        #[test]
        fn codec_by_name_jpegli() {
            let c = codec_by_name("jpegli").expect("jpegli should be available");
            assert_eq!(c.name(), "JPEG Lossless");
        }

        #[test]
        fn codec_for_transfer_syntax_jpegli() {
            let c = codec_for_transfer_syntax(transfer::JPEG_LOSSLESS_FIRST_ORDER)
                .expect("JPEG Lossless transfer syntax should resolve");
            assert_eq!(c.transfer_syntax_uid(), transfer::JPEG_LOSSLESS_FIRST_ORDER);
        }
    }

    #[cfg(feature = "jpeg2k")]
    mod jpeg2k_tests {
        use super::*;

        #[test]
        fn codec_by_name_jpeg2k() {
            let c = codec_by_name("jpeg-2000").expect("jpeg-2000 should be available");
            assert_eq!(c.name(), "JPEG 2000");
            let c2 = codec_by_name("j2k").expect("j2k alias should work");
            assert_eq!(c2.name(), "JPEG 2000");
        }

        #[test]
        fn codec_for_transfer_syntax_jpeg2k() {
            let c = codec_for_transfer_syntax(transfer::JPEG_2000_LOSSLESS)
                .expect("JPEG 2000 transfer syntax should resolve");
            assert_eq!(c.transfer_syntax_uid(), transfer::JPEG_2000_LOSSLESS);
        }

        #[test]
        fn sniff_jpeg2k_codestream() {
            let data = vec![0xFF, 0x4F, 0xFF, 0x51, 0x00, 0x00];
            let c = sniff_codec(&data).expect("should sniff JPEG 2000");
            assert_eq!(c.name(), "JPEG 2000");
        }
    }
}
