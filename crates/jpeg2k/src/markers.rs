//! JPEG 2000 marker definitions and structures (ITU-T T.800 Part-1).
//!
//! Defines SOC, SIZ, COD, QCD, SOT, SOD, EOC markers and their
//! associated data structures for codestream parsing and generation.

// ---------------------------------------------------------------------------
// Marker codes (ITU-T T.800 Table A.1)
// ---------------------------------------------------------------------------

/// Start of codestream.
pub const MARKER_SOC: u16 = 0xFF4F;
/// Image and tile size.
pub const MARKER_SIZ: u16 = 0xFF51;
/// Coding style default.
pub const MARKER_COD: u16 = 0xFF52;
/// Coding style component.
pub const MARKER_COC: u16 = 0xFF53;
/// Quantization default.
pub const MARKER_QCD: u16 = 0xFF5C;
/// Quantization component.
pub const MARKER_QCC: u16 = 0xFF5D;
/// Region of interest.
pub const MARKER_RGN: u16 = 0xFF5E;
/// Progression order change.
pub const MARKER_POC: u16 = 0xFF5F;
/// Tile-part lengths, main header.
pub const MARKER_TLM: u16 = 0xFF55;
/// Packet length, main header.
pub const MARKER_PLM: u16 = 0xFF57;
/// Packet length, tile-part header.
pub const MARKER_PLT: u16 = 0xFF58;
/// Packed packet headers, main header.
pub const MARKER_PPM: u16 = 0xFF60;
/// Packed packet headers, tile-part header.
pub const MARKER_PPT: u16 = 0xFF61;
/// Start of tile-part.
pub const MARKER_SOT: u16 = 0xFF90;
/// Start of data (ITU-T T.800 Table A-1: `0xFF93`).
pub const MARKER_SOD: u16 = 0xFF93;
/// Start of data as emitted by the non-conformant v1.0.0 / Go writer (`0xFFD3`).
///
/// The legacy codestream writer used the wrong SOD code; the conformant decoder
/// accepts this value **only** on the frozen legacy decode path so archived
/// v1.0.0 files keep decoding.
pub const MARKER_SOD_LEGACY: u16 = 0xFFD3;
/// End of codestream.
pub const MARKER_EOC: u16 = 0xFFD9;
/// Component registration.
pub const MARKER_CRG: u16 = 0xFF63;
/// Comment.
pub const MARKER_COM: u16 = 0xFF64;

// ---------------------------------------------------------------------------
// Progression order
// ---------------------------------------------------------------------------

/// Progression order for JPEG 2000 codestream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProgressionOrder {
    /// Layer-Resolution-Component-Position.
    Lrcp = 0,
    /// Resolution-Layer-Component-Position.
    Rlcp = 1,
    /// Resolution-Position-Component-Layer.
    Rpcl = 2,
    /// Position-Component-Resolution-Layer.
    Pcrl = 3,
    /// Component-Position-Resolution-Layer.
    Cprl = 4,
}

impl ProgressionOrder {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Lrcp),
            1 => Some(Self::Rlcp),
            2 => Some(Self::Rpcl),
            3 => Some(Self::Pcrl),
            4 => Some(Self::Cprl),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Transform type
// ---------------------------------------------------------------------------

/// Wavelet transform type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransformType {
    /// 9/7 irreversible (lossy).
    Irreversible97 = 0,
    /// 5/3 reversible (lossless).
    Reversible53 = 1,
}

impl TransformType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Irreversible97),
            1 => Some(Self::Reversible53),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Coding style flags (ITU-T T.800 Table A.13)
// ---------------------------------------------------------------------------

/// Custom precinct sizes present (Scod bit 0).
pub const CODING_STYLE_PRECINCTS_USER: u8 = 0x01;
/// SOP marker segments used (Scod bit 1).
pub const CODING_STYLE_SOP: u8 = 0x02;
/// EPH marker segments used (Scod bit 2).
pub const CODING_STYLE_EPH: u8 = 0x04;

// ---------------------------------------------------------------------------
// Component info
// ---------------------------------------------------------------------------

/// Per-component information from the SIZ marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInfo {
    /// Bit depth (1-38).
    pub precision: u8,
    /// `true` if signed samples.
    pub signed: bool,
    /// Horizontal sub-sampling factor.
    pub x_rsiz: u8,
    /// Vertical sub-sampling factor.
    pub y_rsiz: u8,
}

// ---------------------------------------------------------------------------
// SIZ marker (ITU-T T.800 A.5.1)
// ---------------------------------------------------------------------------

/// Image and tile size parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizMarker {
    /// Capabilities required.
    pub rsiz: u16,
    /// Reference grid width.
    pub x_siz: u32,
    /// Reference grid height.
    pub y_siz: u32,
    /// Horizontal image offset.
    pub x_osiz: u32,
    /// Vertical image offset.
    pub y_osiz: u32,
    /// Tile width.
    pub x_tsiz: u32,
    /// Tile height.
    pub y_tsiz: u32,
    /// Tile horizontal offset.
    pub x_tosiz: u32,
    /// Tile vertical offset.
    pub y_tosiz: u32,
    /// Per-component info.
    pub components: Vec<ComponentInfo>,
}

impl SizMarker {
    /// Number of tiles horizontally.
    pub fn num_x_tiles(&self) -> u32 {
        (self.x_siz - self.x_tosiz).div_ceil(self.x_tsiz)
    }

    /// Number of tiles vertically.
    pub fn num_y_tiles(&self) -> u32 {
        (self.y_siz - self.y_tosiz).div_ceil(self.y_tsiz)
    }

    /// Total number of tiles.
    pub fn num_tiles(&self) -> u32 {
        self.num_x_tiles() * self.num_y_tiles()
    }
}

// ---------------------------------------------------------------------------
// COD marker (ITU-T T.800 A.6.1)
// ---------------------------------------------------------------------------

/// Coding style default parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodMarker {
    /// Coding style byte (Scod).
    pub scod: u8,
    /// Progression order.
    pub progression: ProgressionOrder,
    /// Number of quality layers.
    pub num_layers: u16,
    /// Multiple component transform (0 = none, 1 = RCT/ICT).
    pub mct: u8,
    /// Number of decomposition levels.
    pub decomp_levels: u8,
    /// Code-block width exponent offset (actual exp = value + 2).
    pub cb_width_exp: u8,
    /// Code-block height exponent offset (actual exp = value + 2).
    pub cb_height_exp: u8,
    /// Code-block style flags.
    pub cb_style: u8,
    /// Wavelet transform type.
    pub transform: TransformType,
    /// Precinct sizes (if `scod & 0x01`).
    pub precinct_sizes: Vec<u8>,
}

impl CodMarker {
    /// Actual code-block width.
    pub fn code_block_width(&self) -> usize {
        1 << (self.cb_width_exp + 2)
    }

    /// Actual code-block height.
    pub fn code_block_height(&self) -> usize {
        1 << (self.cb_height_exp + 2)
    }
}

// ---------------------------------------------------------------------------
// QCD marker (ITU-T T.800 A.6.4)
// ---------------------------------------------------------------------------

/// Quantization default parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QcdMarker {
    /// Quantization style (lower 5 bits of Sqcd).
    pub sqcd: u8,
    /// Number of guard bits (upper 3 bits of Sqcd).
    pub guard_bits: u8,
    /// Quantization step sizes / exponents.
    pub step_sizes: Vec<i16>,
}

// ---------------------------------------------------------------------------
// SOT marker (ITU-T T.800 A.4.2)
// ---------------------------------------------------------------------------

/// Tile-part header parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SotMarker {
    /// Tile index.
    pub tile_index: u16,
    /// Length of tile-part (incl. SOT marker segment).
    pub tile_part_len: u32,
    /// Tile-part index.
    pub tile_part_idx: u8,
    /// Number of tile-parts (0 = not specified).
    pub num_tile_parts: u8,
}

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

/// Build a SIZ marker for the given image dimensions and components.
pub fn build_siz(
    width: u32,
    height: u32,
    components: Vec<ComponentInfo>,
    tile_width: u32,
    tile_height: u32,
) -> SizMarker {
    let tw = if tile_width == 0 { width } else { tile_width };
    let th = if tile_height == 0 {
        height
    } else {
        tile_height
    };
    SizMarker {
        rsiz: 0,
        x_siz: width,
        y_siz: height,
        x_osiz: 0,
        y_osiz: 0,
        x_tsiz: tw,
        y_tsiz: th,
        x_tosiz: 0,
        y_tosiz: 0,
        components,
    }
}

/// Build the COD marker for the supported lossless profile.
///
/// `cb_width_exp` / `cb_height_exp` are the **actual** code-block exponents
/// (block size `2^exp`, legal range `2..=10`). Per T.800 A.6.1 the COD stores
/// them offset by `−2` (`SPcod` carries `exp − 2`), so the [`CodMarker`] fields
/// hold `exp − 2`. The profile fixes: `Scod = 0` (no precinct/SOP/EPH flags),
/// LRCP progression, one quality layer, no multi-component transform,
/// `cb_style = 0`, reversible 5/3 transform.
pub fn build_cod(num_decomp_levels: u8, cb_width_exp: u8, cb_height_exp: u8) -> CodMarker {
    CodMarker {
        scod: 0,
        progression: ProgressionOrder::Lrcp,
        num_layers: 1,
        mct: 0,
        decomp_levels: num_decomp_levels,
        cb_width_exp: cb_width_exp.saturating_sub(2),
        cb_height_exp: cb_height_exp.saturating_sub(2),
        cb_style: 0,
        transform: TransformType::Reversible53,
        precinct_sizes: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Subband
// ---------------------------------------------------------------------------

/// Identifies a subband in the DWT decomposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subband {
    /// Low-Low (approximation).
    LL,
    /// High-Low (horizontal detail).
    HL,
    /// Low-High (vertical detail).
    LH,
    /// High-High (diagonal detail).
    HH,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siz_num_tiles_single() {
        let siz = build_siz(
            256,
            256,
            vec![ComponentInfo {
                precision: 16,
                signed: false,
                x_rsiz: 1,
                y_rsiz: 1,
            }],
            0,
            0,
        );
        assert_eq!(siz.num_tiles(), 1);
    }

    #[test]
    fn siz_num_tiles_multiple() {
        let siz = build_siz(
            256,
            256,
            vec![ComponentInfo {
                precision: 16,
                signed: false,
                x_rsiz: 1,
                y_rsiz: 1,
            }],
            128,
            128,
        );
        assert_eq!(siz.num_tiles(), 4);
    }

    #[test]
    fn cod_code_block_dims() {
        // Actual exponent 6 → 64×64 blocks; the marker stores exp − 2 = 4.
        let cod = build_cod(5, 6, 6);
        assert_eq!(cod.cb_width_exp, 4);
        assert_eq!(cod.cb_height_exp, 4);
        assert_eq!(cod.code_block_width(), 64);
        assert_eq!(cod.code_block_height(), 64);
        assert_eq!(cod.num_layers, 1);
        assert_eq!(cod.mct, 0);
        assert_eq!(cod.scod, 0);
    }

    #[test]
    fn cod_smallest_blocks() {
        // Actual exponent 2 → 4×4 blocks; stored exp − 2 = 0.
        let cod = build_cod(1, 2, 2);
        assert_eq!(cod.cb_width_exp, 0);
        assert_eq!(cod.code_block_width(), 4);
    }

    #[test]
    fn progression_order_roundtrip() {
        for &b in &[0u8, 1, 2, 3, 4] {
            let p = ProgressionOrder::from_byte(b).unwrap();
            assert_eq!(p as u8, b);
        }
        assert!(ProgressionOrder::from_byte(5).is_none());
    }

    #[test]
    fn transform_type_roundtrip() {
        assert_eq!(
            TransformType::from_byte(0),
            Some(TransformType::Irreversible97)
        );
        assert_eq!(
            TransformType::from_byte(1),
            Some(TransformType::Reversible53)
        );
        assert!(TransformType::from_byte(2).is_none());
    }
}
