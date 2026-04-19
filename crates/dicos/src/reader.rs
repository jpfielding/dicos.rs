//! DICOS/DICOM file reader.
//!
//! Reads the standard DICOM Part-10 file format:
//!
//! 1. 128-byte preamble (ignored)
//! 2. 4-byte "DICM" magic
//! 3. Group 0002 (File Meta Information) -- always Explicit VR Little Endian
//! 4. Remaining dataset elements using the transfer syntax from (0002,0010)
//!
//! Supports Implicit VR Little Endian, Explicit VR Little Endian, and
//! encapsulated pixel data.

use std::io::{self, Read};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::DicosError;
use crate::tag::{self, Tag};
use crate::transfer;
use crate::types::{Dataset, Element, PixelData, Value};
use crate::vr::Vr;

/// The 4-byte DICOM magic number.
const DICM_MAGIC: &[u8; 4] = b"DICM";

/// Sentinel for undefined length.
const UNDEFINED_LENGTH: u32 = 0xFFFF_FFFF;

/// Default per-element allocation limit (1 GB).
const DEFAULT_MAX_ELEMENT_LENGTH: usize = 1024 * 1024 * 1024;

/// Allocate a buffer of `len` bytes, rejecting sizes above `limit`.
fn checked_alloc(len: u32, limit: usize) -> Result<Vec<u8>, DicosError> {
    let n = len as usize;
    if n > limit {
        return Err(DicosError::InvalidFile(format!(
            "element length {n} exceeds limit ({limit})"
        )));
    }
    Ok(vec![0u8; n])
}

/// Skip `len` bytes without allocating.
fn skip_bytes<R: Read>(reader: &mut R, len: u32) -> Result<(), DicosError> {
    io::copy(&mut reader.take(len as u64), &mut io::sink())?;
    Ok(())
}

/// Parses a DICOS/DICOM file from a reader.
///
/// Uses a default per-element allocation limit of 1 GB. For custom limits,
/// use [`parse_with_limit`].
pub fn parse<R: Read>(reader: R) -> Result<Dataset, DicosError> {
    parse_with_limit(reader, DEFAULT_MAX_ELEMENT_LENGTH)
}

/// Parses a DICOS/DICOM file with a custom per-element allocation limit.
///
/// Any single element whose on-disk length exceeds `max_element_bytes` is
/// rejected. This guards against malicious files without restricting
/// legitimate large volumes.
pub fn parse_with_limit<R: Read>(
    reader: R,
    max_element_bytes: usize,
) -> Result<Dataset, DicosError> {
    let mut r = DicosReader::new(reader, max_element_bytes);
    r.read_dataset()
}

/// Low-level DICOS/DICOM reader that tracks transfer syntax state.
struct DicosReader<R> {
    inner: R,
    explicit_vr: bool,
    transfer_syntax_uid: Option<String>,
    /// Tracks whether we are still inside group 0002 (File Meta Information).
    in_meta: bool,
    /// Per-element allocation ceiling in bytes.
    max_element_bytes: usize,
}

impl<R: Read> DicosReader<R> {
    fn new(inner: R, max_element_bytes: usize) -> Self {
        Self {
            inner,
            explicit_vr: true, // Group 0002 is always explicit
            transfer_syntax_uid: None,
            in_meta: true,
            max_element_bytes,
        }
    }

    fn read_dataset(&mut self) -> Result<Dataset, DicosError> {
        let mut ds = Dataset::new();

        // 1. Read 128-byte preamble
        let mut preamble = [0u8; 128];
        self.inner
            .read_exact(&mut preamble)
            .map_err(|e| DicosError::InvalidFile(format!("failed to read preamble: {e}")))?;

        // 2. Read "DICM" magic
        let mut magic = [0u8; 4];
        self.inner
            .read_exact(&mut magic)
            .map_err(|e| DicosError::InvalidFile(format!("failed to read DICM magic: {e}")))?;
        if &magic != DICM_MAGIC {
            return Err(DicosError::InvalidFile("missing DICM magic number".into()));
        }

        // 3. Group 0002 is always Explicit VR Little Endian
        self.explicit_vr = true;
        self.in_meta = true;

        // 4. Read elements
        loop {
            let tag = match self.read_tag() {
                Ok(t) => t,
                Err(DicosError::Io(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };

            // Transition out of group 0002: apply dataset transfer syntax
            if tag.group != 0x0002 && self.in_meta {
                self.in_meta = false;
                if self.transfer_syntax_uid.is_none() {
                    // No TransferSyntaxUID was found; default to Implicit VR LE
                    self.transfer_syntax_uid =
                        Some(transfer::IMPLICIT_VR_LITTLE_ENDIAN.to_string());
                }
                self.update_transfer_syntax();
            }

            let elem = self.read_element_with_tag(tag)?;

            // Capture TransferSyntaxUID when we see it (still in group 0002)
            if tag == tag::TRANSFER_SYNTAX_UID {
                if let Value::Str(ref s) = elem.value {
                    self.transfer_syntax_uid = Some(s.trim().to_string());
                    // Do NOT switch VR mode yet -- still reading group 0002
                }
            }

            ds.insert(elem);
        }

        Ok(ds)
    }

    /// Reads a 4-byte DICOM tag (group, element).
    fn read_tag(&mut self) -> Result<Tag, DicosError> {
        let group = self.inner.read_u16::<LittleEndian>()?;
        let element = self.inner.read_u16::<LittleEndian>()?;
        Ok(Tag::new(group, element))
    }

    /// Reads an element after the tag has already been consumed.
    fn read_element_with_tag(&mut self, tag: Tag) -> Result<Element, DicosError> {
        // Sequence delimiters always have implicit structure: 4-byte length, no VR
        if tag.group == 0xFFFE {
            let len = self.inner.read_u32::<LittleEndian>()?;
            let value = if len > 0 && len != UNDEFINED_LENGTH {
                let mut buf = checked_alloc(len, self.max_element_bytes)?;
                self.inner.read_exact(&mut buf)?;
                Value::Bytes(buf)
            } else {
                Value::Bytes(Vec::new())
            };
            return Ok(Element::new(tag, Vr::UN, value));
        }

        let (vr, vl) = if self.explicit_vr {
            self.read_explicit_vr_header()?
        } else {
            self.read_implicit_vr_header(tag)?
        };

        // After reading group-0002 elements, when we encounter first non-0002 tag
        // the transfer syntax should have been set. But the header was already read
        // with the correct VR mode because we call update_transfer_syntax before
        // read_element_with_tag for the first non-0002 tag.

        let value = self.read_value(tag, vr, vl)?;

        Ok(Element::new(tag, vr, value))
    }

    /// Reads VR + length in Explicit VR mode.
    fn read_explicit_vr_header(&mut self) -> Result<(Vr, u32), DicosError> {
        let mut vr_buf = [0u8; 2];
        self.inner.read_exact(&mut vr_buf)?;
        let vr = Vr::from_bytes(&vr_buf).unwrap_or(Vr::UN);

        let vl = if vr.is_long_vr() {
            // 2 reserved bytes + 4-byte length
            let mut reserved = [0u8; 2];
            self.inner.read_exact(&mut reserved)?;
            self.inner.read_u32::<LittleEndian>()?
        } else {
            // 2-byte length
            u32::from(self.inner.read_u16::<LittleEndian>()?)
        };

        Ok((vr, vl))
    }

    /// Reads length in Implicit VR mode and infers VR from tag.
    fn read_implicit_vr_header(&mut self, tag: Tag) -> Result<(Vr, u32), DicosError> {
        let vl = self.inner.read_u32::<LittleEndian>()?;
        let vr = implicit_vr_for_tag(tag);
        Ok((vr, vl))
    }

    /// Reads the value bytes and parses them according to VR.
    fn read_value(&mut self, tag: Tag, vr: Vr, vl: u32) -> Result<Value, DicosError> {
        if vl == UNDEFINED_LENGTH {
            return self.read_undefined_length_value(tag, vr);
        }

        let mut data = checked_alloc(vl, self.max_element_bytes)?;
        self.inner.read_exact(&mut data)?;

        parse_value(vr, &data, self.explicit_vr, self.max_element_bytes)
    }

    /// Handles elements with undefined length: encapsulated pixel data or sequences.
    fn read_undefined_length_value(&mut self, tag: Tag, vr: Vr) -> Result<Value, DicosError> {
        if tag == tag::PIXEL_DATA {
            let pd = self.read_encapsulated_pixel_data()?;
            return Ok(Value::PixelData(pd));
        }

        if vr == Vr::SQ {
            let items = self.read_sequence_items()?;
            return Ok(Value::Sequence(items));
        }

        // Unknown undefined-length element: skip until sequence delimitation
        self.skip_undefined_length()?;
        Ok(Value::Bytes(Vec::new()))
    }

    /// Reads sequence items until the Sequence Delimitation Item tag.
    fn read_sequence_items(&mut self) -> Result<Vec<Dataset>, DicosError> {
        let mut items = Vec::new();

        loop {
            let item_tag = self.read_tag()?;
            let item_len = self.inner.read_u32::<LittleEndian>()?;

            if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                break;
            }

            if item_tag != tag::ITEM {
                return Err(DicosError::InvalidFile(format!(
                    "expected Item tag, got {item_tag}"
                )));
            }

            let item_ds = if item_len == UNDEFINED_LENGTH {
                self.read_item_undefined_length()?
            } else {
                self.read_item_fixed_length(item_len)?
            };

            items.push(item_ds);
        }

        Ok(items)
    }

    /// Reads a sequence item with undefined length (delimited by Item Delimitation Item).
    fn read_item_undefined_length(&mut self) -> Result<Dataset, DicosError> {
        let mut ds = Dataset::new();

        loop {
            let elem_tag = self.read_tag()?;

            if elem_tag == tag::ITEM_DELIMITATION_ITEM {
                // Read and discard the 4-byte zero length
                let _len = self.inner.read_u32::<LittleEndian>()?;
                break;
            }

            let elem = self.read_element_with_tag(elem_tag)?;
            ds.insert(elem);
        }

        Ok(ds)
    }

    /// Reads a sequence item with a known fixed length.
    fn read_item_fixed_length(&mut self, len: u32) -> Result<Dataset, DicosError> {
        let mut buf = checked_alloc(len, self.max_element_bytes)?;
        self.inner.read_exact(&mut buf)?;

        // Parse the item bytes as a mini-dataset
        let mut sub_reader = DicosReader {
            inner: io::Cursor::new(buf),
            explicit_vr: self.explicit_vr,
            transfer_syntax_uid: self.transfer_syntax_uid.clone(),
            in_meta: false,
            max_element_bytes: self.max_element_bytes,
        };

        let mut ds = Dataset::new();
        loop {
            let tag = match sub_reader.read_tag() {
                Ok(t) => t,
                Err(DicosError::Io(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };
            let elem = sub_reader.read_element_with_tag(tag)?;
            ds.insert(elem);
        }

        Ok(ds)
    }

    /// Reads encapsulated pixel data (Basic Offset Table + compressed frames).
    fn read_encapsulated_pixel_data(&mut self) -> Result<PixelData, DicosError> {
        let mut offsets = Vec::new();
        let mut frames = Vec::new();

        // Read Basic Offset Table item
        let bot_tag = self.read_tag()?;
        if bot_tag != tag::ITEM {
            return Err(DicosError::InvalidFile(format!(
                "expected BOT item tag, got {bot_tag}"
            )));
        }

        let bot_len = self.inner.read_u32::<LittleEndian>()?;
        if bot_len > 0 {
            let num_offsets = bot_len / 4;
            offsets.reserve(num_offsets as usize);
            for _ in 0..num_offsets {
                offsets.push(self.inner.read_u32::<LittleEndian>()?);
            }
        }

        // Read frames until Sequence Delimitation Item
        loop {
            let item_tag = self.read_tag()?;

            if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                // Read and discard length (should be 0)
                let _len = self.inner.read_u32::<LittleEndian>()?;
                break;
            }

            if item_tag != tag::ITEM {
                return Err(DicosError::InvalidFile(format!(
                    "expected item tag in encapsulated pixel data, got {item_tag}"
                )));
            }

            let item_len = self.inner.read_u32::<LittleEndian>()?;
            let mut frame_data = checked_alloc(item_len, self.max_element_bytes)?;
            self.inner.read_exact(&mut frame_data)?;

            frames.push(frame_data);
        }

        Ok(PixelData::encapsulated(frames, offsets))
    }

    /// Skips an undefined-length element by reading until Sequence Delimitation Item.
    fn skip_undefined_length(&mut self) -> Result<(), DicosError> {
        loop {
            let item_tag = self.read_tag()?;

            if item_tag.group == 0xFFFE {
                let len = self.inner.read_u32::<LittleEndian>()?;

                if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                    return Ok(());
                }

                if item_tag == tag::ITEM_DELIMITATION_ITEM {
                    continue;
                }

                // Item start
                if len != UNDEFINED_LENGTH && len > 0 {
                    skip_bytes(&mut self.inner, len)?;
                } else if len == UNDEFINED_LENGTH {
                    // Nested undefined length
                    self.skip_undefined_length()?;
                }
                continue;
            }

            // Regular element within the sequence -- skip it
            if self.explicit_vr {
                let mut vr_buf = [0u8; 2];
                self.inner.read_exact(&mut vr_buf)?;
                let vr = Vr::from_bytes(&vr_buf).unwrap_or(Vr::UN);

                let vl = if vr.is_long_vr() {
                    let mut reserved = [0u8; 2];
                    self.inner.read_exact(&mut reserved)?;
                    self.inner.read_u32::<LittleEndian>()?
                } else {
                    u32::from(self.inner.read_u16::<LittleEndian>()?)
                };

                if vl != UNDEFINED_LENGTH && vl > 0 {
                    skip_bytes(&mut self.inner, vl)?;
                } else if vl == UNDEFINED_LENGTH {
                    self.skip_undefined_length()?;
                }
            } else {
                let vl = self.inner.read_u32::<LittleEndian>()?;
                if vl != UNDEFINED_LENGTH && vl > 0 {
                    skip_bytes(&mut self.inner, vl)?;
                } else if vl == UNDEFINED_LENGTH {
                    self.skip_undefined_length()?;
                }
            }
        }
    }

    /// Updates reader state based on the stored transfer syntax UID.
    fn update_transfer_syntax(&mut self) {
        if let Some(ref uid) = self.transfer_syntax_uid {
            self.explicit_vr = uid != transfer::IMPLICIT_VR_LITTLE_ENDIAN;
        }
    }
}

/// Infers a VR for a tag when reading Implicit VR transfer syntax.
fn implicit_vr_for_tag(tag: Tag) -> Vr {
    match tag {
        // File Meta Info -- always UL for group length
        t if t.group == 0x0002 => Vr::UL,

        // Pixel Data
        t if t == tag::PIXEL_DATA => Vr::OW,

        // Image Pixel numeric attributes
        t if t == tag::ROWS
            || t == tag::COLUMNS
            || t == tag::BITS_ALLOCATED
            || t == tag::BITS_STORED
            || t == tag::HIGH_BIT
            || t == tag::PIXEL_REPRESENTATION
            || t == tag::SAMPLES_PER_PIXEL =>
        {
            Vr::US
        }

        // Number of Frames is IS
        t if t == tag::NUMBER_OF_FRAMES => Vr::IS,

        // Spacing, windowing, rescale -- DS
        t if t == tag::PIXEL_SPACING
            || t == tag::WINDOW_CENTER
            || t == tag::WINDOW_WIDTH
            || t == tag::RESCALE_INTERCEPT
            || t == tag::RESCALE_SLOPE =>
        {
            Vr::DS
        }

        // RescaleType is CS
        t if t == tag::RESCALE_TYPE => Vr::CS,

        // Photometric Interpretation
        t if t == tag::PHOTOMETRIC_INTERPRETATION => Vr::CS,

        // UIDs
        t if t == tag::SOP_CLASS_UID
            || t == tag::SOP_INSTANCE_UID
            || t == tag::STUDY_INSTANCE_UID
            || t == tag::SERIES_INSTANCE_UID =>
        {
            Vr::UI
        }

        // Modality, ImageType
        t if t == tag::MODALITY || t == tag::IMAGE_TYPE => Vr::CS,

        _ => Vr::UN,
    }
}

/// Parses raw bytes into a typed `Value` based on VR.
fn parse_value(vr: Vr, data: &[u8], explicit_vr: bool, max_element_bytes: usize) -> Result<Value, DicosError> {
    match vr {
        // String types
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
        | Vr::UT => {
            let s = String::from_utf8_lossy(data);
            let trimmed = s.trim_end_matches(['\0', ' ']);
            if trimmed.contains('\\') {
                let values = trimmed.split('\\').map(ToOwned::to_owned).collect();
                Ok(Value::Strings(values))
            } else {
                Ok(Value::Str(trimmed.to_string()))
            }
        }

        // Unsigned Short
        Vr::US => {
            if data.len() == 2 {
                Ok(Value::U16(u16::from_le_bytes([data[0], data[1]])))
            } else if data.len() >= 4 && data.len() % 2 == 0 {
                let values: Vec<u16> = data
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                Ok(Value::U16s(values))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Unsigned Long
        Vr::UL => {
            if data.len() == 4 {
                Ok(Value::U32(u32::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                ])))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Signed Short
        Vr::SS => {
            if data.len() == 2 {
                Ok(Value::I16(i16::from_le_bytes([data[0], data[1]])))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Signed Long
        Vr::SL => {
            if data.len() == 4 {
                Ok(Value::I32(i32::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                ])))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Float
        Vr::FL => {
            if data.len() == 4 {
                Ok(Value::F32(f32::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                ])))
            } else if data.len() >= 8 && data.len() % 4 == 0 {
                let values: Vec<f32> = data
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                Ok(Value::F32s(values))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Double
        Vr::FD => {
            if data.len() == 8 {
                Ok(Value::F64(f64::from_le_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ])))
            } else if data.len() >= 16 && data.len() % 8 == 0 {
                let values: Vec<f64> = data
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                    .collect();
                Ok(Value::F64s(values))
            } else {
                Ok(Value::Bytes(data.to_vec()))
            }
        }

        // Attribute Tag
        Vr::AT => Ok(Value::Bytes(data.to_vec())),

        // Sequence -- fixed-length sequences would be read here
        Vr::SQ => {
            // For fixed-length SQ, parse items from data
            if data.is_empty() {
                return Ok(Value::Sequence(Vec::new()));
            }
            let items = parse_sequence_items(data, explicit_vr, max_element_bytes)?;
            Ok(Value::Sequence(items))
        }

        // Binary / Unknown
        Vr::OB | Vr::OD | Vr::OF | Vr::OL | Vr::OW | Vr::UN => Ok(Value::Bytes(data.to_vec())),
    }
}

/// Parses sequence items from a byte buffer (fixed-length SQ).
///
/// Reuses `DicosReader` for element parsing to avoid duplicating VR/header logic.
fn parse_sequence_items(data: &[u8], explicit_vr: bool, max_element_bytes: usize) -> Result<Vec<Dataset>, DicosError> {
    let mut cursor = io::Cursor::new(data);
    let mut items = Vec::new();

    loop {
        let group = match cursor.read_u16::<LittleEndian>() {
            Ok(g) => g,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(DicosError::Io(e)),
        };
        let element = cursor.read_u16::<LittleEndian>()?;
        let item_tag = Tag::new(group, element);

        if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
            let _len = cursor.read_u32::<LittleEndian>()?;
            break;
        }

        if item_tag != tag::ITEM {
            return Err(DicosError::InvalidFile(format!(
                "expected Item tag in fixed-length SQ, got {item_tag}"
            )));
        }

        let item_len = cursor.read_u32::<LittleEndian>()?;

        if item_len == UNDEFINED_LENGTH {
            return Err(DicosError::InvalidFile(
                "undefined-length item inside fixed-length SQ".into(),
            ));
        }

        let pos = cursor.position() as usize;
        let end = pos + item_len as usize;
        if end > data.len() {
            return Err(DicosError::InvalidFile(format!(
                "SQ item length {item_len} exceeds buffer ({})",
                data.len() - pos
            )));
        }

        let item_bytes = &data[pos..end];
        cursor.set_position(end as u64);

        let mut sub_reader = DicosReader {
            inner: io::Cursor::new(item_bytes),
            explicit_vr,
            transfer_syntax_uid: None,
            in_meta: false,
            max_element_bytes,
        };

        let mut ds = Dataset::new();
        loop {
            let tag = match sub_reader.read_tag() {
                Ok(t) => t,
                Err(DicosError::Io(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };

            if tag.group == 0xFFFE {
                let _ = sub_reader.inner.read_u32::<LittleEndian>();
                break;
            }

            let elem = sub_reader.read_element_with_tag(tag)?;
            ds.insert(elem);
        }

        items.push(ds);
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag;
    use crate::transfer;

    /// Builds a minimal valid DICOS file in memory with explicit VR LE.
    fn build_minimal_explicit_vr_le(elements: &[Element]) -> Vec<u8> {
        let mut buf = Vec::new();

        // Preamble (128 zeros)
        buf.extend_from_slice(&[0u8; 128]);
        // DICM magic
        buf.extend_from_slice(b"DICM");

        // File Meta Information Group Length (0002,0000) UL
        // We will compute the group 0002 length after writing elements
        let meta_start = buf.len();

        // TransferSyntaxUID (0002,0010) UI
        let ts = transfer::EXPLICIT_VR_LITTLE_ENDIAN;
        let ts_bytes = ts.as_bytes();
        let ts_padded_len = if ts_bytes.len() % 2 == 0 {
            ts_bytes.len()
        } else {
            ts_bytes.len() + 1
        };
        // Write (0002,0010) UI <len> <value>
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        buf.extend_from_slice(&0x0010u16.to_le_bytes());
        buf.extend_from_slice(b"UI");
        buf.extend_from_slice(&(ts_padded_len as u16).to_le_bytes());
        buf.extend_from_slice(ts_bytes);
        if ts_bytes.len() % 2 != 0 {
            buf.push(b' ');
        }

        let meta_len = buf.len() - meta_start;

        // Now prepend the group length element
        let mut final_buf = Vec::new();
        final_buf.extend_from_slice(&buf[..meta_start]);

        // (0002,0000) UL 4 <meta_len>
        final_buf.extend_from_slice(&0x0002u16.to_le_bytes());
        final_buf.extend_from_slice(&0x0000u16.to_le_bytes());
        final_buf.extend_from_slice(b"UL");
        final_buf.extend_from_slice(&4u16.to_le_bytes());
        final_buf.extend_from_slice(&(meta_len as u32).to_le_bytes());

        // Re-append the meta elements
        final_buf.extend_from_slice(&buf[meta_start..]);

        // Write dataset elements (non-group-0002)
        for elem in elements {
            write_element_explicit(&mut final_buf, elem);
        }

        final_buf
    }

    fn write_element_explicit(buf: &mut Vec<u8>, elem: &Element) {
        // Tag
        buf.extend_from_slice(&elem.tag.group.to_le_bytes());
        buf.extend_from_slice(&elem.tag.element.to_le_bytes());
        // VR
        buf.extend_from_slice(&elem.vr.as_bytes());

        let val_bytes = encode_value_bytes(&elem.value);

        if elem.vr.is_long_vr() {
            // Reserved + 4-byte length
            buf.extend_from_slice(&[0, 0]);
            buf.extend_from_slice(&(val_bytes.len() as u32).to_le_bytes());
        } else {
            // 2-byte length
            buf.extend_from_slice(&(val_bytes.len() as u16).to_le_bytes());
        }

        buf.extend_from_slice(&val_bytes);
    }

    fn encode_value_bytes(value: &Value) -> Vec<u8> {
        match value {
            Value::Str(s) => {
                let mut b = s.as_bytes().to_vec();
                if b.len() % 2 != 0 {
                    b.push(b' ');
                }
                b
            }
            Value::U16(v) => v.to_le_bytes().to_vec(),
            Value::U32(v) => v.to_le_bytes().to_vec(),
            Value::I16(v) => v.to_le_bytes().to_vec(),
            Value::I32(v) => v.to_le_bytes().to_vec(),
            Value::F32(v) => v.to_le_bytes().to_vec(),
            Value::F64(v) => v.to_le_bytes().to_vec(),
            Value::Bytes(b) => b.clone(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn parse_minimal_explicit_vr_le() {
        let elements = vec![
            Element::new(tag::PATIENT_NAME, Vr::PN, Value::Str("DOE^JOHN".into())),
            Element::new(tag::ROWS, Vr::US, Value::U16(512)),
            Element::new(tag::COLUMNS, Vr::US, Value::U16(256)),
            Element::new(tag::BITS_ALLOCATED, Vr::US, Value::U16(16)),
        ];

        let file_data = build_minimal_explicit_vr_le(&elements);
        let ds = parse(io::Cursor::new(file_data)).expect("parse should succeed");

        assert_eq!(ds.get_string(tag::PATIENT_NAME), Some("DOE^JOHN"));
        assert_eq!(ds.rows(), 512);
        assert_eq!(ds.columns(), 256);
        assert_eq!(ds.bits_allocated(), 16);
        assert_eq!(
            ds.transfer_syntax().uid(),
            transfer::EXPLICIT_VR_LITTLE_ENDIAN
        );
    }

    #[test]
    fn parse_detects_missing_magic() {
        let mut buf = vec![0u8; 128]; // preamble
        buf.extend_from_slice(b"XXXX"); // wrong magic
        let result = parse(io::Cursor::new(buf));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("DICM"));
    }

    #[test]
    fn parse_too_short() {
        let buf = vec![0u8; 50]; // too short for preamble
        let result = parse(io::Cursor::new(buf));
        assert!(result.is_err());
    }

    #[test]
    fn parse_numeric_types() {
        let elements = vec![
            Element::new(tag::ROWS, Vr::US, Value::U16(1024)),
            Element::new(tag::RESCALE_INTERCEPT, Vr::DS, Value::Str("-1024".into())),
            Element::new(tag::RESCALE_SLOPE, Vr::DS, Value::Str("1.0".into())),
        ];

        let file_data = build_minimal_explicit_vr_le(&elements);
        let ds = parse(io::Cursor::new(file_data)).expect("parse should succeed");

        assert_eq!(ds.rows(), 1024);
        let ri = ds
            .get(tag::RESCALE_INTERCEPT)
            .and_then(|e| e.value.as_f64());
        assert_eq!(ri, Some(-1024.0));
    }

    #[test]
    fn parse_multivalue_string_vr() {
        let elements = vec![Element::new(
            tag::IMAGE_TYPE,
            Vr::CS,
            Value::Str("ORIGINAL\\PRIMARY".into()),
        )];

        let file_data = build_minimal_explicit_vr_le(&elements);
        let ds = parse(io::Cursor::new(file_data)).expect("parse should succeed");

        let elem = ds.get(tag::IMAGE_TYPE).expect("image type should exist");
        match &elem.value {
            Value::Strings(values) => {
                assert_eq!(values, &vec!["ORIGINAL".to_string(), "PRIMARY".to_string()])
            }
            other => panic!("expected Value::Strings, got {other:?}"),
        }
        assert_eq!(ds.get_string(tag::IMAGE_TYPE), Some("ORIGINAL"));
    }

    // -- Integration tests against real DICOS files --

    fn testdata_path(name: &str) -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../testdata");
        p.push(name);
        p
    }

    #[test]
    fn parse_real_bag_ct_sample() {
        let path = testdata_path("bag_ct.dcs");
        if !path.exists() {
            eprintln!("skipping: {path:?} not found");
            return;
        }
        let file = std::fs::File::open(&path).expect("open test file");
        let reader = std::io::BufReader::new(file);
        let ds = parse(reader).expect("parse bag CT sample");

        assert!(ds.rows() > 0, "rows should be non-zero");
        assert!(ds.columns() > 0, "columns should be non-zero");
        assert!(ds.bits_allocated() > 0);
        assert!(ds.get(tag::PIXEL_DATA).is_some(), "should have PixelData");
        assert_eq!(ds.modality(), "CT");
        assert_eq!(
            ds.get_string(tag::SOP_CLASS_UID),
            Some("1.2.840.10008.5.1.4.1.1.2")
        );
        assert!(ds.number_of_frames() >= 1);

        let ts = ds.transfer_syntax();
        assert!(!ts.uid().is_empty());
    }

    #[test]
    fn parse_with_native_pixel_data() {
        // Build a small 2x2 native pixel data
        let pixels: Vec<u16> = vec![100, 200, 300, 400];
        let mut pixel_bytes = Vec::new();
        for p in &pixels {
            pixel_bytes.extend_from_slice(&p.to_le_bytes());
        }

        let elements = vec![
            Element::new(tag::ROWS, Vr::US, Value::U16(2)),
            Element::new(tag::COLUMNS, Vr::US, Value::U16(2)),
            Element::new(tag::BITS_ALLOCATED, Vr::US, Value::U16(16)),
            Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(pixel_bytes)),
        ];

        let file_data = build_minimal_explicit_vr_le(&elements);
        let ds = parse(io::Cursor::new(file_data)).expect("parse should succeed");

        assert_eq!(ds.rows(), 2);
        assert_eq!(ds.columns(), 2);

        // Pixel data comes back as raw bytes (OW)
        let pd_elem = ds.get(tag::PIXEL_DATA).expect("should have pixel data");
        match &pd_elem.value {
            Value::Bytes(b) => assert_eq!(b.len(), 8), // 4 pixels x 2 bytes
            other => panic!("expected Bytes, got {other:?}"),
        }
    }
}
