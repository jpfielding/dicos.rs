#![allow(dead_code)]
//! JPEG 2000 Part-1 lossless codec (ITU-T T.800).
//!
//! Implements reversible 5/3 DWT, EBCOT tier-1/tier-2 coding,
//! and MQ arithmetic coding for lossless image compression.

pub mod error;

mod bitstream;
mod codestream;
mod dwt;
mod ebcot;
mod markers;
mod mq;
mod rct;
mod tile;

pub use codestream::{decode, encode};

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

/// DICOM Transfer Syntax UID for JPEG 2000 Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.90";
