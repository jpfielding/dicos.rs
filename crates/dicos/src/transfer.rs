//! DICOM Transfer Syntax definitions.
//!
//! A Transfer Syntax specifies the encoding rules for a DICOS dataset:
//! byte order, VR encoding (implicit vs explicit), and pixel data
//! compression.

use std::fmt;

/// A DICOM Transfer Syntax identified by its UID string.
///
/// Transfer syntaxes determine how data elements and pixel data are encoded
/// on disk and over the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferSyntax {
    uid: String,
}

impl TransferSyntax {
    /// Creates a new TransferSyntax from a UID string.
    pub fn new(uid: impl Into<String>) -> Self {
        Self { uid: uid.into() }
    }

    /// Returns the UID string for this transfer syntax.
    pub fn uid(&self) -> &str {
        &self.uid
    }

    /// Returns `true` if this transfer syntax uses explicit VR encoding.
    ///
    /// Only Implicit VR Little Endian uses implicit VR; all others are explicit.
    pub fn is_explicit_vr(&self) -> bool {
        self.uid != IMPLICIT_VR_LITTLE_ENDIAN
    }

    /// Returns `true` if this transfer syntax uses little-endian byte order.
    ///
    /// All standard transfer syntaxes are little-endian except the retired
    /// Explicit VR Big Endian.
    pub fn is_little_endian(&self) -> bool {
        self.uid != EXPLICIT_VR_BIG_ENDIAN
    }

    /// Returns `true` if pixel data is encapsulated (compressed).
    ///
    /// Uncompressed transfer syntaxes store pixel data in native format;
    /// all others use encapsulated frames.
    pub fn is_encapsulated(&self) -> bool {
        !matches!(
            self.uid.as_str(),
            IMPLICIT_VR_LITTLE_ENDIAN
                | EXPLICIT_VR_LITTLE_ENDIAN
                | EXPLICIT_VR_LITTLE_ENDIAN_EXT
                | EXPLICIT_VR_BIG_ENDIAN
        )
    }

    /// Returns `true` if this is a JPEG-LS transfer syntax.
    pub fn is_jpeg_ls(&self) -> bool {
        self.uid == JPEG_LS_LOSSLESS || self.uid == JPEG_LS_NEAR_LOSSLESS
    }

    /// Returns `true` if this is a JPEG Lossless transfer syntax.
    pub fn is_jpeg_lossless(&self) -> bool {
        self.uid == JPEG_LOSSLESS || self.uid == JPEG_LOSSLESS_FIRST_ORDER
    }

    /// Returns `true` if this is a JPEG 2000 transfer syntax.
    pub fn is_jpeg_2000(&self) -> bool {
        self.uid == JPEG_2000_LOSSLESS || self.uid == JPEG_2000
    }

    /// Returns `true` if this is an RLE transfer syntax.
    pub fn is_rle(&self) -> bool {
        self.uid == RLE_LOSSLESS
    }

    /// Returns a human-readable name for this transfer syntax.
    pub fn name(&self) -> &str {
        match self.uid.as_str() {
            IMPLICIT_VR_LITTLE_ENDIAN => "Implicit VR Little Endian",
            EXPLICIT_VR_LITTLE_ENDIAN => "Explicit VR Little Endian",
            EXPLICIT_VR_LITTLE_ENDIAN_EXT => "Explicit VR Little Endian Extended",
            EXPLICIT_VR_BIG_ENDIAN => "Explicit VR Big Endian (Retired)",
            JPEG_LOSSLESS => "JPEG Lossless (Process 14)",
            JPEG_LOSSLESS_FIRST_ORDER => "JPEG Lossless First-Order (Process 14, SV1)",
            JPEG_LS_LOSSLESS => "JPEG-LS Lossless",
            JPEG_LS_NEAR_LOSSLESS => "JPEG-LS Near-Lossless",
            JPEG_2000_LOSSLESS => "JPEG 2000 Lossless",
            JPEG_2000 => "JPEG 2000",
            JPEG_BASELINE => "JPEG Baseline (Process 1)",
            JPEG_EXTENDED => "JPEG Extended (Process 2 & 4)",
            RLE_LOSSLESS => "RLE Lossless",
            DEFLATED_EXPLICIT_VR => "Deflated Explicit VR Little Endian",
            _ => &self.uid,
        }
    }
}

impl fmt::Display for TransferSyntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl From<&str> for TransferSyntax {
    fn from(uid: &str) -> Self {
        Self::new(uid)
    }
}

impl From<String> for TransferSyntax {
    fn from(uid: String) -> Self {
        Self { uid }
    }
}

// ---------------------------------------------------------------------------
// Transfer Syntax UID constants
// ---------------------------------------------------------------------------

/// Implicit VR Little Endian (default DICOM transfer syntax).
pub const IMPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2";
/// Explicit VR Little Endian.
pub const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
/// Explicit VR Little Endian Extended (>4 GB support).
pub const EXPLICIT_VR_LITTLE_ENDIAN_EXT: &str = "1.2.840.10008.1.2.1.64";
/// Explicit VR Big Endian (Retired).
pub const EXPLICIT_VR_BIG_ENDIAN: &str = "1.2.840.10008.1.2.2";

/// JPEG Lossless, Non-Hierarchical (Process 14).
pub const JPEG_LOSSLESS: &str = "1.2.840.10008.1.2.4.57";
/// JPEG Lossless, Non-Hierarchical, First-Order (Process 14, SV1).
pub const JPEG_LOSSLESS_FIRST_ORDER: &str = "1.2.840.10008.1.2.4.70";

/// JPEG-LS Lossless.
pub const JPEG_LS_LOSSLESS: &str = "1.2.840.10008.1.2.4.80";
/// JPEG-LS Near-Lossless.
pub const JPEG_LS_NEAR_LOSSLESS: &str = "1.2.840.10008.1.2.4.81";

/// JPEG 2000 Lossless Only.
pub const JPEG_2000_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
/// JPEG 2000 (lossless or lossy).
pub const JPEG_2000: &str = "1.2.840.10008.1.2.4.91";

/// JPEG Baseline (Process 1).
pub const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
/// JPEG Extended (Process 2 & 4).
pub const JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";

/// RLE Lossless.
pub const RLE_LOSSLESS: &str = "1.2.840.10008.1.2.5";
/// Deflated Explicit VR Little Endian.
pub const DEFLATED_EXPLICIT_VR: &str = "1.2.840.10008.1.2.1.99";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_vr_little_endian_properties() {
        let ts = TransferSyntax::new(IMPLICIT_VR_LITTLE_ENDIAN);
        assert!(!ts.is_explicit_vr());
        assert!(ts.is_little_endian());
        assert!(!ts.is_encapsulated());
        assert!(!ts.is_jpeg_ls());
        assert!(!ts.is_jpeg_lossless());
        assert!(!ts.is_jpeg_2000());
        assert!(!ts.is_rle());
        assert_eq!(ts.name(), "Implicit VR Little Endian");
    }

    #[test]
    fn explicit_vr_little_endian_properties() {
        let ts = TransferSyntax::new(EXPLICIT_VR_LITTLE_ENDIAN);
        assert!(ts.is_explicit_vr());
        assert!(ts.is_little_endian());
        assert!(!ts.is_encapsulated());
        assert_eq!(ts.name(), "Explicit VR Little Endian");
    }

    #[test]
    fn explicit_vr_big_endian_properties() {
        let ts = TransferSyntax::new(EXPLICIT_VR_BIG_ENDIAN);
        assert!(ts.is_explicit_vr());
        assert!(!ts.is_little_endian());
        assert!(!ts.is_encapsulated());
        assert_eq!(ts.name(), "Explicit VR Big Endian (Retired)");
    }

    #[test]
    fn jpeg_ls_lossless_properties() {
        let ts = TransferSyntax::new(JPEG_LS_LOSSLESS);
        assert!(ts.is_explicit_vr());
        assert!(ts.is_little_endian());
        assert!(ts.is_encapsulated());
        assert!(ts.is_jpeg_ls());
        assert!(!ts.is_jpeg_lossless());
        assert!(!ts.is_jpeg_2000());
        assert!(!ts.is_rle());
        assert_eq!(ts.name(), "JPEG-LS Lossless");
    }

    #[test]
    fn jpeg_ls_near_lossless_properties() {
        let ts = TransferSyntax::new(JPEG_LS_NEAR_LOSSLESS);
        assert!(ts.is_jpeg_ls());
        assert!(ts.is_encapsulated());
        assert_eq!(ts.name(), "JPEG-LS Near-Lossless");
    }

    #[test]
    fn jpeg_lossless_properties() {
        let ts = TransferSyntax::new(JPEG_LOSSLESS_FIRST_ORDER);
        assert!(ts.is_jpeg_lossless());
        assert!(ts.is_encapsulated());
        assert!(!ts.is_jpeg_ls());
        assert_eq!(ts.name(), "JPEG Lossless First-Order (Process 14, SV1)");
    }

    #[test]
    fn jpeg_2000_lossless_properties() {
        let ts = TransferSyntax::new(JPEG_2000_LOSSLESS);
        assert!(ts.is_jpeg_2000());
        assert!(ts.is_encapsulated());
        assert!(!ts.is_jpeg_ls());
        assert!(!ts.is_jpeg_lossless());
        assert_eq!(ts.name(), "JPEG 2000 Lossless");
    }

    #[test]
    fn jpeg_2000_properties() {
        let ts = TransferSyntax::new(JPEG_2000);
        assert!(ts.is_jpeg_2000());
        assert!(ts.is_encapsulated());
    }

    #[test]
    fn rle_lossless_properties() {
        let ts = TransferSyntax::new(RLE_LOSSLESS);
        assert!(ts.is_rle());
        assert!(ts.is_encapsulated());
        assert!(!ts.is_jpeg_ls());
        assert!(!ts.is_jpeg_2000());
        assert_eq!(ts.name(), "RLE Lossless");
    }

    #[test]
    fn deflated_explicit_vr_properties() {
        let ts = TransferSyntax::new(DEFLATED_EXPLICIT_VR);
        assert!(ts.is_explicit_vr());
        assert!(ts.is_little_endian());
        assert!(ts.is_encapsulated());
        assert_eq!(ts.name(), "Deflated Explicit VR Little Endian");
    }

    #[test]
    fn extended_little_endian_properties() {
        let ts = TransferSyntax::new(EXPLICIT_VR_LITTLE_ENDIAN_EXT);
        assert!(ts.is_explicit_vr());
        assert!(ts.is_little_endian());
        assert!(!ts.is_encapsulated());
        assert_eq!(ts.name(), "Explicit VR Little Endian Extended");
    }

    #[test]
    fn unknown_transfer_syntax_name() {
        let ts = TransferSyntax::new("1.2.3.4.5.999");
        assert_eq!(ts.name(), "1.2.3.4.5.999");
        assert_eq!(ts.uid(), "1.2.3.4.5.999");
    }

    #[test]
    fn from_str() {
        let ts: TransferSyntax = EXPLICIT_VR_LITTLE_ENDIAN.into();
        assert_eq!(ts.uid(), EXPLICIT_VR_LITTLE_ENDIAN);
    }

    #[test]
    fn display() {
        let ts = TransferSyntax::new(RLE_LOSSLESS);
        assert_eq!(format!("{ts}"), "RLE Lossless");
    }

    #[test]
    fn all_compressed_syntaxes_are_encapsulated() {
        let compressed = [
            JPEG_LOSSLESS,
            JPEG_LOSSLESS_FIRST_ORDER,
            JPEG_LS_LOSSLESS,
            JPEG_LS_NEAR_LOSSLESS,
            JPEG_2000_LOSSLESS,
            JPEG_2000,
            JPEG_BASELINE,
            JPEG_EXTENDED,
            RLE_LOSSLESS,
            DEFLATED_EXPLICIT_VR,
        ];
        for uid in &compressed {
            let ts = TransferSyntax::new(*uid);
            assert!(ts.is_encapsulated(), "{uid} should be encapsulated");
        }
    }

    #[test]
    fn uncompressed_syntaxes_are_not_encapsulated() {
        let uncompressed = [
            IMPLICIT_VR_LITTLE_ENDIAN,
            EXPLICIT_VR_LITTLE_ENDIAN,
            EXPLICIT_VR_LITTLE_ENDIAN_EXT,
            EXPLICIT_VR_BIG_ENDIAN,
        ];
        for uid in &uncompressed {
            let ts = TransferSyntax::new(*uid);
            assert!(!ts.is_encapsulated(), "{uid} should NOT be encapsulated");
        }
    }
}
