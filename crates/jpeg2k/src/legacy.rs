//! FROZEN legacy (v1.0.0) tile pipeline — DO NOT MODIFY.
//!
//! This is the byte-for-byte 1.0.0 tile encoder/decoder, moved here verbatim
//! from the former `tile.rs` so the conformant T.800 pipeline (in `tile.rs`)
//! can be built without disturbing files that already exist in the wild.
//!
//! The 1.0.0 `encode` serialised raw DWT coefficients as signed 32-bit
//! big-endian integers — a non-conformant format that is nonetheless produced
//! by the shipped 1.0.0 crate and by the Go dicos codec. The public
//! `encode`/`decode` still route through this module (codestream rewiring is
//! Workstream 1 step 9), and `tests/legacy_fixtures.rs` pins this format
//! forever: any change here that alters the emitted bytes will fail those
//! byte-identity fixtures. Treat this module as immutable.
//!
//! It deliberately calls the legacy *truncating-division* DWT
//! (`dwt::forward_multi_level`/`inverse_multi_level`) — the legacy files were
//! produced with it, so decode parity depends on keeping that exact transform.

use crate::error::CodecError;

use crate::dwt;

// ---------------------------------------------------------------------------
// Tile encoder
// ---------------------------------------------------------------------------

/// Encodes a single-component tile.
pub struct TileEncoder {
    width: usize,
    height: usize,
    decomp_levels: usize,
}

impl TileEncoder {
    pub fn new(width: usize, height: usize, decomp_levels: usize) -> Self {
        Self {
            width,
            height,
            decomp_levels,
        }
    }

    /// Encode a tile (single component) and return the serialised byte stream.
    ///
    /// Format:
    /// - `width`  (2 bytes, big-endian u16)
    /// - `height` (2 bytes, big-endian u16)
    /// - `coeffs` (width * height * 4 bytes, each i32 big-endian)
    pub fn encode_tile(&self, data: &[i32]) -> Result<Vec<u8>, CodecError> {
        let expected = self.width * self.height;
        if data.len() != expected {
            return Err(CodecError::DimensionMismatch {
                expected,
                actual: data.len(),
            });
        }

        // Copy and apply forward DWT.
        let mut coeffs = data.to_vec();
        dwt::forward_multi_level(&mut coeffs, self.width, self.height, self.decomp_levels);

        // Serialise.
        let mut result = Vec::with_capacity(4 + expected * 4);
        result.push((self.width >> 8) as u8);
        result.push(self.width as u8);
        result.push((self.height >> 8) as u8);
        result.push(self.height as u8);

        for &c in &coeffs {
            result.push((c >> 24) as u8);
            result.push((c >> 16) as u8);
            result.push((c >> 8) as u8);
            result.push(c as u8);
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Tile decoder
// ---------------------------------------------------------------------------

/// Decodes a single-component tile.
pub struct TileDecoder {
    decomp_levels: usize,
}

impl TileDecoder {
    pub fn new(decomp_levels: usize) -> Self {
        Self { decomp_levels }
    }

    /// Decode a serialised tile and return the reconstructed pixel data.
    ///
    /// Returns `(width, height, pixels)`.
    pub fn decode_tile(&self, data: &[u8]) -> Result<(usize, usize, Vec<i32>), CodecError> {
        if data.len() < 4 {
            return Err(CodecError::InvalidData(
                "tile data too short for header".into(),
            ));
        }

        let width = ((data[0] as usize) << 8) | (data[1] as usize);
        let height = ((data[2] as usize) << 8) | (data[3] as usize);

        let expected_len = 4 + width * height * 4;
        if data.len() < expected_len {
            return Err(CodecError::InvalidData(format!(
                "tile data too short: expected {expected_len}, got {}",
                data.len()
            )));
        }

        let mut coeffs = Vec::with_capacity(width * height);
        let mut pos = 4;
        for _ in 0..(width * height) {
            let v = ((data[pos] as i32) << 24)
                | ((data[pos + 1] as i32) << 16)
                | ((data[pos + 2] as i32) << 8)
                | (data[pos + 3] as i32);
            coeffs.push(v);
            pos += 4;
        }

        dwt::inverse_multi_level(&mut coeffs, width, height, self.decomp_levels);

        Ok((width, height, coeffs))
    }

    /// Return the byte length consumed by a tile component's data block,
    /// given the tile data starting at the header.
    #[allow(dead_code)] // TODO(t800): legacy tile helper, used by tests only
    pub fn tile_data_len(data: &[u8]) -> Result<usize, CodecError> {
        if data.len() < 4 {
            return Err(CodecError::InvalidData(
                "tile data too short for header".into(),
            ));
        }
        let width = ((data[0] as usize) << 8) | (data[1] as usize);
        let height = ((data[2] as usize) << 8) | (data[3] as usize);
        Ok(4 + width * height * 4)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_roundtrip_basic() {
        let w = 8;
        let h = 8;
        let data: Vec<i32> = (0..(w * h) as i32).collect();

        let enc = TileEncoder::new(w, h, 3);
        let encoded = enc.encode_tile(&data).unwrap();

        let dec = TileDecoder::new(3);
        let (dw, dh, decoded) = dec.decode_tile(&encoded).unwrap();

        assert_eq!(dw, w);
        assert_eq!(dh, h);
        assert_eq!(decoded, data);
    }

    #[test]
    fn tile_roundtrip_16bit_values() {
        let w = 16;
        let h = 16;
        let data: Vec<i32> = (0..(w * h) as i32).map(|x| x * 100 + 1000).collect();

        let enc = TileEncoder::new(w, h, 4);
        let encoded = enc.encode_tile(&data).unwrap();

        let dec = TileDecoder::new(4);
        let (_, _, decoded) = dec.decode_tile(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn tile_roundtrip_odd_dims() {
        let w = 13;
        let h = 7;
        let data: Vec<i32> = (0..(w * h) as i32).collect();

        let enc = TileEncoder::new(w, h, 2);
        let encoded = enc.encode_tile(&data).unwrap();

        let dec = TileDecoder::new(2);
        let (dw, dh, decoded) = dec.decode_tile(&encoded).unwrap();
        assert_eq!(dw, w);
        assert_eq!(dh, h);
        assert_eq!(decoded, data);
    }

    #[test]
    fn tile_roundtrip_all_zeros() {
        let w = 32;
        let h = 32;
        let data = vec![0i32; w * h];

        let enc = TileEncoder::new(w, h, 5);
        let encoded = enc.encode_tile(&data).unwrap();

        let dec = TileDecoder::new(5);
        let (_, _, decoded) = dec.decode_tile(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn tile_roundtrip_constant() {
        let w = 16;
        let h = 16;
        let data = vec![12345i32; w * h];

        let enc = TileEncoder::new(w, h, 3);
        let encoded = enc.encode_tile(&data).unwrap();

        let dec = TileDecoder::new(3);
        let (_, _, decoded) = dec.decode_tile(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn tile_data_len_calculation() {
        let w = 10;
        let h = 20;
        let data: Vec<i32> = (0..(w * h) as i32).collect();

        let enc = TileEncoder::new(w, h, 2);
        let encoded = enc.encode_tile(&data).unwrap();

        let len = TileDecoder::tile_data_len(&encoded).unwrap();
        assert_eq!(len, encoded.len());
    }

    #[test]
    fn tile_decode_too_short() {
        let dec = TileDecoder::new(3);
        assert!(dec.decode_tile(&[0, 0]).is_err());
    }

    #[test]
    fn tile_encode_dimension_mismatch() {
        let enc = TileEncoder::new(4, 4, 2);
        let data = vec![0i32; 10]; // wrong size
        assert!(enc.encode_tile(&data).is_err());
    }
}
