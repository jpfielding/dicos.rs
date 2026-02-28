//! DICOM Value Representation (VR) types.
//!
//! A VR describes the data type and format of a DICOM element's value.
//! This module defines all 31 standard VRs and provides methods for
//! querying their encoding properties.

use std::fmt;

/// DICOM Value Representation.
///
/// Each variant represents one of the 31 standard DICOM VR types as defined
/// in DICOM Part 5 Section 6.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vr {
    /// Application Entity (16 bytes max)
    AE,
    /// Age String (4 bytes fixed)
    AS,
    /// Attribute Tag (4 bytes fixed)
    AT,
    /// Code String (16 bytes max)
    CS,
    /// Date (8 bytes fixed)
    DA,
    /// Decimal String (16 bytes max)
    DS,
    /// DateTime (26 bytes max)
    DT,
    /// Floating Point Single (4 bytes fixed)
    FL,
    /// Floating Point Double (8 bytes fixed)
    FD,
    /// Integer String (12 bytes max)
    IS,
    /// Long String (64 bytes max)
    LO,
    /// Long Text (10240 bytes max)
    LT,
    /// Other Byte String
    OB,
    /// Other Double String
    OD,
    /// Other Float String
    OF,
    /// Other Long
    OL,
    /// Other Word String
    OW,
    /// Person Name (64 bytes max per component)
    PN,
    /// Short String (16 bytes max)
    SH,
    /// Signed Long (4 bytes fixed)
    SL,
    /// Sequence of Items
    SQ,
    /// Signed Short (2 bytes fixed)
    SS,
    /// Short Text (1024 bytes max)
    ST,
    /// Time (16 bytes max)
    TM,
    /// Unlimited Characters
    UC,
    /// Unique Identifier (64 bytes max)
    UI,
    /// Unsigned Long (4 bytes fixed)
    UL,
    /// Unknown
    UN,
    /// Universal Resource Identifier
    UR,
    /// Unsigned Short (2 bytes fixed)
    US,
    /// Unlimited Text
    UT,
}

impl Vr {
    /// Returns `true` if this VR contains string data.
    pub fn is_string(self) -> bool {
        matches!(
            self,
            Vr::AE
                | Vr::AS
                | Vr::CS
                | Vr::DA
                | Vr::DS
                | Vr::DT
                | Vr::IS
                | Vr::LO
                | Vr::LT
                | Vr::PN
                | Vr::SH
                | Vr::ST
                | Vr::TM
                | Vr::UC
                | Vr::UI
                | Vr::UR
                | Vr::UT
        )
    }

    /// Returns `true` if this VR contains binary data.
    pub fn is_binary(self) -> bool {
        matches!(
            self,
            Vr::AT
                | Vr::FL
                | Vr::FD
                | Vr::OB
                | Vr::OD
                | Vr::OF
                | Vr::OL
                | Vr::OW
                | Vr::SL
                | Vr::SS
                | Vr::UL
                | Vr::UN
                | Vr::US
        )
    }

    /// Returns `true` if this is a sequence VR.
    pub fn is_sequence(self) -> bool {
        self == Vr::SQ
    }

    /// Returns `true` if this VR uses a 4-byte length field in explicit VR encoding.
    ///
    /// These VRs use 2 reserved bytes followed by a 4-byte length, instead of
    /// the standard 2-byte length used by short VRs.
    pub fn is_long_vr(self) -> bool {
        matches!(
            self,
            Vr::OB | Vr::OD | Vr::OF | Vr::OL | Vr::OW | Vr::SQ | Vr::UC | Vr::UN | Vr::UR | Vr::UT
        )
    }

    /// Returns the fixed size in bytes for fixed-size VRs, or `None` for
    /// variable-length VRs.
    pub fn fixed_size(self) -> Option<usize> {
        match self {
            Vr::AT => Some(4),
            Vr::FL => Some(4),
            Vr::FD => Some(8),
            Vr::SL => Some(4),
            Vr::SS => Some(2),
            Vr::UL => Some(4),
            Vr::US => Some(2),
            _ => None,
        }
    }

    /// Parses a VR from a two-byte ASCII slice.
    ///
    /// Returns `None` if the bytes do not represent a known VR.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 2 {
            return None;
        }
        match [bytes[0], bytes[1]] {
            [b'A', b'E'] => Some(Vr::AE),
            [b'A', b'S'] => Some(Vr::AS),
            [b'A', b'T'] => Some(Vr::AT),
            [b'C', b'S'] => Some(Vr::CS),
            [b'D', b'A'] => Some(Vr::DA),
            [b'D', b'S'] => Some(Vr::DS),
            [b'D', b'T'] => Some(Vr::DT),
            [b'F', b'L'] => Some(Vr::FL),
            [b'F', b'D'] => Some(Vr::FD),
            [b'I', b'S'] => Some(Vr::IS),
            [b'L', b'O'] => Some(Vr::LO),
            [b'L', b'T'] => Some(Vr::LT),
            [b'O', b'B'] => Some(Vr::OB),
            [b'O', b'D'] => Some(Vr::OD),
            [b'O', b'F'] => Some(Vr::OF),
            [b'O', b'L'] => Some(Vr::OL),
            [b'O', b'W'] => Some(Vr::OW),
            [b'P', b'N'] => Some(Vr::PN),
            [b'S', b'H'] => Some(Vr::SH),
            [b'S', b'L'] => Some(Vr::SL),
            [b'S', b'Q'] => Some(Vr::SQ),
            [b'S', b'S'] => Some(Vr::SS),
            [b'S', b'T'] => Some(Vr::ST),
            [b'T', b'M'] => Some(Vr::TM),
            [b'U', b'C'] => Some(Vr::UC),
            [b'U', b'I'] => Some(Vr::UI),
            [b'U', b'L'] => Some(Vr::UL),
            [b'U', b'N'] => Some(Vr::UN),
            [b'U', b'R'] => Some(Vr::UR),
            [b'U', b'S'] => Some(Vr::US),
            [b'U', b'T'] => Some(Vr::UT),
            _ => None,
        }
    }

    /// Returns the two-byte ASCII representation of this VR.
    pub fn as_bytes(self) -> [u8; 2] {
        match self {
            Vr::AE => *b"AE",
            Vr::AS => *b"AS",
            Vr::AT => *b"AT",
            Vr::CS => *b"CS",
            Vr::DA => *b"DA",
            Vr::DS => *b"DS",
            Vr::DT => *b"DT",
            Vr::FL => *b"FL",
            Vr::FD => *b"FD",
            Vr::IS => *b"IS",
            Vr::LO => *b"LO",
            Vr::LT => *b"LT",
            Vr::OB => *b"OB",
            Vr::OD => *b"OD",
            Vr::OF => *b"OF",
            Vr::OL => *b"OL",
            Vr::OW => *b"OW",
            Vr::PN => *b"PN",
            Vr::SH => *b"SH",
            Vr::SL => *b"SL",
            Vr::SQ => *b"SQ",
            Vr::SS => *b"SS",
            Vr::ST => *b"ST",
            Vr::TM => *b"TM",
            Vr::UC => *b"UC",
            Vr::UI => *b"UI",
            Vr::UL => *b"UL",
            Vr::UN => *b"UN",
            Vr::UR => *b"UR",
            Vr::US => *b"US",
            Vr::UT => *b"UT",
        }
    }
}

impl fmt::Display for Vr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.as_bytes();
        write!(f, "{}{}", bytes[0] as char, bytes[1] as char)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_vrs() {
        let string_vrs = [
            Vr::AE,
            Vr::AS,
            Vr::CS,
            Vr::DA,
            Vr::DS,
            Vr::DT,
            Vr::IS,
            Vr::LO,
            Vr::LT,
            Vr::PN,
            Vr::SH,
            Vr::ST,
            Vr::TM,
            Vr::UC,
            Vr::UI,
            Vr::UR,
            Vr::UT,
        ];
        for vr in &string_vrs {
            assert!(vr.is_string(), "{vr:?} should be string");
            assert!(!vr.is_binary(), "{vr:?} should not be binary");
        }
    }

    #[test]
    fn binary_vrs() {
        let binary_vrs = [
            Vr::AT,
            Vr::FL,
            Vr::FD,
            Vr::OB,
            Vr::OD,
            Vr::OF,
            Vr::OL,
            Vr::OW,
            Vr::SL,
            Vr::SS,
            Vr::UL,
            Vr::UN,
            Vr::US,
        ];
        for vr in &binary_vrs {
            assert!(vr.is_binary(), "{vr:?} should be binary");
            assert!(!vr.is_string(), "{vr:?} should not be string");
        }
    }

    #[test]
    fn sequence_vr() {
        assert!(Vr::SQ.is_sequence());
        assert!(!Vr::US.is_sequence());
        assert!(!Vr::OB.is_sequence());
        // SQ is neither string nor binary
        assert!(!Vr::SQ.is_string());
        assert!(!Vr::SQ.is_binary());
    }

    #[test]
    fn long_vrs() {
        let long_vrs = [
            Vr::OB,
            Vr::OD,
            Vr::OF,
            Vr::OL,
            Vr::OW,
            Vr::SQ,
            Vr::UC,
            Vr::UN,
            Vr::UR,
            Vr::UT,
        ];
        for vr in &long_vrs {
            assert!(vr.is_long_vr(), "{vr:?} should be long VR");
        }

        let short_vrs = [
            Vr::AE,
            Vr::AS,
            Vr::AT,
            Vr::CS,
            Vr::DA,
            Vr::DS,
            Vr::DT,
            Vr::FL,
            Vr::FD,
            Vr::IS,
            Vr::LO,
            Vr::LT,
            Vr::PN,
            Vr::SH,
            Vr::SL,
            Vr::SS,
            Vr::ST,
            Vr::TM,
            Vr::UI,
            Vr::UL,
            Vr::US,
        ];
        for vr in &short_vrs {
            assert!(!vr.is_long_vr(), "{vr:?} should be short VR");
        }
    }

    #[test]
    fn fixed_sizes() {
        assert_eq!(Vr::AT.fixed_size(), Some(4));
        assert_eq!(Vr::FL.fixed_size(), Some(4));
        assert_eq!(Vr::FD.fixed_size(), Some(8));
        assert_eq!(Vr::SL.fixed_size(), Some(4));
        assert_eq!(Vr::SS.fixed_size(), Some(2));
        assert_eq!(Vr::UL.fixed_size(), Some(4));
        assert_eq!(Vr::US.fixed_size(), Some(2));
        assert_eq!(Vr::OB.fixed_size(), None);
        assert_eq!(Vr::LO.fixed_size(), None);
        assert_eq!(Vr::SQ.fixed_size(), None);
    }

    #[test]
    fn from_bytes_all_vrs() {
        assert_eq!(Vr::from_bytes(b"AE"), Some(Vr::AE));
        assert_eq!(Vr::from_bytes(b"AS"), Some(Vr::AS));
        assert_eq!(Vr::from_bytes(b"AT"), Some(Vr::AT));
        assert_eq!(Vr::from_bytes(b"CS"), Some(Vr::CS));
        assert_eq!(Vr::from_bytes(b"DA"), Some(Vr::DA));
        assert_eq!(Vr::from_bytes(b"DS"), Some(Vr::DS));
        assert_eq!(Vr::from_bytes(b"DT"), Some(Vr::DT));
        assert_eq!(Vr::from_bytes(b"FL"), Some(Vr::FL));
        assert_eq!(Vr::from_bytes(b"FD"), Some(Vr::FD));
        assert_eq!(Vr::from_bytes(b"IS"), Some(Vr::IS));
        assert_eq!(Vr::from_bytes(b"LO"), Some(Vr::LO));
        assert_eq!(Vr::from_bytes(b"LT"), Some(Vr::LT));
        assert_eq!(Vr::from_bytes(b"OB"), Some(Vr::OB));
        assert_eq!(Vr::from_bytes(b"OD"), Some(Vr::OD));
        assert_eq!(Vr::from_bytes(b"OF"), Some(Vr::OF));
        assert_eq!(Vr::from_bytes(b"OL"), Some(Vr::OL));
        assert_eq!(Vr::from_bytes(b"OW"), Some(Vr::OW));
        assert_eq!(Vr::from_bytes(b"PN"), Some(Vr::PN));
        assert_eq!(Vr::from_bytes(b"SH"), Some(Vr::SH));
        assert_eq!(Vr::from_bytes(b"SL"), Some(Vr::SL));
        assert_eq!(Vr::from_bytes(b"SQ"), Some(Vr::SQ));
        assert_eq!(Vr::from_bytes(b"SS"), Some(Vr::SS));
        assert_eq!(Vr::from_bytes(b"ST"), Some(Vr::ST));
        assert_eq!(Vr::from_bytes(b"TM"), Some(Vr::TM));
        assert_eq!(Vr::from_bytes(b"UC"), Some(Vr::UC));
        assert_eq!(Vr::from_bytes(b"UI"), Some(Vr::UI));
        assert_eq!(Vr::from_bytes(b"UL"), Some(Vr::UL));
        assert_eq!(Vr::from_bytes(b"UN"), Some(Vr::UN));
        assert_eq!(Vr::from_bytes(b"UR"), Some(Vr::UR));
        assert_eq!(Vr::from_bytes(b"US"), Some(Vr::US));
        assert_eq!(Vr::from_bytes(b"UT"), Some(Vr::UT));
    }

    #[test]
    fn from_bytes_invalid() {
        assert_eq!(Vr::from_bytes(b"XX"), None);
        assert_eq!(Vr::from_bytes(b"ZZ"), None);
        assert_eq!(Vr::from_bytes(b"A"), None); // too short
        assert_eq!(Vr::from_bytes(b""), None);
    }

    #[test]
    fn as_bytes_roundtrip() {
        let all = [
            Vr::AE,
            Vr::AS,
            Vr::AT,
            Vr::CS,
            Vr::DA,
            Vr::DS,
            Vr::DT,
            Vr::FL,
            Vr::FD,
            Vr::IS,
            Vr::LO,
            Vr::LT,
            Vr::OB,
            Vr::OD,
            Vr::OF,
            Vr::OL,
            Vr::OW,
            Vr::PN,
            Vr::SH,
            Vr::SL,
            Vr::SQ,
            Vr::SS,
            Vr::ST,
            Vr::TM,
            Vr::UC,
            Vr::UI,
            Vr::UL,
            Vr::UN,
            Vr::UR,
            Vr::US,
            Vr::UT,
        ];
        for vr in &all {
            let bytes = vr.as_bytes();
            let back = Vr::from_bytes(&bytes).expect("roundtrip should succeed");
            assert_eq!(*vr, back);
        }
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", Vr::US), "US");
        assert_eq!(format!("{}", Vr::OB), "OB");
        assert_eq!(format!("{}", Vr::SQ), "SQ");
    }

    #[test]
    fn every_vr_has_exactly_one_category() {
        // Every VR should be either string, binary, or sequence (but not more than one)
        let all = [
            Vr::AE,
            Vr::AS,
            Vr::AT,
            Vr::CS,
            Vr::DA,
            Vr::DS,
            Vr::DT,
            Vr::FL,
            Vr::FD,
            Vr::IS,
            Vr::LO,
            Vr::LT,
            Vr::OB,
            Vr::OD,
            Vr::OF,
            Vr::OL,
            Vr::OW,
            Vr::PN,
            Vr::SH,
            Vr::SL,
            Vr::SQ,
            Vr::SS,
            Vr::ST,
            Vr::TM,
            Vr::UC,
            Vr::UI,
            Vr::UL,
            Vr::UN,
            Vr::UR,
            Vr::US,
            Vr::UT,
        ];
        for vr in &all {
            let count = vr.is_string() as u8 + vr.is_binary() as u8 + vr.is_sequence() as u8;
            assert_eq!(
                count, 1,
                "{vr:?} should belong to exactly one category (string/binary/sequence), got {count}"
            );
        }
    }
}
