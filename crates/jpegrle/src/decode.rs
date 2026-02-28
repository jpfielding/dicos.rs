use crate::error::CodecError;
use crate::packbits::decode_packbits;

/// DICOM RLE header size in bytes.
const HEADER_SIZE: usize = 64;

/// Decode DICOM RLE compressed data into a 16-bit grayscale pixel buffer.
///
/// The input `data` contains the 64-byte RLE header followed by
/// PackBits-compressed byte-plane segments. For 16-bit images,
/// segment 1 is the high-byte plane and segment 2 is the low-byte plane.
///
/// `width` and `height` must be provided since RLE streams do not
/// encode image dimensions.
///
/// Returns `(pixels, width, height)` where pixels is a row-major `Vec<u16>`.
pub fn decode(data: &[u8], width: u32, height: u32) -> Result<(Vec<u16>, u32, u32), CodecError> {
    if data.len() < HEADER_SIZE {
        return Err(CodecError::InvalidData(
            "RLE data too short for header".into(),
        ));
    }

    // Parse header
    let num_segments = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if num_segments == 0 {
        return Err(CodecError::InvalidData("RLE: zero segments".into()));
    }
    if num_segments > 15 {
        return Err(CodecError::InvalidData(format!(
            "RLE: invalid segment count {num_segments}"
        )));
    }

    let mut offsets = [0u32; 15];
    for i in 0..15 {
        let start = 4 + i * 4;
        offsets[i] = u32::from_le_bytes(data[start..start + 4].try_into().unwrap());
    }

    let num_pixels = (width as usize) * (height as usize);

    // Decode each segment
    let mut segments = Vec::with_capacity(num_segments as usize);
    for i in 0..num_segments as usize {
        let start = offsets[i] as usize;
        let end = if i < (num_segments as usize) - 1 {
            offsets[i + 1] as usize
        } else {
            data.len()
        };

        if start > data.len() || end > data.len() || start > end {
            return Err(CodecError::InvalidData(format!(
                "RLE: invalid segment {i} offsets (start={start}, end={end}, data_len={})",
                data.len()
            )));
        }

        let seg_data = &data[start..end];
        let decoded = decode_packbits(seg_data, num_pixels).map_err(|e| {
            CodecError::InvalidData(format!("RLE: failed to decode segment {i}: {e}"))
        })?;

        if decoded.len() != num_pixels {
            return Err(CodecError::DimensionMismatch {
                expected: num_pixels,
                actual: decoded.len(),
            });
        }

        segments.push(decoded);
    }

    // Reconstruct the 16-bit image from byte planes
    match num_segments {
        1 => {
            // 8-bit data stored as 16-bit
            let pixels: Vec<u16> = segments[0].iter().map(|&b| b as u16).collect();
            if pixels.len() != num_pixels {
                return Err(CodecError::DimensionMismatch {
                    expected: num_pixels,
                    actual: pixels.len(),
                });
            }
            Ok((pixels, width, height))
        }
        2 => {
            // 16-bit: segment 0 = high bytes, segment 1 = low bytes
            let high = &segments[0];
            let low = &segments[1];
            let mut pixels = Vec::with_capacity(num_pixels);
            for i in 0..num_pixels {
                pixels.push(((high[i] as u16) << 8) | (low[i] as u16));
            }
            if pixels.len() != num_pixels {
                return Err(CodecError::DimensionMismatch {
                    expected: num_pixels,
                    actual: pixels.len(),
                });
            }
            Ok((pixels, width, height))
        }
        n => Err(CodecError::Unsupported(format!(
            "RLE: unsupported segment count {n} (expected 1 or 2)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode;

    #[test]
    fn decode_too_short() {
        let result = decode(&[0u8; 10], 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn decode_zero_segments() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&0u32.to_le_bytes());
        let result = decode(&data, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn decode_too_many_segments() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(&16u32.to_le_bytes());
        let result = decode(&data, 1, 1);
        assert!(result.is_err());
    }

    #[test]
    fn roundtrip_small() {
        let original = vec![100u16, 200, 300, 400, 500, 600];
        let mut buf = Vec::new();
        encode(&original, 3, 2, &mut buf).unwrap();
        let (decoded, w, h) = decode(&buf, 3, 2).unwrap();
        assert_eq!(w, 3);
        assert_eq!(h, 2);
        assert_eq!(original, decoded);
    }

    #[test]
    fn roundtrip_uniform() {
        let original = vec![0x1234u16; 50 * 50];
        let mut buf = Vec::new();
        encode(&original, 50, 50, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 50, 50).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn roundtrip_gradient() {
        let pixels: Vec<u16> = (0..256).collect();
        let mut buf = Vec::new();
        encode(&pixels, 16, 16, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 16, 16).unwrap();
        assert_eq!(pixels, decoded);
    }

    #[test]
    fn roundtrip_max_values() {
        let original = vec![u16::MAX; 10 * 10];
        let mut buf = Vec::new();
        encode(&original, 10, 10, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 10, 10).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn roundtrip_alternating() {
        let pixels: Vec<u16> = (0..100)
            .map(|i| if i % 2 == 0 { 0x0000 } else { 0xFFFF })
            .collect();
        let mut buf = Vec::new();
        encode(&pixels, 10, 10, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 10, 10).unwrap();
        assert_eq!(pixels, decoded);
    }

    #[test]
    fn roundtrip_large_image() {
        // 512x512 gradient -- realistic CT slice size
        let pixels: Vec<u16> = (0..512 * 512).map(|i| (i % 65536) as u16).collect();
        let mut buf = Vec::new();
        encode(&pixels, 512, 512, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 512, 512).unwrap();
        assert_eq!(pixels, decoded);
    }

    #[test]
    fn roundtrip_odd_dimensions() {
        let pixels: Vec<u16> = (0..7 * 13).map(|i| (i * 137) as u16).collect();
        let mut buf = Vec::new();
        encode(&pixels, 7, 13, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 7, 13).unwrap();
        assert_eq!(pixels, decoded);
    }

    #[test]
    fn roundtrip_single_pixel() {
        let original = vec![0xABCDu16];
        let mut buf = Vec::new();
        encode(&original, 1, 1, &mut buf).unwrap();
        let (decoded, _, _) = decode(&buf, 1, 1).unwrap();
        assert_eq!(original, decoded);
    }
}
