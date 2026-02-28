use std::io::Write;

use crate::error::CodecError;
use crate::img::GrayImage;

/// Trait for lossless image codecs used in DICOS/DICOM.
///
/// Implementors provide encode/decode for 16-bit grayscale frames.
/// All DICOS codecs are lossless -- decoded output must be pixel-identical
/// to the original input.
pub trait Codec: Send + Sync {
    /// Encode a 16-bit grayscale image to the codec's compressed format.
    fn encode(&self, img: &GrayImage<u16>, w: &mut dyn Write) -> Result<(), CodecError>;

    /// Decode compressed data back into a 16-bit grayscale image.
    fn decode(&self, data: &[u8], width: u32, height: u32) -> Result<GrayImage<u16>, CodecError>;

    /// Human-readable codec name (e.g., "RLE", "JPEG-LS").
    fn name(&self) -> &str;

    /// DICOM Transfer Syntax UID for this codec.
    fn transfer_syntax_uid(&self) -> &str;
}
