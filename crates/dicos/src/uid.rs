//! Standard SOP Class UID constants.
//!
//! These are the registered DICOM/DICOS SOP Class UIDs used to identify the
//! information object a dataset conforms to (via `MediaStorageSOPClassUID`
//! (0002,0002) and `SOPClassUID` (0008,0016)). Transfer-syntax UIDs live in
//! [`crate::transfer`]; this module is for object/SOP-class identifiers.

/// DICOS CT Image Storage SOP Class UID (NEMA IIC 1).
///
/// Identifies a dataset as a DICOS Computed Tomography image, as used in
/// security-screening CT scanners.
pub const DICOS_CT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.501.1";
