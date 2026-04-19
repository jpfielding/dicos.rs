//! DICOS/DICOM file writer.
//!
//! Writes datasets in the standard DICOM Part-10 file format using
//! Explicit VR Little Endian encoding:
//!
//! 1. 128-byte preamble (all zeros)
//! 2. 4-byte "DICM" magic
//! 3. Group 0002 (File Meta Information) -- Explicit VR Little Endian
//! 4. Remaining dataset elements in tag order

use std::io::{self, Write};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::DicosError;
use crate::tag;
use crate::transfer;
use crate::types::{Dataset, Element, PixelData, Value};
use crate::vr::Vr;

/// Writes a dataset to a writer in DICOM Part-10 file format.
///
/// Returns the number of bytes written.
pub fn write<W: Write>(writer: &mut W, ds: &Dataset) -> Result<u64, DicosError> {
    write_part10(writer, ds)
}

/// Writes a dataset to a writer in DICOM Part-10 file format.
///
/// This function always emits a normalized File Meta Information section
/// (group `0002`) with a recomputed `(0002,0000)` group length and a
/// default Transfer Syntax UID when absent.
///
/// Returns the number of bytes written.
pub fn write_part10<W: Write>(writer: &mut W, ds: &Dataset) -> Result<u64, DicosError> {
    let mut cw = CountingWriter::new(writer);

    // 1. Preamble (128 zeros)
    cw.write_all(&[0u8; 128])?;

    // 2. DICM magic
    cw.write_all(b"DICM")?;

    // 3. Build normalized group-0002 metadata.
    let mut meta_elements: Vec<Element> = ds
        .iter()
        .filter_map(|(tag, elem)| {
            if tag.group == 0x0002 && *tag != tag::FILE_META_INFORMATION_GROUP_LENGTH {
                Some(elem.clone())
            } else {
                None
            }
        })
        .collect();

    if !meta_elements
        .iter()
        .any(|elem| elem.tag == tag::TRANSFER_SYNTAX_UID)
    {
        meta_elements.push(Element::new(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            Value::Str(transfer::EXPLICIT_VR_LITTLE_ENDIAN.to_string()),
        ));
    }
    meta_elements.sort_by_key(|elem| elem.tag);

    // Encode all metadata except group length to compute (0002,0000).
    let mut meta_buf = Vec::new();
    for elem in &meta_elements {
        write_element(&mut meta_buf, elem)?;
    }

    let group_length = u32::try_from(meta_buf.len())
        .map_err(|_| DicosError::Validation("group 0002 is too large".into()))?;

    write_element(
        &mut cw,
        &Element::new(
            tag::FILE_META_INFORMATION_GROUP_LENGTH,
            Vr::UL,
            Value::U32(group_length),
        ),
    )?;
    cw.write_all(&meta_buf)?;

    // 4. Write all non-group-0002 elements in tag order.
    for (tag, elem) in ds.iter() {
        if tag.group != 0x0002 {
            write_element(&mut cw, elem)?;
        }
    }

    Ok(cw.count)
}

/// Writes a single element in Explicit VR Little Endian.
fn write_element<W: Write>(w: &mut W, elem: &Element) -> Result<(), DicosError> {
    // Tag
    w.write_u16::<LittleEndian>(elem.tag.group)?;
    w.write_u16::<LittleEndian>(elem.tag.element)?;

    // VR
    let vr_bytes = elem.vr.as_bytes();
    w.write_all(&vr_bytes)?;

    // Encode value
    let (val_bytes, is_undefined_length) = encode_value(&elem.value, elem.vr)?;

    // Length
    if elem.vr.is_long_vr() {
        // 2 reserved bytes + 4-byte length
        w.write_all(&[0, 0])?;
        let length = if is_undefined_length {
            0xFFFF_FFFFu32
        } else {
            val_bytes.len() as u32
        };
        w.write_u32::<LittleEndian>(length)?;
    } else {
        if is_undefined_length {
            return Err(DicosError::InvalidFile(format!(
                "undefined length not supported for short VR {}",
                elem.vr
            )));
        }
        w.write_u16::<LittleEndian>(val_bytes.len() as u16)?;
    }

    // Value bytes
    w.write_all(&val_bytes)?;

    Ok(())
}

/// Encodes a `Value` into bytes. Returns `(bytes, is_undefined_length)`.
fn encode_value(value: &Value, vr: Vr) -> Result<(Vec<u8>, bool), DicosError> {
    match value {
        Value::Str(s) => {
            let mut b = s.as_bytes().to_vec();
            if b.len() % 2 != 0 {
                // Pad with space for string VRs, null for UI
                if vr == Vr::UI {
                    b.push(0);
                } else {
                    b.push(b' ');
                }
            }
            Ok((b, false))
        }

        Value::Strings(strings) => {
            let joined = strings.join("\\");
            let mut b = joined.into_bytes();
            if b.len() % 2 != 0 {
                if vr == Vr::UI {
                    b.push(0);
                } else {
                    b.push(b' ');
                }
            }
            Ok((b, false))
        }

        Value::U16(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::U16s(values) => {
            let mut b = Vec::with_capacity(values.len() * 2);
            for v in values {
                b.extend_from_slice(&v.to_le_bytes());
            }
            Ok((b, false))
        }

        Value::U32(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::I16(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::I32(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::F32(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::F64(v) => Ok((v.to_le_bytes().to_vec(), false)),

        Value::F32s(values) => {
            let mut b = Vec::with_capacity(values.len() * 4);
            for v in values {
                b.extend_from_slice(&v.to_le_bytes());
            }
            Ok((b, false))
        }

        Value::F64s(values) => {
            let mut b = Vec::with_capacity(values.len() * 8);
            for v in values {
                b.extend_from_slice(&v.to_le_bytes());
            }
            Ok((b, false))
        }

        Value::Bytes(data) => Ok((data.clone(), false)),

        Value::Sequence(datasets) => {
            let b = encode_sequence(datasets)?;
            Ok((b, true)) // Sequences use undefined length
        }

        Value::PixelData(pd) => match pd {
            PixelData::Native { frames } => Ok((encode_native_pixel_data(frames), false)),
            PixelData::Encapsulated { frames, offsets } => {
                let b = encode_encapsulated_pixel_data(frames, offsets)?;
                Ok((b, true)) // Encapsulated uses undefined length
            }
        },
    }
}

/// Encodes a sequence of datasets into bytes.
fn encode_sequence(datasets: &[Dataset]) -> Result<Vec<u8>, DicosError> {
    let mut buf = Vec::new();

    for ds in datasets {
        // Item tag (FFFE,E000)
        buf.write_u16::<LittleEndian>(0xFFFE)?;
        buf.write_u16::<LittleEndian>(0xE000)?;

        // Encode item body
        let mut item_buf = Vec::new();
        for (_tag, elem) in ds.iter() {
            write_element(&mut item_buf, elem)?;
        }

        // Item length
        buf.write_u32::<LittleEndian>(item_buf.len() as u32)?;

        // Item data
        buf.write_all(&item_buf)?;
    }

    // Sequence Delimitation Item (FFFE,E0DD)
    buf.write_u16::<LittleEndian>(0xFFFE)?;
    buf.write_u16::<LittleEndian>(0xE0DD)?;
    buf.write_u32::<LittleEndian>(0)?;

    Ok(buf)
}

/// Encodes native (uncompressed) pixel data.
fn encode_native_pixel_data(frames: &[Vec<u16>]) -> Vec<u8> {
    let total_pixels: usize = frames.iter().map(Vec::len).sum();
    let mut buf = Vec::with_capacity(total_pixels * 2);
    for frame in frames {
        // On little-endian targets the in-memory u16 layout matches DICOM LE,
        // so we can copy the raw bytes directly.
        #[cfg(target_endian = "little")]
        {
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(frame.as_ptr() as *const u8, frame.len() * 2)
            };
            buf.extend_from_slice(bytes);
        }
        #[cfg(target_endian = "big")]
        {
            for pixel in frame {
                buf.extend_from_slice(&pixel.to_le_bytes());
            }
        }
    }
    buf
}

/// Encodes encapsulated (compressed) pixel data with BOT and frame items.
fn encode_encapsulated_pixel_data(
    frames: &[Vec<u8>],
    offsets: &[u32],
) -> Result<Vec<u8>, DicosError> {
    let mut buf = Vec::new();

    // Basic Offset Table item
    buf.write_u16::<LittleEndian>(0xFFFE)?; // Item tag
    buf.write_u16::<LittleEndian>(0xE000)?;
    let bot_len = (offsets.len() * 4) as u32;
    buf.write_u32::<LittleEndian>(bot_len)?;
    for offset in offsets {
        buf.write_u32::<LittleEndian>(*offset)?;
    }

    // Frame items
    for frame in frames {
        buf.write_u16::<LittleEndian>(0xFFFE)?;
        buf.write_u16::<LittleEndian>(0xE000)?;
        buf.write_u32::<LittleEndian>(frame.len() as u32)?;
        buf.write_all(frame)?;
    }

    // Sequence Delimitation Item
    buf.write_u16::<LittleEndian>(0xFFFE)?;
    buf.write_u16::<LittleEndian>(0xE0DD)?;
    buf.write_u32::<LittleEndian>(0)?;

    Ok(buf)
}

/// A writer wrapper that counts bytes written.
struct CountingWriter<W> {
    inner: W,
    count: u64,
}

impl<W: Write> CountingWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner, count: 0 }
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader;
    use crate::tag;
    use crate::transfer;

    /// Helper: build a dataset, write it, read it back, and verify roundtrip.
    fn roundtrip(ds: &Dataset) -> Dataset {
        let mut buf = Vec::new();
        write(&mut buf, ds).expect("write should succeed");
        reader::parse(io::Cursor::new(buf)).expect("parse should succeed")
    }

    #[test]
    fn write_injects_transfer_syntax_when_missing() {
        let mut ds = Dataset::new();
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "DOE^ALICE");

        let rt = roundtrip(&ds);
        assert_eq!(
            rt.get_string(tag::TRANSFER_SYNTAX_UID),
            Some(transfer::EXPLICIT_VR_LITTLE_ENDIAN)
        );
    }

    #[test]
    fn write_recomputes_meta_group_length() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_string(tag::SOP_CLASS_UID, Vr::UI, "1.2.3.4");
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "META^TEST");

        let rt = roundtrip(&ds);
        let elem = rt
            .get(tag::FILE_META_INFORMATION_GROUP_LENGTH)
            .expect("meta group length should be present");
        match &elem.value {
            Value::U32(v) => assert!(*v > 0),
            other => panic!("expected UL group length, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_string_elements() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "SMITH^ALICE");
        ds.put_string(tag::PATIENT_ID, Vr::LO, "12345");
        ds.put_string(tag::MODALITY, Vr::CS, "DX");
        ds.put_string(tag::STUDY_DESCRIPTION, Vr::LO, "Baggage Scan");

        let rt = roundtrip(&ds);

        assert_eq!(rt.get_string(tag::PATIENT_NAME), Some("SMITH^ALICE"));
        assert_eq!(rt.get_string(tag::PATIENT_ID), Some("12345"));
        assert_eq!(rt.modality(), "DX");
        assert_eq!(rt.get_string(tag::STUDY_DESCRIPTION), Some("Baggage Scan"));
    }

    #[test]
    fn roundtrip_numeric_elements() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_u16(tag::ROWS, Vr::US, 512);
        ds.put_u16(tag::COLUMNS, Vr::US, 256);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 16);
        ds.put_u16(tag::BITS_STORED, Vr::US, 12);
        ds.put_u16(tag::HIGH_BIT, Vr::US, 11);
        ds.put_u16(tag::PIXEL_REPRESENTATION, Vr::US, 0);

        let rt = roundtrip(&ds);

        assert_eq!(rt.rows(), 512);
        assert_eq!(rt.columns(), 256);
        assert_eq!(rt.bits_allocated(), 16);
        assert_eq!(rt.get_u16(tag::BITS_STORED), Some(12));
        assert_eq!(rt.pixel_representation(), 0);
    }

    #[test]
    fn roundtrip_native_pixel_data() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_u16(tag::ROWS, Vr::US, 2);
        ds.put_u16(tag::COLUMNS, Vr::US, 2);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 16);

        // Write pixel data as raw bytes (OW)
        let pixels: Vec<u16> = vec![100, 200, 300, 400];
        let mut pixel_bytes = Vec::new();
        for p in &pixels {
            pixel_bytes.extend_from_slice(&p.to_le_bytes());
        }
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::Bytes(pixel_bytes),
        ));

        let rt = roundtrip(&ds);

        assert_eq!(rt.rows(), 2);
        assert_eq!(rt.columns(), 2);
        let pd_elem = rt.get(tag::PIXEL_DATA).expect("should have pixel data");
        match &pd_elem.value {
            Value::Bytes(b) => {
                assert_eq!(b.len(), 8);
                // Verify first pixel
                let p0 = u16::from_le_bytes([b[0], b[1]]);
                assert_eq!(p0, 100);
            }
            other => panic!("expected Bytes, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_encapsulated_pixel_data() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_u16(tag::ROWS, Vr::US, 4);
        ds.put_u16(tag::COLUMNS, Vr::US, 4);

        let compressed_frame = vec![0xFF, 0xD8, 0x00, 0x01, 0x02, 0x03];
        let pd = PixelData::encapsulated(vec![compressed_frame.clone()], vec![0]);
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::PixelData(pd)));

        let rt = roundtrip(&ds);

        let pd_elem = rt.get(tag::PIXEL_DATA).expect("should have pixel data");
        match &pd_elem.value {
            Value::PixelData(pd) => {
                assert!(pd.is_compressed());
                assert_eq!(pd.num_frames(), 1);
                assert_eq!(pd.encapsulated_frame(0), Some(compressed_frame.as_slice()));
                assert_eq!(pd.offsets(), &[0]);
            }
            other => panic!("expected PixelData, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_signed_values() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.insert(Element::new(
            tag::RESCALE_INTERCEPT,
            Vr::DS,
            Value::Str("-1024".into()),
        ));

        let rt = roundtrip(&ds);

        let ri = rt
            .get(tag::RESCALE_INTERCEPT)
            .and_then(|e| e.value.as_f64());
        assert_eq!(ri, Some(-1024.0));
    }

    #[test]
    fn write_produces_valid_preamble_and_magic() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );

        let mut buf = Vec::new();
        write(&mut buf, &ds).expect("write should succeed");

        // First 128 bytes should be zero
        assert!(buf[..128].iter().all(|&b| b == 0));
        // Followed by DICM
        assert_eq!(&buf[128..132], b"DICM");
    }

    #[test]
    fn roundtrip_multiple_frames() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        );

        let pd = PixelData::encapsulated(
            vec![vec![0x01, 0x02, 0x03, 0x04], vec![0x05, 0x06, 0x07, 0x08]],
            vec![0, 12],
        );
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::PixelData(pd)));

        let rt = roundtrip(&ds);
        let pd_elem = rt.get(tag::PIXEL_DATA).expect("should have pixel data");
        match &pd_elem.value {
            Value::PixelData(pd) => {
                assert_eq!(pd.num_frames(), 2);
                assert_eq!(pd.encapsulated_frame(0), Some(&[0x01, 0x02, 0x03, 0x04][..]));
                assert_eq!(pd.encapsulated_frame(1), Some(&[0x05, 0x06, 0x07, 0x08][..]));
            }
            other => panic!("expected PixelData, got {other:?}"),
        }
    }
}
