#![allow(dead_code)]
//! JPEG Lossless codec (ITU-T T.81 Annex H).
//!
//! Implements DICOM Process 14 Selection Value 1 with 7 DPCM predictors
//! and Huffman entropy coding for single-component grayscale (precision
//! `2..=16`), with point transform and row-aligned restart intervals
//! (H.1.1). Verified against libjpeg-turbo >= 3.0.
//!
//! # Overview
//!
//! This crate provides a pure-Rust implementation of the JPEG Lossless
//! compression standard as used in DICOM medical imaging. The codec uses
//! differential pulse-code modulation (DPCM) with selectable predictors
//! and Huffman entropy coding.
//!
//! # Predictors
//!
//! | Selection | Formula               | Name        |
//! |-----------|-----------------------|-------------|
//! | 1         | Ra                    | Left        |
//! | 2         | Rb                    | Above       |
//! | 3         | Rc                    | Above-left  |
//! | 4         | Ra + Rb - Rc          | Linear      |
//! | 5         | Ra + (Rb - Rc) / 2    |             |
//! | 6         | Rb + (Ra - Rc) / 2    |             |
//! | 7         | (Ra + Rb) / 2         | Average     |
//!
//! # Example
//!
//! ```
//! use jpegli::{encode, decode};
//!
//! let pixels = vec![100u16, 200, 300, 400];
//! let mut buf = Vec::new();
//! encode(&pixels, 2, 2, &mut buf).unwrap();
//!
//! let (decoded, _, _) = decode(&buf, 2, 2).unwrap();
//! assert_eq!(decoded, pixels);
//! ```

pub mod error;

mod decode;
mod encode;
mod huffman;
mod scan;

pub use decode::decode;
pub use encode::{encode, encode_with_options, EncodeOptions};

/// DICOM Transfer Syntax UID for JPEG Lossless, Non-Hierarchical (Process 14, SV1).
pub const TRANSFER_SYNTAX_UID: &str = "1.2.840.10008.1.2.4.70";
