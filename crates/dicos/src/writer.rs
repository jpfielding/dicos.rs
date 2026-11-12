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
        .filter_map(|elem| {
            if elem.tag.group == 0x0002 && elem.tag != tag::FILE_META_INFORMATION_GROUP_LENGTH {
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

    // The body is always written Explicit VR Little Endian. If the dataset
    // advertised Implicit VR Little Endian, rewrite that UID so it matches
    // what we actually emit (the same UID used when it is missing).
    // Compressed transfer syntaxes are left alone: they describe the pixel
    // data, not the element VR encoding.
    for elem in &mut meta_elements {
        if elem.tag != tag::TRANSFER_SYNTAX_UID {
            continue;
        }
        let implicit = elem
            .value
            .as_str()
            .is_some_and(|uid| uid.trim() == transfer::IMPLICIT_VR_LITTLE_ENDIAN);
        if implicit {
            *elem = Element::new(
                tag::TRANSFER_SYNTAX_UID,
                Vr::UI,
                Value::Str(transfer::EXPLICIT_VR_LITTLE_ENDIAN.to_string()),
            );
        }
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
    for elem in ds.iter() {
        if elem.tag.group != 0x0002 {
            let bits = if elem.tag == tag::PIXEL_DATA {
                ds.bits_allocated()
            } else {
                None
            };
            write_element_with_bits(&mut cw, elem, bits)?;
        }
    }

    Ok(cw.count)
}

/// Writes a single element in Explicit VR Little Endian.
fn write_element<W: Write>(w: &mut W, elem: &Element) -> Result<(), DicosError> {
    write_element_with_bits(w, elem, None)
}

/// Writes a single element, using `bits_allocated` when encoding native pixels.
fn write_element_with_bits<W: Write>(
    w: &mut W,
    elem: &Element,
    bits_allocated: Option<u16>,
) -> Result<(), DicosError> {
    // Tag
    w.write_u16::<LittleEndian>(elem.tag.group)?;
    w.write_u16::<LittleEndian>(elem.tag.element)?;

    // VR
    let vr_bytes = elem.vr.as_bytes();
    w.write_all(&vr_bytes)?;

    // Encode value
    let (val_bytes, is_undefined_length) = encode_value(&elem.value, elem.vr, bits_allocated)?;

    // Length
    if elem.vr.is_long_vr() {
        // 2 reserved bytes + 4-byte length
        w.write_all(&[0, 0])?;
        let length = if is_undefined_length {
            0xFFFF_FFFFu32
        } else {
            u32::try_from(val_bytes.len()).map_err(|_| {
                DicosError::Validation(format!(
                    "value too large for element {} ({} bytes, max {})",
                    elem.tag,
                    val_bytes.len(),
                    u32::MAX
                ))
            })?
        };
        w.write_u32::<LittleEndian>(length)?;
    } else {
        if is_undefined_length {
            return Err(DicosError::Validation(format!(
                "undefined length not supported for short VR {}",
                elem.vr
            )));
        }
        let length = u16::try_from(val_bytes.len()).map_err(|_| {
            DicosError::Validation(format!(
                "value too large for short-VR element {} ({} bytes, max 65535)",
                elem.tag,
                val_bytes.len()
            ))
        })?;
        w.write_u16::<LittleEndian>(length)?;
    }

    // Value bytes
    w.write_all(&val_bytes)?;

    Ok(())
}

/// Encodes a `Value` into bytes. Returns `(bytes, is_undefined_length)`.
fn encode_value(
    value: &Value,
    vr: Vr,
    bits_allocated: Option<u16>,
) -> Result<(Vec<u8>, bool), DicosError> {
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
            PixelData::Native { frames } => {
                let b = encode_native_pixel_data(frames, bits_allocated)?;
                Ok((b, false))
            }
            PixelData::Encapsulated { frames, .. } => {
                let b = encode_encapsulated_pixel_data(frames)?;
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
        for elem in ds.iter() {
            write_element(&mut item_buf, elem)?;
        }

        // Item length
        buf.write_u32::<LittleEndian>(u32::try_from(item_buf.len()).map_err(|_| {
            DicosError::Validation(format!(
                "sequence item too large ({} bytes, max {})",
                item_buf.len(),
                u32::MAX
            ))
        })?)?;

        // Item data
        buf.write_all(&item_buf)?;
    }

    // Sequence Delimitation Item (FFFE,E0DD)
    buf.write_u16::<LittleEndian>(0xFFFE)?;
    buf.write_u16::<LittleEndian>(0xE0DD)?;
    buf.write_u32::<LittleEndian>(0)?;

    Ok(buf)
}

/// Encodes native (uncompressed) pixel data using `BitsAllocated`.
///
/// 8-bit samples are written as one byte each (values must fit in 0..=255)
/// and padded to an even length as required by DICOM. 16-bit samples (the
/// default when BitsAllocated is absent) are written little-endian. Other
/// widths are rejected because the in-memory representation is `u16`.
fn encode_native_pixel_data(
    frames: &[Vec<u16>],
    bits_allocated: Option<u16>,
) -> Result<Vec<u8>, DicosError> {
    match bits_allocated.unwrap_or(16) {
        8 => encode_native_8bit(frames),
        16 => Ok(encode_native_16bit(frames)),
        other => Err(DicosError::Validation(format!(
            "unsupported BitsAllocated {other} for native pixel data (supported: 8 and 16)"
        ))),
    }
}

fn encode_native_8bit(frames: &[Vec<u16>]) -> Result<Vec<u8>, DicosError> {
    let total_pixels: usize = frames.iter().map(Vec::len).sum();
    let mut buf = Vec::with_capacity(total_pixels + 1);
    for frame in frames {
        for &pixel in frame {
            let byte = u8::try_from(pixel).map_err(|_| {
                DicosError::Validation(format!(
                    "native sample {pixel} does not fit in 8-bit pixel data"
                ))
            })?;
            buf.push(byte);
        }
    }
    // DICOM Value Length must be even.
    if buf.len() % 2 == 1 {
        buf.push(0);
    }
    Ok(buf)
}

fn encode_native_16bit(frames: &[Vec<u16>]) -> Vec<u8> {
    let total_pixels: usize = frames.iter().map(Vec::len).sum();
    let mut buf = Vec::with_capacity(total_pixels * 2);
    for frame in frames {
        // On little-endian targets the in-memory u16 layout matches DICOM LE,
        // so we can reinterpret the slice and copy the raw bytes directly.
        #[cfg(target_endian = "little")]
        {
            buf.extend_from_slice(bytemuck::cast_slice::<u16, u8>(frame));
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

/// Encodes encapsulated (compressed) pixel data with a BOT rebuilt from the
/// items actually written (one item per assembled frame, including padding).
fn encode_encapsulated_pixel_data(frames: &[Vec<u8>]) -> Result<Vec<u8>, DicosError> {
    let mut items = Vec::new();
    let mut offsets = Vec::with_capacity(frames.len());
    let mut offset: u32 = 0;

    for frame in frames {
        offsets.push(offset);

        // Item lengths must be even; a trailing 0 pad is part of the item.
        let pad = usize::from(frame.len() % 2 == 1);
        let body_len = frame.len() + pad;
        let body_len_u32 = u32::try_from(body_len).map_err(|_| {
            DicosError::Validation(format!(
                "compressed frame too large ({} bytes, max {})",
                body_len,
                u32::MAX
            ))
        })?;
        let item_size = body_len_u32
            .checked_add(8)
            .ok_or_else(|| DicosError::Validation("compressed frame too large".into()))?;

        items.write_u16::<LittleEndian>(0xFFFE)?;
        items.write_u16::<LittleEndian>(0xE000)?;
        items.write_u32::<LittleEndian>(body_len_u32)?;
        items.write_all(frame)?;
        if pad == 1 {
            items.write_all(&[0])?;
        }

        offset = offset.checked_add(item_size).ok_or_else(|| {
            DicosError::Validation("encapsulated pixel data offsets overflow u32".into())
        })?;
    }

    let mut buf = Vec::new();

    // Basic Offset Table item, one entry per emitted frame.
    buf.write_u16::<LittleEndian>(0xFFFE)?; // Item tag
    buf.write_u16::<LittleEndian>(0xE000)?;
    let bot_len = u32::try_from(offsets.len() * 4)
        .map_err(|_| DicosError::Validation("Basic Offset Table too large".into()))?;
    buf.write_u32::<LittleEndian>(bot_len)?;
    for o in &offsets {
        buf.write_u32::<LittleEndian>(*o)?;
    }

    buf.write_all(&items)?;

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

        assert_eq!(rt.rows(), Some(512));
        assert_eq!(rt.columns(), Some(256));
        assert_eq!(rt.bits_allocated(), Some(16));
        assert_eq!(rt.get_u16(tag::BITS_STORED), Some(12));
        assert_eq!(rt.pixel_representation(), Some(0));
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

        assert_eq!(rt.rows(), Some(2));
        assert_eq!(rt.columns(), Some(2));
        // After roundtrip the reader normalizes raw OW bytes to PixelData::Native
        let pd = rt.pixel_data().expect("pixel_data() should return Some");
        let frames = pd.native_frames().expect("should be native pixel data");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], vec![100u16, 200, 300, 400]);
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
                assert_eq!(
                    pd.encapsulated_frame(0),
                    Some(&[0x01, 0x02, 0x03, 0x04][..])
                );
                assert_eq!(
                    pd.encapsulated_frame(1),
                    Some(&[0x05, 0x06, 0x07, 0x08][..])
                );
            }
            other => panic!("expected PixelData, got {other:?}"),
        }
    }

    // --- Review findings 1–3 (and 5, which is needed for odd 8-bit writes) ---

    /// Finding 3: 8-bit native samples must not be widened to 16-bit on write.
    /// The review probe writes raw OB bytes, parses to Native, then writes again.
    #[test]
    fn roundtrip_8bit_native_pixels() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 2);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 8);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OB,
            Value::Bytes(vec![10, 20]),
        ));

        let first = roundtrip(&ds);
        assert_eq!(
            first.pixel_data(),
            Some(&PixelData::Native {
                frames: vec![vec![10, 20]]
            })
        );
        assert_eq!(first.bits_allocated(), Some(8));

        let second = roundtrip(&first);
        assert_eq!(first.pixel_data(), second.pixel_data());
        assert_eq!(second.bits_allocated(), Some(8));
    }

    /// Finding 3: 16-bit Native frames still round-trip unchanged.
    #[test]
    fn roundtrip_16bit_native_pixel_data_enum() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 2);
        ds.put_u16(tag::COLUMNS, Vr::US, 2);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 16);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::PixelData(PixelData::native(vec![vec![100, 200, 300, 400]])),
        ));

        let rt = roundtrip(&ds);
        assert_eq!(
            rt.pixel_data(),
            Some(&PixelData::Native {
                frames: vec![vec![100, 200, 300, 400]]
            })
        );
    }

    /// Finding 5: an odd-sized 8-bit image is padded on write; the pad is
    /// not treated as a voxel on the next parse.
    #[test]
    fn roundtrip_8bit_odd_length_strips_padding() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 8);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OB,
            Value::Bytes(vec![10, 0]),
        ));

        let first = roundtrip(&ds);
        assert_eq!(
            first.pixel_data(),
            Some(&PixelData::Native {
                frames: vec![vec![10]]
            })
        );

        let second = roundtrip(&first);
        assert_eq!(first.pixel_data(), second.pixel_data());
    }

    #[test]
    fn write_8bit_rejects_samples_that_do_not_fit() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, 8);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::PixelData(PixelData::native(vec![vec![256]])),
        ));

        let mut buf = Vec::new();
        let err = write(&mut buf, &ds).expect_err("8-bit overflow should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("256") && msg.contains("8-bit"),
            "unexpected error: {msg}"
        );
    }

    /// Finding 2: writing a dataset tagged Implicit VR Little Endian must
    /// still produce a file the reader can parse. The UID is normalized to
    /// Explicit VR Little Endian, matching the missing-UID path.
    #[test]
    fn implicit_vr_write_normalizes_to_explicit_and_roundtrips() {
        let mut ds = Dataset::new();
        ds.put_string(
            tag::TRANSFER_SYNTAX_UID,
            Vr::UI,
            transfer::IMPLICIT_VR_LITTLE_ENDIAN,
        );
        ds.put_u16(tag::ROWS, Vr::US, 1);

        let rt = roundtrip(&ds);
        assert_eq!(
            rt.get_string(tag::TRANSFER_SYNTAX_UID),
            Some(transfer::EXPLICIT_VR_LITTLE_ENDIAN)
        );
        assert_eq!(rt.rows(), Some(1));
    }

    /// Finding 2: compressed UIDs describe the pixel data and must be kept.
    #[test]
    fn compressed_transfer_syntax_is_preserved() {
        let mut ds = Dataset::new();
        ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, transfer::JPEG_LS_LOSSLESS);
        ds.put_u16(tag::ROWS, Vr::US, 1);

        let rt = roundtrip(&ds);
        assert_eq!(
            rt.get_string(tag::TRANSFER_SYNTAX_UID),
            Some(transfer::JPEG_LS_LOSSLESS)
        );
        assert_eq!(rt.rows(), Some(1));
    }

    /// Finding 1: after fragments have been assembled into frames, a stale
    /// BOT must not be copied on write. The review probe starts from
    /// frames `[[1,2,3,4],[5,6]]` with leftover offsets `[0, 20]`.
    #[test]
    fn multi_fragment_roundtrip_recomputes_bot() {
        let mut ds = Dataset::new();
        ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, transfer::JPEG_LS_LOSSLESS);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OB,
            Value::PixelData(PixelData::encapsulated(
                vec![vec![1, 2, 3, 4], vec![5, 6]],
                vec![0, 20],
            )),
        ));

        let rt = roundtrip(&ds);
        match rt.pixel_data() {
            Some(PixelData::Encapsulated { frames, offsets }) => {
                assert_eq!(frames, &vec![vec![1, 2, 3, 4], vec![5, 6]]);
                // Frame 0 is 4 bytes, so the item is 8 (header) + 4 = 12.
                assert_eq!(offsets, &vec![0, 12]);
            }
            other => panic!("expected encapsulated pixel data, got {other:?}"),
        }

        // A second cycle must be stable.
        let again = roundtrip(&rt);
        assert_eq!(rt.pixel_data(), again.pixel_data());
    }

    /// Finding 1: odd-length fragments are padded, and that pad is included
    /// in the recomputed offsets so frames stay separate.
    #[test]
    fn encapsulated_odd_length_frame_pads_and_keeps_boundaries() {
        let mut ds = Dataset::new();
        ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, transfer::JPEG_LS_LOSSLESS);
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OB,
            Value::PixelData(PixelData::encapsulated(
                vec![vec![1, 2, 3], vec![4, 5]],
                vec![0, 99],
            )),
        ));

        let rt = roundtrip(&ds);
        match rt.pixel_data() {
            Some(PixelData::Encapsulated { frames, offsets }) => {
                // The odd first frame is padded; the even second frame is not.
                assert_eq!(frames, &vec![vec![1, 2, 3, 0], vec![4, 5]]);
                // Padded body is 4 bytes, so the item is 8 + 4 = 12.
                assert_eq!(offsets, &vec![0, 12]);
            }
            other => panic!("expected encapsulated pixel data, got {other:?}"),
        }
    }
}
