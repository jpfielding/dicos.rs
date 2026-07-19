use std::io::Write;

use crate::error::CodecError;
use crate::packbits::encode_packbits;

/// DICOM RLE header size in bytes.
/// 4 bytes for segment count + 15 * 4 bytes for offsets = 64 bytes.
const HEADER_SIZE: u32 = 64;

/// Encode a 16-bit grayscale image into DICOM RLE format.
///
/// `pixels` is a row-major pixel buffer of length `width * height`.
/// 16-bit images are split into high-byte and low-byte segments
/// which are independently PackBits-compressed. This byte-plane
/// separation improves compression since adjacent high bytes (or
/// low bytes) are often similar.
///
/// The output format is:
/// - 64-byte header: segment count (u32 LE) + 15 offset slots (u32 LE)
/// - Segment data: PackBits-compressed byte planes, each padded to even length
pub fn encode(
    pixels: &[u16],
    width: u32,
    height: u32,
    w: &mut dyn Write,
) -> Result<(), CodecError> {
    let expected = (width as usize) * (height as usize);
    if pixels.len() != expected {
        return Err(CodecError::DimensionMismatch {
            expected,
            actual: pixels.len(),
        });
    }

    let num_pixels = pixels.len();

    // Split 16-bit pixels into high-byte and low-byte planes
    let mut high_bytes = Vec::with_capacity(num_pixels);
    let mut low_bytes = Vec::with_capacity(num_pixels);

    for &pixel in pixels {
        high_bytes.push((pixel >> 8) as u8);
        low_bytes.push((pixel & 0xFF) as u8);
    }

    // PackBits-compress each byte plane
    let mut segments = vec![encode_packbits(&high_bytes), encode_packbits(&low_bytes)];

    // Pad each segment to even length (DICOM RLE requirement)
    for seg in &mut segments {
        if seg.len() % 2 != 0 {
            seg.push(0x00);
        }
    }

    // Build the 64-byte header
    let num_segments = segments.len() as u32;
    let mut offsets = [0u32; 15];
    let mut current_offset = HEADER_SIZE;
    for (i, seg) in segments.iter().enumerate() {
        offsets[i] = current_offset;
        current_offset += seg.len() as u32;
    }

    // Write header: segment count
    w.write_all(&num_segments.to_le_bytes())?;
    // Write header: 15 offset slots
    for offset in &offsets {
        w.write_all(&offset.to_le_bytes())?;
    }

    // Write segment data
    for seg in &segments {
        w.write_all(seg)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_small_image() {
        let pixels = vec![0x0102u16, 0x0304, 0x0506, 0x0708];
        let mut buf = Vec::new();
        encode(&pixels, 2, 2, &mut buf).unwrap();

        // Verify header
        assert!(buf.len() >= 64);
        let num_segments = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        assert_eq!(num_segments, 2);

        // Verify segment offsets are valid
        let off1 = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let off2 = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        assert_eq!(off1, 64);
        assert!(off2 > off1);
    }

    #[test]
    fn encode_uniform_image() {
        let pixels = vec![0x1234u16; 100 * 100];
        let mut buf = Vec::new();
        encode(&pixels, 100, 100, &mut buf).unwrap();
        // 64-byte header + 2 compressed segments (each ~80 bytes for 10k identical bytes)
        // plus even-padding. Should be well under 1000 bytes for 20000 raw bytes.
        assert!(
            buf.len() < 1000,
            "uniform image should compress well, got {} bytes",
            buf.len()
        );
    }

    #[test]
    fn encode_zero_image() {
        let pixels: Vec<u16> = vec![];
        let mut buf = Vec::new();
        encode(&pixels, 0, 0, &mut buf).unwrap();
        assert!(buf.len() >= 64);
    }

    #[test]
    fn encode_single_pixel() {
        let pixels = vec![0xABCDu16];
        let mut buf = Vec::new();
        encode(&pixels, 1, 1, &mut buf).unwrap();
        let num_segments = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        assert_eq!(num_segments, 2);
    }

    #[test]
    fn encode_dimension_mismatch() {
        let pixels = vec![0u16; 10];
        let mut buf = Vec::new();
        let result = encode(&pixels, 2, 2, &mut buf);
        assert!(result.is_err());
    }
}
