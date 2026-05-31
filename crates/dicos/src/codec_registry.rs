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

use crate::codec::Codec;

#[cfg(any(
    feature = "rle",
    feature = "jpegls",
    feature = "jpegli",
    feature = "jpeg2k"
))]
use {crate::error::CodecError, crate::img::GrayImage, crate::transfer, std::io::Write};

// ---------------------------------------------------------------------------
// Codec adapter structs -- bridge raw codec crate APIs to the Codec trait
// ---------------------------------------------------------------------------

#[cfg(feature = "rle")]
struct RleAdapter;

#[cfg(feature = "rle")]
impl Codec for RleAdapter {
    fn encode(&self, img: &GrayImage<u16>, w: &mut dyn Write) -> Result<(), CodecError> {
        jpegrle::encode(&img.data, img.width, img.height, w)
            .map_err(|e| CodecError::InvalidData(e.to_string()))
    }

    fn decode(&self, data: &[u8], width: u32, height: u32) -> Result<GrayImage<u16>, CodecError> {
        let (pixels, w, h) = jpegrle::decode(data, width, height)
            .map_err(|e| CodecError::InvalidData(e.to_string()))?;
        GrayImage::from_data(w, h, pixels).ok_or_else(|| CodecError::DimensionMismatch {
            expected: (w as usize) * (h as usize),
            actual: 0,
        })
    }

    fn name(&self) -> &str {
        "RLE"
    }

    fn transfer_syntax_uid(&self) -> &str {
        jpegrle::TRANSFER_SYNTAX_UID
    }
}

#[cfg(feature = "jpegls")]
struct JpegLsAdapter;

#[cfg(feature = "jpegls")]
impl Codec for JpegLsAdapter {
    fn encode(&self, img: &GrayImage<u16>, w: &mut dyn Write) -> Result<(), CodecError> {
        jpegls::encode(&img.data, img.width, img.height, w)
            .map_err(|e| CodecError::InvalidData(e.to_string()))
    }

    fn decode(&self, data: &[u8], width: u32, height: u32) -> Result<GrayImage<u16>, CodecError> {
        let (pixels, w, h) = jpegls::decode(data, width, height)
            .map_err(|e| CodecError::InvalidData(e.to_string()))?;
        GrayImage::from_data(w, h, pixels).ok_or_else(|| CodecError::DimensionMismatch {
            expected: (w as usize) * (h as usize),
            actual: 0,
        })
    }

    fn name(&self) -> &str {
        "JPEG-LS"
    }

    fn transfer_syntax_uid(&self) -> &str {
        jpegls::TRANSFER_SYNTAX_UID
    }
}

#[cfg(feature = "jpegli")]
struct JpegLiAdapter;

#[cfg(feature = "jpegli")]
impl Codec for JpegLiAdapter {
    fn encode(&self, img: &GrayImage<u16>, w: &mut dyn Write) -> Result<(), CodecError> {
        jpegli::encode(&img.data, img.width, img.height, w)
            .map_err(|e| CodecError::InvalidData(e.to_string()))
    }

    fn decode(&self, data: &[u8], width: u32, height: u32) -> Result<GrayImage<u16>, CodecError> {
        let (pixels, w, h) = jpegli::decode(data, width, height)
            .map_err(|e| CodecError::InvalidData(e.to_string()))?;
        GrayImage::from_data(w, h, pixels).ok_or_else(|| CodecError::DimensionMismatch {
            expected: (w as usize) * (h as usize),
            actual: 0,
        })
    }

    fn name(&self) -> &str {
        "JPEG Lossless"
    }

    fn transfer_syntax_uid(&self) -> &str {
        jpegli::TRANSFER_SYNTAX_UID
    }
}

#[cfg(feature = "jpeg2k")]
struct Jpeg2kAdapter;

#[cfg(feature = "jpeg2k")]
impl Codec for Jpeg2kAdapter {
    fn encode(&self, img: &GrayImage<u16>, w: &mut dyn Write) -> Result<(), CodecError> {
        let opts = jpeg2k::Jpeg2kOptions::default();
        jpeg2k::encode(&img.data, img.width, img.height, &opts, w)
            .map_err(|e| CodecError::InvalidData(e.to_string()))
    }

    fn decode(&self, data: &[u8], width: u32, height: u32) -> Result<GrayImage<u16>, CodecError> {
        let (pixels, w, h) = jpeg2k::decode(data, width, height)
            .map_err(|e| CodecError::InvalidData(e.to_string()))?;
        GrayImage::from_data(w, h, pixels).ok_or_else(|| CodecError::DimensionMismatch {
            expected: (w as usize) * (h as usize),
            actual: 0,
        })
    }

    fn name(&self) -> &str {
        "JPEG 2000"
    }

    fn transfer_syntax_uid(&self) -> &str {
        jpeg2k::TRANSFER_SYNTAX_UID
    }
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
/// This is a best-effort heuristic that checks magic bytes:
/// - JPEG-LS / JPEG Lossless: starts with `FF D8`
/// - JPEG 2000: starts with `FF 4F` (codestream) or `00 00 00 0C 6A 50` (JP2 box)
/// - RLE: checked last if JPEG signatures do not match
///
/// Returns `None` if the data cannot be identified or the corresponding
/// feature is not enabled.
pub fn sniff_codec(data: &[u8]) -> Option<&'static dyn Codec> {
    if data.len() < 2 {
        return None;
    }

    // JPEG 2000 codestream: starts with FF 4F
    #[cfg(feature = "jpeg2k")]
    if data[0] == 0xFF && data[1] == 0x4F {
        return Some(&JPEG2K_CODEC);
    }

    // JPEG 2000 JP2 box format: 00 00 00 0C 6A 50
    #[cfg(feature = "jpeg2k")]
    if data.len() >= 6
        && data[0] == 0x00
        && data[1] == 0x00
        && data[2] == 0x00
        && data[3] == 0x0C
        && data[4] == 0x6A
        && data[5] == 0x50
    {
        return Some(&JPEG2K_CODEC);
    }

    // JPEG-LS / JPEG Lossless: SOF markers appear in the header, so limit
    // the scan to the first 4 KB to avoid scanning multi-MB frames. Only used
    // when the jpegls/jpegli codec features are enabled.
    #[allow(unused_variables)]
    let probe_end = data.len().min(4096);

    // JPEG-LS: starts with FF D8, then SOF55 marker (FF F7)
    #[cfg(feature = "jpegls")]
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        for i in 2..probe_end.saturating_sub(1) {
            if data[i] == 0xFF && data[i + 1] == 0xF7 {
                return Some(&JPEGLS_CODEC);
            }
        }
    }

    // JPEG Lossless: starts with FF D8, then SOF3 marker (FF C3)
    #[cfg(feature = "jpegli")]
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        for i in 2..probe_end.saturating_sub(1) {
            if data[i] == 0xFF && data[i + 1] == 0xC3 {
                return Some(&JPEGLI_CODEC);
            }
        }
    }

    // RLE: no reliable magic bytes, but if nothing else matched and data is
    // large enough for the RLE segment header (64 bytes), try RLE.
    #[cfg(feature = "rle")]
    if data.len() >= 64 {
        // RLE header starts with number of segments (1-15) as u32 LE
        let num_segments = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if (1..=15).contains(&num_segments) {
            return Some(&RLE_CODEC);
        }
    }

    None
}

/// Decode a single compressed frame using the transfer syntax to select the codec.
///
/// Falls back to sniffing the data if the transfer syntax is not recognized.
/// Returns the decoded pixel data as a `Vec<u16>`.
pub fn decode_frame(
    data: &[u8],
    width: u32,
    height: u32,
    transfer_syntax_uid: &str,
) -> Result<Vec<u16>, crate::error::DicosError> {
    let codec = codec_for_transfer_syntax(transfer_syntax_uid)
        .or_else(|| sniff_codec(data))
        .ok_or_else(|| {
            crate::error::DicosError::UnsupportedTransferSyntax(transfer_syntax_uid.to_string())
        })?;
    let img = codec
        .decode(data, width, height)
        .map_err(crate::error::DicosError::Codec)?;
    Ok(img.data)
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
