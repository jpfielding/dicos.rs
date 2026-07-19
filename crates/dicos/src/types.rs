//! Core DICOS data types: Dataset, Element, Value, and PixelData.
//!
//! These types represent the in-memory structure of a DICOS file after parsing,
//! and serve as the input for writing DICOS files.

use std::collections::BTreeMap;
use std::fmt;

use crate::tag::{self, Tag};
use crate::transfer::{self, TransferSyntax};
use crate::vr::Vr;

/// A typed value stored in a DICOM/DICOS data element.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Single string value.
    Str(String),
    /// Multi-valued string (backslash-separated in DICOM).
    Strings(Vec<String>),
    /// Unsigned 16-bit integer.
    U16(u16),
    /// Multiple unsigned 16-bit integers.
    U16s(Vec<u16>),
    /// Unsigned 32-bit integer.
    U32(u32),
    /// Signed 16-bit integer.
    I16(i16),
    /// Signed 32-bit integer.
    I32(i32),
    /// 32-bit floating point.
    F32(f32),
    /// 64-bit floating point.
    F64(f64),
    /// Multiple 32-bit floating point values.
    F32s(Vec<f32>),
    /// Multiple 64-bit floating point values.
    F64s(Vec<f64>),
    /// Raw byte data (OB, OW, UN).
    Bytes(Vec<u8>),
    /// Sequence of nested datasets.
    Sequence(Vec<Dataset>),
    /// Pixel data with frame structure.
    PixelData(PixelData),
}

impl Value {
    /// Returns the value as a string if it is `Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Strings(values) => values.first().map(String::as_str),
            _ => None,
        }
    }

    /// Returns all string values when the value is `Strings`.
    pub fn as_strings(&self) -> Option<&[String]> {
        match self {
            Value::Strings(values) => Some(values),
            _ => None,
        }
    }

    /// Extracts the value as a `u16` from `U16` or numeric string.
    pub fn as_u16(&self) -> Option<u16> {
        match self {
            Value::U16(v) => Some(*v),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Extracts the value as a `u32`.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(v) => Some(*v),
            Value::U16(v) => Some(u32::from(*v)),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Extracts the value as an `i32` from any integer variant or string.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::I32(v) => Some(*v),
            Value::I16(v) => Some(i32::from(*v)),
            Value::U16(v) => Some(i32::from(*v)),
            Value::U32(v) => i32::try_from(*v).ok(),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Extracts the value as an `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            Value::F32(v) => Some(f64::from(*v)),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Returns the slice of u16 values if this is `U16s`.
    pub fn as_u16s(&self) -> Option<&[u16]> {
        match self {
            Value::U16s(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the slice of f32 values if this is `F32s`.
    pub fn as_f32s(&self) -> Option<&[f32]> {
        match self {
            Value::F32s(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the slice of f64 values if this is `F64s`.
    pub fn as_f64s(&self) -> Option<&[f64]> {
        match self {
            Value::F64s(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the raw bytes if this is `Bytes`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }
}

/// A single DICOM/DICOS data element.
///
/// This is a plain record with public fields to support struct-literal
/// construction. There is no enforced invariant between `vr` and `value`:
/// keeping the Value Representation coherent with the stored [`Value`] (for
/// example, `Vr::US` paired with `Value::U16`) is the caller's responsibility.
/// The writer encodes `value` according to its own variant, not `vr`, so a
/// mismatch produces bytes that may not match the declared VR.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The tag identifying this element.
    pub tag: Tag,
    /// The Value Representation.
    pub vr: Vr,
    /// The typed value.
    pub value: Value,
}

impl Element {
    /// Creates a new element.
    pub fn new(tag: Tag, vr: Vr, value: Value) -> Self {
        Self { tag, vr, value }
    }
}

/// An ordered collection of DICOM data elements, keyed by Tag.
///
/// Elements are stored in a `BTreeMap` so iteration yields elements in
/// ascending tag order, matching the required DICOM on-disk ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    elements: BTreeMap<Tag, Element>,
}

impl Dataset {
    /// Creates a new empty dataset.
    pub fn new() -> Self {
        Self {
            elements: BTreeMap::new(),
        }
    }

    /// Inserts an element. Replaces any existing element with the same tag.
    pub fn insert(&mut self, element: Element) {
        self.elements.insert(element.tag, element);
    }

    /// Returns a reference to the element with the given tag, if present.
    pub fn get(&self, tag: Tag) -> Option<&Element> {
        self.elements.get(&tag)
    }

    /// Returns a mutable reference to the element with the given tag.
    pub fn get_mut(&mut self, tag: Tag) -> Option<&mut Element> {
        self.elements.get_mut(&tag)
    }

    /// Removes and returns the element with the given tag.
    pub fn remove(&mut self, tag: Tag) -> Option<Element> {
        self.elements.remove(&tag)
    }

    /// Returns `true` if the dataset contains an element with the given tag.
    pub fn contains(&self, tag: Tag) -> bool {
        self.elements.contains_key(&tag)
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns `true` if the dataset has no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns an iterator over all elements in tag order.
    ///
    /// Each element carries its own [`Tag`] via [`Element::tag`]; use
    /// [`Dataset::tags`] when only the tags are needed.
    pub fn iter(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    /// Returns an iterator over all tags in ascending order.
    pub fn tags(&self) -> impl Iterator<Item = Tag> + '_ {
        self.elements.keys().copied()
    }

    /// Returns a string value for the given tag, trimming trailing whitespace/nulls.
    pub fn get_string(&self, tag: Tag) -> Option<&str> {
        self.get(tag).and_then(|e| e.value.as_str())
    }

    /// Returns a u16 value for the given tag.
    pub fn get_u16(&self, tag: Tag) -> Option<u16> {
        self.get(tag).and_then(|e| e.value.as_u16())
    }

    /// Returns a u32 value for the given tag.
    pub fn get_u32(&self, tag: Tag) -> Option<u32> {
        self.get(tag).and_then(|e| e.value.as_u32())
    }

    /// Returns an i32 value for the given tag.
    pub fn get_i32(&self, tag: Tag) -> Option<i32> {
        self.get(tag).and_then(|e| e.value.as_i32())
    }

    /// Returns an f64 value for the given tag.
    pub fn get_f64(&self, tag: Tag) -> Option<f64> {
        self.get(tag).and_then(|e| e.value.as_f64())
    }

    /// Returns all string values for the given tag.
    ///
    /// Single-string values are returned as a single-item vector.
    pub fn get_strs(&self, tag: Tag) -> Option<Vec<&str>> {
        self.get(tag).and_then(|e| match &e.value {
            Value::Str(s) => Some(vec![s.as_str()]),
            Value::Strings(values) => Some(values.iter().map(String::as_str).collect()),
            _ => None,
        })
    }

    /// Returns the number of rows from (0028,0010), or `None` if absent.
    ///
    /// Rows has no DICOM-defined default; callers must decide how to treat a
    /// missing value (for example, `.unwrap_or(0)` to preserve legacy behavior,
    /// or raise a `MissingAttribute` error).
    pub fn rows(&self) -> Option<u16> {
        self.get_u16(tag::ROWS)
    }

    /// Returns the number of columns from (0028,0011), or `None` if absent.
    ///
    /// Columns has no DICOM-defined default; see [`Dataset::rows`] for the
    /// recommended handling of a missing value.
    pub fn columns(&self) -> Option<u16> {
        self.get_u16(tag::COLUMNS)
    }

    /// Returns BitsAllocated from (0028,0100), or `None` if absent.
    ///
    /// The previous default of 16 was invented, not standard-backed; callers
    /// should supply their own fallback via `.unwrap_or(...)`.
    pub fn bits_allocated(&self) -> Option<u16> {
        self.get_u16(tag::BITS_ALLOCATED)
    }

    /// Returns PixelRepresentation from (0028,0103), or `None` if absent.
    ///
    /// The previous default of 0 (unsigned) was invented, not standard-backed;
    /// callers should supply their own fallback via `.unwrap_or(...)`.
    pub fn pixel_representation(&self) -> Option<u16> {
        self.get_u16(tag::PIXEL_REPRESENTATION)
    }

    /// Returns NumberOfFrames from (0028,0008), defaulting to 1.
    ///
    /// Unlike the other pixel accessors, this default is DICOM-defined: an
    /// absent NumberOfFrames means a single-frame image.
    pub fn number_of_frames(&self) -> u32 {
        self.get(tag::NUMBER_OF_FRAMES)
            .and_then(|e| {
                e.value
                    .as_u32()
                    .or_else(|| e.value.as_str().and_then(|s| s.trim().parse::<u32>().ok()))
            })
            .unwrap_or(1)
    }

    /// Returns the Modality value, or an empty string.
    pub fn modality(&self) -> &str {
        self.get_string(tag::MODALITY).unwrap_or("")
    }

    /// Returns the Transfer Syntax UID, defaulting to Explicit VR Little Endian.
    pub fn transfer_syntax(&self) -> TransferSyntax {
        self.get_string(tag::TRANSFER_SYNTAX_UID)
            .map(|s| TransferSyntax::new(s.trim()))
            .unwrap_or_else(|| TransferSyntax::new(transfer::EXPLICIT_VR_LITTLE_ENDIAN))
    }

    /// Returns `true` if the dataset uses an encapsulated transfer syntax.
    pub fn is_encapsulated(&self) -> bool {
        self.transfer_syntax().is_encapsulated()
    }

    /// Returns the pixel data element if present.
    pub fn pixel_data(&self) -> Option<&PixelData> {
        self.get(tag::PIXEL_DATA).and_then(|e| match &e.value {
            Value::PixelData(pd) => Some(pd),
            _ => None,
        })
    }

    /// Convenience helper: creates and inserts a string element.
    pub fn put_string(&mut self, tag: Tag, vr: Vr, value: impl Into<String>) {
        self.insert(Element::new(tag, vr, Value::Str(value.into())));
    }

    /// Convenience helper: creates and inserts a u16 element.
    pub fn put_u16(&mut self, tag: Tag, vr: Vr, value: u16) {
        self.insert(Element::new(tag, vr, Value::U16(value)));
    }

    /// Convenience helper: creates and inserts a u32 element.
    pub fn put_u32(&mut self, tag: Tag, vr: Vr, value: u32) {
        self.insert(Element::new(tag, vr, Value::U32(value)));
    }
}

impl Default for Dataset {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Dataset ({} elements)", self.len())?;
        for elem in self.iter() {
            writeln!(f, "  {} {}: {:?}", elem.tag, elem.vr, elem.value)?;
        }
        Ok(())
    }
}

/// Pixel data with support for both native (uncompressed) and encapsulated
/// (compressed) formats.
///
/// In native format, each frame stores decoded pixel values.
/// In encapsulated format, each frame stores a compressed bitstream item.
#[derive(Debug, Clone, PartialEq)]
pub enum PixelData {
    /// Native uncompressed pixel data.
    Native {
        /// The image frames as decoded pixels.
        frames: Vec<Vec<u16>>,
    },
    /// Encapsulated compressed pixel data.
    Encapsulated {
        /// The compressed frame items.
        frames: Vec<Vec<u8>>,
        /// Basic Offset Table entries.
        offsets: Vec<u32>,
    },
}

impl PixelData {
    /// Creates new native (uncompressed) pixel data from a single frame.
    pub fn native_single(data: Vec<u16>) -> Self {
        Self::Native { frames: vec![data] }
    }

    /// Creates native pixel data from multiple frames.
    pub fn native(frames: Vec<Vec<u16>>) -> Self {
        Self::Native { frames }
    }

    /// Creates encapsulated pixel data from compressed frames and BOT offsets.
    pub fn encapsulated(frames: Vec<Vec<u8>>, offsets: Vec<u32>) -> Self {
        Self::Encapsulated { frames, offsets }
    }

    /// Returns all native frames concatenated into a single slice.
    ///
    /// Returns `None` for encapsulated data.
    pub fn flat_data(&self) -> Option<Vec<u16>> {
        let mut result = Vec::new();
        self.flat_data_into(&mut result)?;
        Some(result)
    }

    /// Writes all native frame data into `out`.
    ///
    /// Returns `None` for encapsulated data.
    pub fn flat_data_into(&self, out: &mut Vec<u16>) -> Option<()> {
        let frames = self.native_frames()?;
        let total: usize = frames.iter().map(Vec::len).sum();
        out.clear();
        out.reserve(total);
        for frame in frames {
            out.extend_from_slice(frame);
        }
        Some(())
    }

    /// Returns the native frame at the given index, or `None` if not native or out of bounds.
    pub fn native_frame(&self, index: usize) -> Option<&[u16]> {
        self.native_frames()?.get(index).map(Vec::as_slice)
    }

    /// Returns the encapsulated frame at the given index, or `None` if not encapsulated or out of bounds.
    pub fn encapsulated_frame(&self, index: usize) -> Option<&[u8]> {
        self.encapsulated_frames()?.get(index).map(Vec::as_slice)
    }

    /// Returns the native frames when the pixel data is uncompressed.
    pub fn native_frames(&self) -> Option<&[Vec<u16>]> {
        match self {
            PixelData::Native { frames } => Some(frames),
            PixelData::Encapsulated { .. } => None,
        }
    }

    /// Returns the encapsulated frames when the pixel data is compressed.
    pub fn encapsulated_frames(&self) -> Option<&[Vec<u8>]> {
        match self {
            PixelData::Encapsulated { frames, .. } => Some(frames),
            PixelData::Native { .. } => None,
        }
    }

    /// Returns the encapsulated Basic Offset Table entries, or an empty slice for native data.
    pub fn offsets(&self) -> &[u32] {
        match self {
            PixelData::Encapsulated { offsets, .. } => offsets,
            PixelData::Native { .. } => &[],
        }
    }

    /// Iterates all native pixels across all frames.
    pub fn iter_native_pixels(&self) -> impl Iterator<Item = u16> + '_ {
        self.native_frames()
            .into_iter()
            .flatten()
            .flat_map(|frame| frame.iter().copied())
    }

    /// Returns the number of frames.
    pub fn num_frames(&self) -> usize {
        match self {
            PixelData::Native { frames } => frames.len(),
            PixelData::Encapsulated { frames, .. } => frames.len(),
        }
    }

    /// Returns `true` if the pixel data is compressed.
    pub fn is_compressed(&self) -> bool {
        matches!(self, PixelData::Encapsulated { .. })
    }

    /// Returns `true` if at least one frame is present.
    pub fn has_frames(&self) -> bool {
        self.num_frames() > 0
    }

    /// Returns the number of pixels in the first frame (native only), or 0.
    pub fn frame_size(&self) -> usize {
        match self {
            PixelData::Native { frames } => frames.first().map_or(0, Vec::len),
            PixelData::Encapsulated { .. } => 0,
        }
    }

    /// Returns the total number of pixels across all frames (native only), or 0.
    pub fn total_pixels(&self) -> usize {
        match self {
            PixelData::Native { frames } => frames.iter().map(Vec::len).sum(),
            PixelData::Encapsulated { .. } => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag;
    use crate::vr::Vr;

    #[test]
    fn dataset_insert_and_get() {
        let mut ds = Dataset::new();
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "DOE^JOHN");
        ds.put_u16(tag::ROWS, Vr::US, 512);

        assert_eq!(ds.len(), 2);
        assert!(!ds.is_empty());

        let name = ds.get_string(tag::PATIENT_NAME);
        assert_eq!(name, Some("DOE^JOHN"));

        assert_eq!(ds.rows(), Some(512));
    }

    #[test]
    fn dataset_remove() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 256);
        assert!(ds.contains(tag::ROWS));

        let removed = ds.remove(tag::ROWS);
        assert!(removed.is_some());
        assert!(!ds.contains(tag::ROWS));
        assert!(ds.is_empty());
    }

    #[test]
    fn dataset_defaults() {
        let ds = Dataset::new();
        assert_eq!(ds.rows(), None);
        assert_eq!(ds.columns(), None);
        assert_eq!(ds.bits_allocated(), None);
        assert_eq!(ds.pixel_representation(), None);
        assert_eq!(ds.number_of_frames(), 1);
        assert_eq!(ds.modality(), "");
    }

    #[test]
    fn dataset_transfer_syntax() {
        let mut ds = Dataset::new();
        // Default when not set
        assert_eq!(
            ds.transfer_syntax().uid(),
            transfer::EXPLICIT_VR_LITTLE_ENDIAN
        );

        ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, transfer::JPEG_LS_LOSSLESS);
        assert!(ds.is_encapsulated());
        assert!(ds.transfer_syntax().is_jpeg_ls());
    }

    #[test]
    fn dataset_ordering() {
        let mut ds = Dataset::new();
        // Insert in reverse order
        ds.put_u16(tag::PIXEL_REPRESENTATION, Vr::US, 0);
        ds.put_u16(tag::ROWS, Vr::US, 128);
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "TEST");
        ds.put_u16(tag::COLUMNS, Vr::US, 256);

        let tags: Vec<Tag> = ds.tags().collect();
        // Should be sorted by group, then element
        for i in 1..tags.len() {
            assert!(tags[i - 1] < tags[i], "Tags should be in order");
        }
    }

    #[test]
    fn value_conversions() {
        assert_eq!(Value::U16(42).as_u16(), Some(42));
        assert_eq!(Value::U16(42).as_u32(), Some(42));
        assert_eq!(Value::U32(100_000).as_u32(), Some(100_000));
        assert_eq!(Value::I32(-5).as_i32(), Some(-5));
        assert_eq!(Value::I16(-3).as_i32(), Some(-3));
        assert_eq!(Value::F64(2.5).as_f64(), Some(2.5));
        assert_eq!(Value::Str("123".into()).as_u16(), Some(123));
        assert_eq!(Value::Str("2.5".into()).as_f64(), Some(2.5));
        assert_eq!(Value::Str("hello".into()).as_str(), Some("hello"));
    }

    #[test]
    fn value_string_helpers() {
        let value = Value::Strings(vec!["A".into(), "B".into()]);
        assert_eq!(value.as_str(), Some("A"));
        assert_eq!(
            value.as_strings().map(<[_]>::to_vec),
            Some(vec!["A".to_string(), "B".to_string()])
        );
    }

    #[test]
    fn value_conversion_failures() {
        assert_eq!(Value::Bytes(vec![1, 2]).as_str(), None);
        assert_eq!(Value::Str("abc".into()).as_u16(), None);
        assert_eq!(Value::U16(5).as_str(), None);
    }

    #[test]
    fn pixel_data_native() {
        let pd = PixelData::native_single(vec![100, 200, 300, 400]);
        assert!(!pd.is_compressed());
        assert!(pd.has_frames());
        assert_eq!(pd.num_frames(), 1);
        assert_eq!(pd.frame_size(), 4);
        assert_eq!(pd.total_pixels(), 4);
        assert_eq!(pd.flat_data(), Some(vec![100, 200, 300, 400]));
    }

    #[test]
    fn pixel_data_native_helpers() {
        let pd = PixelData::native(vec![vec![1, 2], vec![3, 4]]);

        let mut out = Vec::new();
        assert_eq!(pd.flat_data_into(&mut out), Some(()));
        assert_eq!(out, vec![1, 2, 3, 4]);
        assert_eq!(pd.native_frames().unwrap().len(), 2);
        assert_eq!(
            pd.iter_native_pixels().collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn pixel_data_encapsulated() {
        let pd = PixelData::encapsulated(vec![vec![0xFF, 0xD8, 0x01, 0x02]], vec![0]);
        assert!(pd.is_compressed());
        assert!(pd.has_frames());
        assert_eq!(pd.frame_size(), 0);
        assert_eq!(pd.total_pixels(), 0);
        assert_eq!(pd.flat_data(), None);
    }

    #[test]
    fn pixel_data_multi_frame() {
        let pd = PixelData::native(vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]]);
        assert_eq!(pd.num_frames(), 2);
        assert_eq!(pd.total_pixels(), 8);
        assert_eq!(pd.flat_data(), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(pd.native_frame(0), Some(&[1, 2, 3, 4][..]));
        assert_eq!(pd.native_frame(1), Some(&[5, 6, 7, 8][..]));
        assert!(pd.native_frame(2).is_none());
    }

    #[test]
    fn dataset_with_pixel_data() {
        let mut ds = Dataset::new();
        let pd = PixelData::native_single(vec![10, 20, 30, 40]);
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::PixelData(pd)));
        let retrieved = ds.pixel_data().expect("should have pixel data");
        assert_eq!(retrieved.num_frames(), 1);
        assert_eq!(retrieved.flat_data(), Some(vec![10, 20, 30, 40]));
    }

    #[test]
    fn dataset_display() {
        let mut ds = Dataset::new();
        ds.put_string(tag::PATIENT_NAME, Vr::PN, "TEST");
        let s = format!("{ds}");
        assert!(s.contains("Dataset"));
        assert!(s.contains("1 elements"));
    }

    #[test]
    fn number_of_frames_from_string() {
        let mut ds = Dataset::new();
        ds.put_string(tag::NUMBER_OF_FRAMES, Vr::IS, "10");
        assert_eq!(ds.number_of_frames(), 10);
    }

    #[test]
    fn dataset_get_strs_for_multi_value() {
        let mut ds = Dataset::new();
        ds.insert(Element::new(
            tag::IMAGE_TYPE,
            Vr::CS,
            Value::Strings(vec!["ORIGINAL".into(), "PRIMARY".into()]),
        ));

        assert_eq!(
            ds.get_strs(tag::IMAGE_TYPE),
            Some(vec!["ORIGINAL", "PRIMARY"])
        );
        assert_eq!(ds.get_string(tag::IMAGE_TYPE), Some("ORIGINAL"));
    }

    #[test]
    fn dataset_sequence_value() {
        let mut inner = Dataset::new();
        inner.put_string(tag::SOP_CLASS_UID, Vr::UI, "1.2.3");

        let mut ds = Dataset::new();
        ds.insert(Element::new(
            tag::REFERENCED_IMAGE_SEQUENCE,
            Vr::SQ,
            Value::Sequence(vec![inner]),
        ));

        if let Some(elem) = ds.get(tag::REFERENCED_IMAGE_SEQUENCE) {
            if let Value::Sequence(items) = &elem.value {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].get_string(tag::SOP_CLASS_UID), Some("1.2.3"));
            } else {
                panic!("Expected Sequence value");
            }
        } else {
            panic!("Expected element");
        }
    }
}
