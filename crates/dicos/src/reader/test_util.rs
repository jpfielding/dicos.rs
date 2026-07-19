//! Wire-format builders shared across the crate's unit tests.
//!
//! These helpers assemble DICOS/DICOM byte streams by hand so parser and
//! writer tests can construct precise on-disk layouts (explicit/implicit VR,
//! fixed- and undefined-length sequences, deliberately malformed inputs).
//! They are compiled only under `#[cfg(test)]` and shared as `pub(crate)` so
//! reader, writer, and proptest modules can reuse them.

use crate::tag::Tag;
use crate::transfer;
use crate::types::{Element, Value};

use super::UNDEFINED_LENGTH;

/// Builds a minimal valid DICOS file in memory with explicit VR LE.
pub(crate) fn build_minimal_explicit_vr_le(elements: &[Element]) -> Vec<u8> {
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

/// Builds a minimal valid DICOS file in memory with an arbitrary transfer syntax UID.
///
/// Identical structure to `build_minimal_explicit_vr_le` but lets the caller choose
/// the TS UID so we can test how the reader reacts to, e.g., big-endian files.
pub(crate) fn build_minimal_dicos_with_ts(ts_uid: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    // Preamble (128 zeros)
    buf.extend_from_slice(&[0u8; 128]);
    // DICM magic
    buf.extend_from_slice(b"DICM");

    let meta_start = buf.len();

    // TransferSyntaxUID (0002,0010) UI
    let ts_bytes = ts_uid.as_bytes();
    let ts_padded_len = if ts_bytes.len() % 2 == 0 {
        ts_bytes.len()
    } else {
        ts_bytes.len() + 1
    };
    buf.extend_from_slice(&0x0002u16.to_le_bytes());
    buf.extend_from_slice(&0x0010u16.to_le_bytes());
    buf.extend_from_slice(b"UI");
    buf.extend_from_slice(&(ts_padded_len as u16).to_le_bytes());
    buf.extend_from_slice(ts_bytes);
    if ts_bytes.len() % 2 != 0 {
        buf.push(b' ');
    }

    let meta_len = buf.len() - meta_start;

    // Prepend the group-length element before the TS UID element.
    let mut final_buf = Vec::new();
    final_buf.extend_from_slice(&buf[..meta_start]);

    // (0002,0000) UL 4 <meta_len>
    final_buf.extend_from_slice(&0x0002u16.to_le_bytes());
    final_buf.extend_from_slice(&0x0000u16.to_le_bytes());
    final_buf.extend_from_slice(b"UL");
    final_buf.extend_from_slice(&4u16.to_le_bytes());
    final_buf.extend_from_slice(&(meta_len as u32).to_le_bytes());

    // Re-append the TS UID element
    final_buf.extend_from_slice(&buf[meta_start..]);

    // Append a trivial non-group-0002 element so the reader crosses the
    // meta boundary and evaluates the declared transfer syntax.
    // (0008,0060) Modality CS "CT" — two bytes, explicit VR LE encoding.
    final_buf.extend_from_slice(&0x0008u16.to_le_bytes()); // group
    final_buf.extend_from_slice(&0x0060u16.to_le_bytes()); // element
    final_buf.extend_from_slice(b"CS"); // VR
    final_buf.extend_from_slice(&2u16.to_le_bytes()); // length
    final_buf.extend_from_slice(b"CT"); // value

    final_buf
}

/// Builds a DICOS file with no TransferSyntaxUID in group 0002.
///
/// The meta section contains only (0002,0000) GroupLength with value 0.
/// The dataset element is encoded as Implicit VR LE (4-byte length, no VR
/// field) because that is what the reader will default to per the DICOM
/// standard.
pub(crate) fn build_dicos_without_transfer_syntax() -> Vec<u8> {
    let mut buf = Vec::new();

    // Preamble
    buf.extend_from_slice(&[0u8; 128]);
    // DICM magic
    buf.extend_from_slice(b"DICM");

    // (0002,0000) UL 4 <0> — group length; no other meta elements follow,
    // so the value is 0 (nothing after this element in group 0002).
    buf.extend_from_slice(&0x0002u16.to_le_bytes()); // group
    buf.extend_from_slice(&0x0000u16.to_le_bytes()); // element
    buf.extend_from_slice(b"UL"); // VR (group 0002 is always explicit)
    buf.extend_from_slice(&4u16.to_le_bytes()); // length
    buf.extend_from_slice(&0u32.to_le_bytes()); // value: 0 bytes of meta follow

    // One non-group-0002 element so the reader crosses the meta boundary.
    // Encoded as Implicit VR LE: tag (4 bytes) + length (4 bytes) + value.
    // (0028,0010) Rows = 64
    buf.extend_from_slice(&0x0028u16.to_le_bytes()); // group
    buf.extend_from_slice(&0x0010u16.to_le_bytes()); // element
    buf.extend_from_slice(&2u32.to_le_bytes()); // length (implicit VR: 4-byte len)
    buf.extend_from_slice(&64u16.to_le_bytes()); // value

    buf
}

/// Resolves a path under the workspace `testdata/` directory.
pub(crate) fn testdata_path(name: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../testdata");
    p.push(name);
    p
}

/// Encodes a single element in explicit-VR wire format: tag | VR | len | value.
pub(crate) fn encode_explicit_elem(elem: &Element) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&elem.tag.group.to_le_bytes());
    buf.extend_from_slice(&elem.tag.element.to_le_bytes());
    buf.extend_from_slice(&elem.vr.as_bytes());
    let val = encode_value_bytes(&elem.value);
    if elem.vr.is_long_vr() {
        buf.extend_from_slice(&[0, 0]); // reserved
        buf.extend_from_slice(&(val.len() as u32).to_le_bytes());
    } else {
        buf.extend_from_slice(&(val.len() as u16).to_le_bytes());
    }
    buf.extend_from_slice(&val);
    buf
}

/// Wraps `content` in a fixed-length SQ item: FFFE,E000 | 4-byte len | content.
pub(crate) fn build_sq_item(content: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0xFFFEu16.to_le_bytes());
    buf.extend_from_slice(&0xE000u16.to_le_bytes());
    buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
    buf.extend_from_slice(content);
    buf
}

/// Wraps `items_bytes` in a fixed-length SQ element (long VR encoding).
pub(crate) fn build_fixed_sq(sq_tag: Tag, items_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&sq_tag.group.to_le_bytes());
    buf.extend_from_slice(&sq_tag.element.to_le_bytes());
    buf.extend_from_slice(b"SQ");
    buf.extend_from_slice(&[0u8, 0u8]); // reserved (long VR)
    buf.extend_from_slice(&(items_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(items_bytes);
    buf
}

/// Wraps `inner` in one level of undefined-length SQ containing a single
/// undefined-length item, in explicit-VR LE encoding.
pub(crate) fn wrap_undefined_length_sq(inner: &[u8]) -> Vec<u8> {
    let mut item = Vec::new();
    item.extend_from_slice(&0xFFFEu16.to_le_bytes()); // Item tag
    item.extend_from_slice(&0xE000u16.to_le_bytes());
    item.extend_from_slice(&UNDEFINED_LENGTH.to_le_bytes());
    item.extend_from_slice(inner);
    item.extend_from_slice(&0xFFFEu16.to_le_bytes()); // Item Delimitation
    item.extend_from_slice(&0xE00Du16.to_le_bytes());
    item.extend_from_slice(&0u32.to_le_bytes());

    let tag = crate::tag::REFERENCED_IMAGE_SEQUENCE;
    let mut sq = Vec::new();
    sq.extend_from_slice(&tag.group.to_le_bytes());
    sq.extend_from_slice(&tag.element.to_le_bytes());
    sq.extend_from_slice(b"SQ");
    sq.extend_from_slice(&[0u8, 0u8]); // reserved (long VR)
    sq.extend_from_slice(&UNDEFINED_LENGTH.to_le_bytes());
    sq.extend_from_slice(&item);
    sq.extend_from_slice(&0xFFFEu16.to_le_bytes()); // Sequence Delimitation
    sq.extend_from_slice(&0xE0DDu16.to_le_bytes());
    sq.extend_from_slice(&0u32.to_le_bytes());
    sq
}
