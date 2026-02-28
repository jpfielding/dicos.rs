//! DICOS core library -- NEMA IIC 1 v04-2023 compliant.
//!
//! This crate provides the foundation for working with DICOS
//! (Digital Imaging and Communications for Security) files used
//! in security screening imaging.
//!
//! # Modules
//!
//! - [`tag`] -- Tag constants for standard DICOM/DICOS data elements
//! - [`vr`] -- Value Representation type definitions
//! - [`transfer`] -- Transfer Syntax UID constants and properties
//! - [`types`] -- Core types: Dataset, Element, Value, PixelData
//! - [`reader`] -- DICOS file parser
//! - [`writer`] -- DICOS file writer
//! - [`codec_registry`] -- Codec lookup by name or transfer syntax
//! - [`codec`] -- Codec trait definition
//! - [`img`] -- GrayImage type for pixel buffers
//! - [`error`] -- Error types
//!
//! # Feature flags
//!
//! - `rle`: Enable RLE PackBits codec
//! - `jpegls`: Enable JPEG-LS codec
//! - `jpegli`: Enable JPEG Lossless codec
//! - `jpeg2k`: Enable JPEG 2000 codec
//! - `all-codecs`: Enable all codec crates

pub mod codec;
pub mod codec_registry;
pub mod error;
pub mod img;
pub mod reader;
pub mod tag;
pub mod transfer;
pub mod types;
pub mod vr;
pub mod writer;

// Re-export commonly used types at crate root for convenience.
pub use codec::Codec;
pub use error::CodecError;
pub use img::GrayImage;
