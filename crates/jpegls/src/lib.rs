//! JPEG-LS lossless codec (ISO/IEC 14495-1 / ITU-T T.87).
//!
//! Implements the LOCO-I algorithm with context-based Golomb-Rice coding
//! and run-length mode for uniform regions.
//!
//! # Profiles
//!
//! The default [`Profile::T87`] path is ITU-T T.87 conformant. The
//! [`Profile::LegacyGo`] path reproduces the frozen 1.0.0 / Go-compatible
//! bitstream (T.81-style `0xFF00` stuffing, no run mode, uncapped Golomb) for
//! reading and writing files produced by earlier releases.

pub mod error;

// Retains a few reader/writer helpers (write_byte/write_u16be/pending_marker/…)
// exercised only by unit tests or reserved for marker handling.
#[allow(dead_code)]
mod bitstream;
mod context;
mod decode;
mod encode;
// Frozen 1.0.0 coder. Retains verbatim helpers (write_byte/write_u16be/...)
// that the public path no longer calls; see legacy.rs. DO NOT MODIFY.
#[allow(dead_code)]
mod legacy;
mod predictor;
mod run_mode;

pub use decode::{decode, decode_with_options};
pub use encode::{encode, encode_with_options};
pub use error::CodecError;

/// DICOM Transfer Syntax UID for JPEG-LS Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.80";

/// Bitstream profile selecting the entropy-coded scan format.
///
/// The two formats are not bridgeable by a single flag — single-bit stuffing
/// and the length-limited Golomb code differ from the legacy path — so the
/// choice is a whole-stream profile.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// ITU-T T.87 conformant (default).
    #[default]
    T87,
    /// Frozen 1.0.0 / Go-compatible bytes (lossless only).
    LegacyGo,
}

/// Options controlling [`encode_with_options`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EncodeOptions {
    /// Near-lossless error bound (`0` = lossless). Bounded by
    /// `min(255, MAXVAL/2)`. Must be `0` for [`Profile::LegacyGo`].
    pub near: u8,
    /// Bitstream profile.
    pub profile: Profile,
    /// Explicit sample precision in bits (`2..=16`). `None` derives it from the
    /// data (at least 8), preserving legacy behavior.
    pub precision: Option<u8>,
}

/// Options controlling [`decode_with_options`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeOptions {
    /// Bitstream profile to decode with.
    pub profile: Profile,
}
