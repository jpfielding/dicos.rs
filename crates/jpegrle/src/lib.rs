//! DICOM RLE PackBits lossless codec.
//!
//! Implements DICOM Part 5 Section 8.1.1 Run Length Encoding.
//! 16-bit images are split into high-byte and low-byte segments
//! for improved compression.

pub mod error;

mod decode;
mod encode;
mod packbits;

pub use decode::decode;
pub use encode::encode;

/// DICOM Transfer Syntax UID for RLE Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.5";
