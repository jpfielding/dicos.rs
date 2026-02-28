#![allow(dead_code)]
//! JPEG-LS lossless codec (ISO/IEC 14495-1 / ITU-T T.87).
//!
//! Implements the LOCO-I algorithm with context-based Golomb-Rice coding
//! and run-length mode for uniform regions.

pub mod error;

mod bitstream;
mod context;
mod decode;
mod encode;
mod predictor;
mod run_mode;

pub use decode::decode;
pub use encode::encode;

/// DICOM Transfer Syntax UID for JPEG-LS Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.80";
