//! JPEG 2000 Part-1 lossless codec (ITU-T T.800).
//!
//! Implements reversible 5/3 DWT, EBCOT tier-1/tier-2 coding,
//! and MQ arithmetic coding for lossless image compression.
//!
//! # Conformance scope
//!
//! Emits conformant T.800 codestreams for a single unsigned-16-bit component,
//! one tile, LRCP, one layer, `cb_style = 0`, zero grid origins; everything
//! else legal-but-unsupported is rejected via a validated support matrix.
//! Verified against OpenJPEG in both directions. 1.0.0 / Go raw-DWT files
//! remain decodable via [`LegacyPolicy`] (`Auto` by default).

use crate::error::CodecError;

pub mod error;

// `bitstream` still carries the bit-granularity `BitReader`/`BitWriter` helpers
// that the conformant pipeline does not use (packet I/O has its own stuffing bit
// codec); only `ByteReader`/`ByteWriter` are live. `rct` (multi-component
// transform) is unused while the profile is single-component MCT=0. Both keep a
// `dead_code` allow for those genuinely-unused helpers.
#[allow(dead_code)]
mod bitstream;
mod codestream;
mod dwt;
mod ebcot;
mod geometry;
// Frozen v1.0.0 raw-DWT tile pipeline, gated behind `legacy-decode`. Only its
// `TileDecoder` is live now (the conformant encoder replaced `TileEncoder`), so
// the frozen encoder half is a genuinely-unused legacy helper.
#[cfg(feature = "legacy-decode")]
#[allow(dead_code)]
mod legacy;
// `markers` exposes structural helpers (tile counts, code-block dims) that are
// part of its API surface but not all consumed by the single-tile pipeline.
#[allow(dead_code)]
mod markers;
mod mq;
mod packet;
#[allow(dead_code)]
mod rct;
mod tagtree;
mod tile;

pub use codestream::{decode, decode_with_options, encode, DecodeOptions, LegacyPolicy};

/// Options for JPEG 2000 encoding.
#[derive(Debug, Clone)]
pub struct Jpeg2kOptions {
    /// Tile width (0 = single tile)
    pub tile_width: u32,
    /// Tile height (0 = single tile)
    pub tile_height: u32,
    /// Code-block width exponent (typical: 6 for 64)
    pub cb_width_exp: u8,
    /// Code-block height exponent (typical: 6 for 64)
    pub cb_height_exp: u8,
    /// Number of decomposition levels
    pub num_decomp_levels: u8,
}

impl Default for Jpeg2kOptions {
    fn default() -> Self {
        Self {
            tile_width: 0,
            tile_height: 0,
            cb_width_exp: 6,
            cb_height_exp: 6,
            num_decomp_levels: 5,
        }
    }
}

impl Jpeg2kOptions {
    /// Validate the options against the supported T.800 profile.
    ///
    /// - Code-block width/height exponents must each be in `2..=10` and their
    ///   sum must not exceed `12` (T.800 Table A.18).
    /// - Decomposition levels must not exceed `32`.
    /// - Tiling is not supported: both `tile_width` and `tile_height` must be
    ///   `0` (single tile spanning the whole image).
    pub fn validate(&self) -> Result<(), CodecError> {
        if !(2..=10).contains(&self.cb_width_exp) {
            return Err(CodecError::InvalidData(format!(
                "cb_width_exp {} out of range 2..=10 (T.800 Table A.18)",
                self.cb_width_exp
            )));
        }
        if !(2..=10).contains(&self.cb_height_exp) {
            return Err(CodecError::InvalidData(format!(
                "cb_height_exp {} out of range 2..=10 (T.800 Table A.18)",
                self.cb_height_exp
            )));
        }
        if self.cb_width_exp + self.cb_height_exp > 12 {
            return Err(CodecError::InvalidData(format!(
                "cb_width_exp + cb_height_exp = {} exceeds 12 (T.800 Table A.18)",
                self.cb_width_exp + self.cb_height_exp
            )));
        }
        if self.num_decomp_levels > 32 {
            return Err(CodecError::InvalidData(format!(
                "num_decomp_levels {} exceeds 32",
                self.num_decomp_levels
            )));
        }
        if self.tile_width != 0 || self.tile_height != 0 {
            return Err(CodecError::Unsupported(
                "multi-tile encoding not supported".into(),
            ));
        }
        Ok(())
    }
}

/// DICOM Transfer Syntax UID for JPEG 2000 Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.90";
