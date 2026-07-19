//! JPEG-LS lossless codec (ISO/IEC 14495-1 / ITU-T T.87).
//!
//! Implements the LOCO-I algorithm with context-based Golomb-Rice coding
//! and run-length mode for uniform regions.

pub mod error;

// TODO(t87): the T.87 bitstream and context model are being built here but are
// not yet wired into the public encode/decode path (that lands in L3/L4). Until
// then they are exercised only by their own unit tests, so allow dead code at
// the module level rather than sprinkling per-item attributes.
#[allow(dead_code)]
mod bitstream;
#[allow(dead_code)]
mod context;
mod decode;
mod encode;
// Frozen 1.0.0 coder. Retains verbatim helpers (write_byte/write_u16be/...)
// that the public path no longer calls; see legacy.rs. DO NOT MODIFY.
#[allow(dead_code)]
mod legacy;
mod predictor;
// TODO(t87): the standalone run-mode module is superseded by L4 (run mode wired
// directly into the scan loops). It currently exercises the frozen legacy
// primitives and is otherwise unused.
#[allow(dead_code)]
mod run_mode;

pub use decode::decode;
pub use encode::encode;

/// DICOM Transfer Syntax UID for JPEG-LS Lossless.
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.80";
