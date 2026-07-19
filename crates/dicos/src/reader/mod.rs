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

use std::fmt;
use std::io::{self, Read};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::DicosError;
use crate::tag::{self, Tag};
use crate::transfer;
use crate::types::{Dataset, Element, PixelData, Value};
use crate::vr::Vr;

/// A non-fatal issue detected while parsing.
///
/// Returned by [`parse_with_warnings`] and [`parse_with_warnings_and_limit`].
/// The convenience [`parse`]/[`parse_with_limit`] entry points log each
/// warning via `log::warn!` instead of returning them.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseWarning {
    /// Native pixel data had an odd byte length; the trailing byte was ignored.
    OddPixelDataLength {
        /// The odd byte length of the raw pixel data.
        length: usize,
    },
    /// Native pixel count did not match `rows * columns * frames`.
    PixelCountMismatch {
        /// The number of pixels actually decoded.
        actual: usize,
        /// The number of pixels expected from the image geometry.
        expected: usize,
    },
}

impl fmt::Display for ParseWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseWarning::OddPixelDataLength { length } => write!(
                f,
                "native pixel data has an odd byte length ({length}); trailing byte ignored"
            ),
            ParseWarning::PixelCountMismatch { actual, expected } => write!(
                f,
                "native pixel data size ({actual} pixels) does not match rows*cols*frames \
                 ({expected}); storing as a single frame"
            ),
        }
    }
}

/// The 4-byte DICOM magic number.
const DICM_MAGIC: &[u8; 4] = b"DICM";

/// Sentinel for undefined length.
const UNDEFINED_LENGTH: u32 = 0xFFFF_FFFF;

/// Default per-element allocation limit (1 GB).
const DEFAULT_MAX_ELEMENT_LENGTH: usize = 1024 * 1024 * 1024;

/// Maximum sequence/item nesting depth.
///
/// Bounds recursion on attacker-controlled nesting so a crafted file with
/// deeply nested sequences cannot overflow the stack. Legitimate DICOS/DICOM
/// files nest only a handful of levels.
const MAX_NESTING_DEPTH: usize = 64;

/// Allocate a buffer of `len` bytes, rejecting sizes above `limit`.
fn checked_alloc(len: u32, limit: usize) -> Result<Vec<u8>, DicosError> {
    let n = len as usize;
    if n > limit {
        return Err(DicosError::LengthExceedsLimit { length: n, limit });
    }
    Ok(vec![0u8; n])
}

/// Parses a DICOS/DICOM file from a reader.
///
/// Uses a default per-element allocation limit of 1 GB. For custom limits,
/// use [`parse_with_limit`]. Any [`ParseWarning`]s are logged via `log::warn!`;
/// use [`parse_with_warnings`] to receive them instead.
pub fn parse<R: Read>(reader: R) -> Result<Dataset, DicosError> {
    parse_with_limit(reader, DEFAULT_MAX_ELEMENT_LENGTH)
}

/// Parses a DICOS/DICOM file with a custom per-element allocation limit.
///
/// Any single element whose on-disk length exceeds `max_element_bytes` is
/// rejected. This guards against malicious files without restricting
/// legitimate large volumes. Any [`ParseWarning`]s are logged via `log::warn!`.
pub fn parse_with_limit<R: Read>(
    reader: R,
    max_element_bytes: usize,
) -> Result<Dataset, DicosError> {
    let (ds, warnings) = parse_with_warnings_and_limit(reader, max_element_bytes)?;
    for w in &warnings {
        log::warn!("{w}");
    }
    Ok(ds)
}

/// Parses a DICOS/DICOM file, returning the dataset alongside any
/// [`ParseWarning`]s rather than logging them.
///
/// Uses a default per-element allocation limit of 1 GB. Warnings are not stored
/// on the returned [`Dataset`].
pub fn parse_with_warnings<R: Read>(reader: R) -> Result<(Dataset, Vec<ParseWarning>), DicosError> {
    parse_with_warnings_and_limit(reader, DEFAULT_MAX_ELEMENT_LENGTH)
}

/// Parses a DICOS/DICOM file with a custom per-element allocation limit,
/// returning the dataset alongside any [`ParseWarning`]s.
pub fn parse_with_warnings_and_limit<R: Read>(
    reader: R,
    max_element_bytes: usize,
) -> Result<(Dataset, Vec<ParseWarning>), DicosError> {
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
    /// Number of bytes consumed from `inner`, used for truncation offsets.
    bytes_read: u64,
}

impl<R: Read> DicosReader<R> {
    fn new(inner: R, max_element_bytes: usize) -> Self {
        Self {
            inner,
            explicit_vr: true, // Group 0002 is always explicit
            transfer_syntax_uid: None,
            in_meta: true,
            max_element_bytes,
            bytes_read: 0,
        }
    }

    /// Reads into `buf`, returning how many bytes were actually read.
    ///
    /// Distinguishes a clean end-of-stream (`Ok(0)` at a boundary) from a short
    /// read; callers decide whether a short read is a truncation.
    fn fill(&mut self, buf: &mut [u8]) -> Result<usize, DicosError> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.inner.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.bytes_read += filled as u64;
                    return Err(DicosError::Io(e));
                }
            }
        }
        self.bytes_read += filled as u64;
        Ok(filled)
    }

    /// Reads exactly `buf.len()` bytes, mapping a short read to `Truncated`.
    fn read_exact_ctx(&mut self, buf: &mut [u8], context: &'static str) -> Result<(), DicosError> {
        let offset = self.bytes_read;
        let n = self.fill(buf)?;
        if n < buf.len() {
            return Err(DicosError::Truncated { offset, context });
        }
        Ok(())
    }

    /// Reads a little-endian `u16`, mapping a short read to `Truncated`.
    fn read_u16_ctx(&mut self, context: &'static str) -> Result<u16, DicosError> {
        let mut b = [0u8; 2];
        self.read_exact_ctx(&mut b, context)?;
        Ok(u16::from_le_bytes(b))
    }

    /// Reads a little-endian `u32`, mapping a short read to `Truncated`.
    fn read_u32_ctx(&mut self, context: &'static str) -> Result<u32, DicosError> {
        let mut b = [0u8; 4];
        self.read_exact_ctx(&mut b, context)?;
        Ok(u32::from_le_bytes(b))
    }

    /// Reads a 4-byte tag at an element boundary.
    ///
    /// Returns `Ok(None)` on a clean EOF (zero bytes available), `Ok(Some(tag))`
    /// for a full tag, and `Err(Truncated)` when only part of a tag is present.
    fn read_boundary_tag(&mut self, context: &'static str) -> Result<Option<Tag>, DicosError> {
        let offset = self.bytes_read;
        let mut buf = [0u8; 4];
        let n = self.fill(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        if n < 4 {
            return Err(DicosError::Truncated { offset, context });
        }
        let group = u16::from_le_bytes([buf[0], buf[1]]);
        let element = u16::from_le_bytes([buf[2], buf[3]]);
        Ok(Some(Tag::new(group, element)))
    }

    /// Reads a tag that must be present; a clean EOF here is a truncation.
    fn read_tag_required(&mut self, context: &'static str) -> Result<Tag, DicosError> {
        match self.read_boundary_tag(context)? {
            Some(t) => Ok(t),
            None => Err(DicosError::Truncated {
                offset: self.bytes_read,
                context,
            }),
        }
    }

    /// Skips `len` bytes, verifying the full count was consumed.
    fn skip_bytes(&mut self, len: u32) -> Result<(), DicosError> {
        let offset = self.bytes_read;
        let copied = io::copy(&mut (&mut self.inner).take(u64::from(len)), &mut io::sink())?;
        self.bytes_read += copied;
        if copied != u64::from(len) {
            return Err(DicosError::Truncated {
                offset,
                context: "skipped element body",
            });
        }
        Ok(())
    }

    /// Applies the dataset transfer syntax when crossing out of group 0002.
    ///
    /// No-op unless we are still in the File Meta Information group and the tag
    /// belongs to the dataset proper. Rejects big-endian transfer syntaxes.
    fn enter_dataset_if_needed(&mut self, tag: Tag) -> Result<(), DicosError> {
        if tag.group == 0x0002 || !self.in_meta {
            return Ok(());
        }
        self.in_meta = false;
        if self.transfer_syntax_uid.is_none() {
            // No TransferSyntaxUID was present; default to Implicit VR LE per
            // DICOM PS3.5 §10.1.
            self.transfer_syntax_uid = Some(transfer::IMPLICIT_VR_LITTLE_ENDIAN.to_string());
        }
        self.update_transfer_syntax();

        if let Some(uid) = &self.transfer_syntax_uid {
            let ts = transfer::TransferSyntax::new(uid.as_str());
            if !ts.is_little_endian() {
                return Err(DicosError::UnsupportedTransferSyntax(uid.clone()));
            }
        }
        Ok(())
    }

    /// Reads elements until a clean EOF or a group-`0xFFFE` delimiter tag.
    ///
    /// Each parsed element is passed to `on_element`. Returns the delimiter tag
    /// that stopped the loop, or `None` on a clean end-of-stream. The delimiter's
    /// trailing length field is left unconsumed for the caller to handle.
    fn parse_elements_until(
        &mut self,
        depth: usize,
        mut on_element: impl FnMut(&mut Self, Element) -> Result<(), DicosError>,
    ) -> Result<Option<Tag>, DicosError> {
        loop {
            let tag = match self.read_boundary_tag("element tag")? {
                None => return Ok(None),
                Some(t) if t.group == 0xFFFE => return Ok(Some(t)),
                Some(t) => t,
            };
            self.enter_dataset_if_needed(tag)?;
            let elem = self.read_element_with_tag(tag, depth)?;
            on_element(self, elem)?;
        }
    }

    fn read_dataset(&mut self) -> Result<(Dataset, Vec<ParseWarning>), DicosError> {
        let mut ds = Dataset::new();

        // 1. Read 128-byte preamble
        let mut preamble = [0u8; 128];
        if self.fill(&mut preamble)? != preamble.len() {
            return Err(DicosError::BadPreamble {
                reason: "preamble too short",
            });
        }

        // 2. Read "DICM" magic
        let mut magic = [0u8; 4];
        if self.fill(&mut magic)? != magic.len() || &magic != DICM_MAGIC {
            return Err(DicosError::BadPreamble {
                reason: "missing DICM magic number",
            });
        }

        // 3. Group 0002 is always Explicit VR Little Endian
        self.explicit_vr = true;
        self.in_meta = true;

        // 4. Read elements. The transfer-syntax transition happens inside
        // `enter_dataset_if_needed`; capturing (0002,0010) happens here.
        self.parse_elements_until(0, |this, elem| {
            if elem.tag == tag::TRANSFER_SYNTAX_UID {
                if let Value::Str(ref s) = elem.value {
                    this.transfer_syntax_uid = Some(s.trim().to_string());
                    // Do NOT switch VR mode yet -- still reading group 0002.
                }
            }
            ds.insert(elem);
            Ok(())
        })?;

        // Materialize the inferred default TS so Dataset::transfer_syntax()
        // agrees with the VR mode the reader actually used (issue #8). This only
        // fires when the file carried no TransferSyntaxUID of its own.
        if !ds.contains(tag::TRANSFER_SYNTAX_UID) {
            if let Some(uid) = self.transfer_syntax_uid.clone() {
                ds.put_string(tag::TRANSFER_SYNTAX_UID, Vr::UI, uid);
            }
        }

        let warnings = normalize_native_pixel_data(&mut ds);
        Ok((ds, warnings))
    }

    /// Reads an element after the tag has already been consumed.
    ///
    /// `depth` is the current sequence-nesting level; it is propagated to any
    /// nested sequence parsing and bounded by [`MAX_NESTING_DEPTH`].
    fn read_element_with_tag(&mut self, tag: Tag, depth: usize) -> Result<Element, DicosError> {
        // Sequence delimiters always have implicit structure: 4-byte length, no VR
        if tag.group == 0xFFFE {
            let len = self.read_u32_ctx("delimiter length")?;
            let value = if len > 0 && len != UNDEFINED_LENGTH {
                let mut buf = checked_alloc(len, self.max_element_bytes)?;
                self.read_exact_ctx(&mut buf, "delimiter body")?;
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

        let value = self.read_value(tag, vr, vl, depth)?;

        Ok(Element::new(tag, vr, value))
    }

    /// Reads VR + length in Explicit VR mode.
    fn read_explicit_vr_header(&mut self) -> Result<(Vr, u32), DicosError> {
        let mut vr_buf = [0u8; 2];
        self.read_exact_ctx(&mut vr_buf, "explicit VR header")?;
        let vr = Vr::from_bytes(&vr_buf).unwrap_or(Vr::UN);

        let vl = if vr.is_long_vr() {
            // 2 reserved bytes + 4-byte length
            let mut reserved = [0u8; 2];
            self.read_exact_ctx(&mut reserved, "explicit VR header")?;
            self.read_u32_ctx("explicit VR length")?
        } else {
            // 2-byte length
            u32::from(self.read_u16_ctx("explicit VR length")?)
        };

        Ok((vr, vl))
    }

    /// Reads length in Implicit VR mode and infers VR from tag.
    fn read_implicit_vr_header(&mut self, tag: Tag) -> Result<(Vr, u32), DicosError> {
        let vl = self.read_u32_ctx("implicit VR length")?;
        let vr = implicit_vr_for_tag(tag);
        Ok((vr, vl))
    }

    /// Reads the value bytes and parses them according to VR.
    fn read_value(&mut self, tag: Tag, vr: Vr, vl: u32, depth: usize) -> Result<Value, DicosError> {
        if vl == UNDEFINED_LENGTH {
            return self.read_undefined_length_value(tag, vr, depth);
        }

        let mut data = checked_alloc(vl, self.max_element_bytes)?;
        self.read_exact_ctx(&mut data, "element value")?;

        parse_value(vr, &data, self.explicit_vr, self.max_element_bytes, depth)
    }

    /// Handles elements with undefined length: encapsulated pixel data or sequences.
    fn read_undefined_length_value(
        &mut self,
        tag: Tag,
        vr: Vr,
        depth: usize,
    ) -> Result<Value, DicosError> {
        if tag == tag::PIXEL_DATA {
            let pd = self.read_encapsulated_pixel_data()?;
            return Ok(Value::PixelData(pd));
        }

        if vr == Vr::SQ {
            let items = self.read_sequence_items(depth)?;
            return Ok(Value::Sequence(items));
        }

        // Unknown undefined-length element: skip until sequence delimitation
        self.skip_undefined_length(depth)?;
        Ok(Value::Bytes(Vec::new()))
    }

    /// Reads sequence items until the Sequence Delimitation Item tag.
    fn read_sequence_items(&mut self, depth: usize) -> Result<Vec<Dataset>, DicosError> {
        if depth >= MAX_NESTING_DEPTH {
            return Err(DicosError::NestingTooDeep {
                max: MAX_NESTING_DEPTH,
            });
        }

        let mut items = Vec::new();

        loop {
            let item_tag = self.read_tag_required("sequence item")?;
            let item_len = self.read_u32_ctx("sequence item length")?;

            if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                break;
            }

            if item_tag != tag::ITEM {
                return Err(DicosError::UnexpectedTag {
                    expected: tag::ITEM,
                    got: item_tag,
                    context: "sequence item",
                });
            }

            let item_ds = if item_len == UNDEFINED_LENGTH {
                self.read_item_undefined_length(depth)?
            } else {
                self.read_item_fixed_length(item_len, depth)?
            };

            items.push(item_ds);
        }

        Ok(items)
    }

    /// Reads a sequence item with undefined length (delimited by Item Delimitation Item).
    fn read_item_undefined_length(&mut self, depth: usize) -> Result<Dataset, DicosError> {
        let mut ds = Dataset::new();

        loop {
            let elem_tag = self.read_tag_required("sequence item element")?;

            if elem_tag == tag::ITEM_DELIMITATION_ITEM {
                // Read and discard the 4-byte zero length
                let _len = self.read_u32_ctx("item delimiter length")?;
                break;
            }

            let elem = self.read_element_with_tag(elem_tag, depth + 1)?;
            ds.insert(elem);
        }

        Ok(ds)
    }

    /// Reads a sequence item with a known fixed length.
    fn read_item_fixed_length(&mut self, len: u32, depth: usize) -> Result<Dataset, DicosError> {
        let mut buf = checked_alloc(len, self.max_element_bytes)?;
        self.read_exact_ctx(&mut buf, "fixed-length item body")?;

        // Parse the item bytes as a mini-dataset
        let mut sub_reader = DicosReader {
            inner: io::Cursor::new(buf),
            explicit_vr: self.explicit_vr,
            transfer_syntax_uid: self.transfer_syntax_uid.clone(),
            in_meta: false,
            max_element_bytes: self.max_element_bytes,
            bytes_read: 0,
        };

        let mut ds = Dataset::new();
        sub_reader.parse_elements_until(depth + 1, |_, elem| {
            ds.insert(elem);
            Ok(())
        })?;

        Ok(ds)
    }

    /// Reads encapsulated pixel data (Basic Offset Table + compressed frames).
    fn read_encapsulated_pixel_data(&mut self) -> Result<PixelData, DicosError> {
        let mut offsets = Vec::new();
        let mut frames = Vec::new();

        // Read Basic Offset Table item
        let bot_tag = self.read_tag_required("basic offset table")?;
        if bot_tag != tag::ITEM {
            return Err(DicosError::UnexpectedTag {
                expected: tag::ITEM,
                got: bot_tag,
                context: "basic offset table",
            });
        }

        let bot_len = self.read_u32_ctx("basic offset table length")?;
        if bot_len > 0 {
            let num_offsets = bot_len / 4;
            offsets.reserve(num_offsets as usize);
            for _ in 0..num_offsets {
                offsets.push(self.read_u32_ctx("basic offset table entry")?);
            }
        }

        // Read frames until Sequence Delimitation Item
        loop {
            let item_tag = self.read_tag_required("encapsulated pixel data")?;

            if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                // Read and discard length (should be 0)
                let _len = self.read_u32_ctx("pixel data delimiter length")?;
                break;
            }

            if item_tag != tag::ITEM {
                return Err(DicosError::UnexpectedTag {
                    expected: tag::ITEM,
                    got: item_tag,
                    context: "encapsulated pixel data",
                });
            }

            let item_len = self.read_u32_ctx("encapsulated frame length")?;
            let mut frame_data = checked_alloc(item_len, self.max_element_bytes)?;
            self.read_exact_ctx(&mut frame_data, "encapsulated frame")?;

            frames.push(frame_data);
        }

        Ok(PixelData::encapsulated(frames, offsets))
    }

    /// Skips an undefined-length element by reading until Sequence Delimitation Item.
    fn skip_undefined_length(&mut self, depth: usize) -> Result<(), DicosError> {
        if depth >= MAX_NESTING_DEPTH {
            return Err(DicosError::NestingTooDeep {
                max: MAX_NESTING_DEPTH,
            });
        }

        loop {
            let item_tag = self.read_tag_required("skipped sequence")?;

            if item_tag.group == 0xFFFE {
                let len = self.read_u32_ctx("skipped item length")?;

                if item_tag == tag::SEQUENCE_DELIMITATION_ITEM {
                    return Ok(());
                }

                if item_tag == tag::ITEM_DELIMITATION_ITEM {
                    continue;
                }

                // Item start
                if len == UNDEFINED_LENGTH {
                    // Nested undefined length
                    self.skip_undefined_length(depth + 1)?;
                } else if len > 0 {
                    self.skip_bytes(len)?;
                }
                continue;
            }

            // Regular element within the sequence -- skip it, reusing the shared
            // VR-header readers rather than reimplementing the header parse.
            let (_vr, vl) = if self.explicit_vr {
                self.read_explicit_vr_header()?
            } else {
                self.read_implicit_vr_header(item_tag)?
            };

            if vl == UNDEFINED_LENGTH {
                self.skip_undefined_length(depth + 1)?;
            } else if vl > 0 {
                self.skip_bytes(vl)?;
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
fn parse_value(
    vr: Vr,
    data: &[u8],
    explicit_vr: bool,
    max_element_bytes: usize,
    depth: usize,
) -> Result<Value, DicosError> {
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
            let items = parse_sequence_items(data, explicit_vr, max_element_bytes, depth)?;
            Ok(Value::Sequence(items))
        }

        // Binary / Unknown
        Vr::OB | Vr::OD | Vr::OF | Vr::OL | Vr::OW | Vr::UN => Ok(Value::Bytes(data.to_vec())),
    }
}

/// Parses sequence items from a byte buffer (fixed-length SQ).
///
/// Reuses `DicosReader` for element parsing to avoid duplicating VR/header logic.
fn parse_sequence_items(
    data: &[u8],
    explicit_vr: bool,
    max_element_bytes: usize,
    depth: usize,
) -> Result<Vec<Dataset>, DicosError> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(DicosError::NestingTooDeep {
            max: MAX_NESTING_DEPTH,
        });
    }

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
            return Err(DicosError::UnexpectedTag {
                expected: tag::ITEM,
                got: item_tag,
                context: "fixed-length SQ item",
            });
        }

        let item_len = cursor.read_u32::<LittleEndian>()?;

        if item_len == UNDEFINED_LENGTH {
            return Err(DicosError::UndefinedLengthInFixedSq);
        }

        let pos = cursor.position() as usize;
        let end = pos + item_len as usize;
        if end > data.len() {
            return Err(DicosError::LengthExceedsBuffer {
                length: item_len as usize,
                remaining: data.len() - pos,
            });
        }

        let item_bytes = &data[pos..end];
        cursor.set_position(end as u64);

        let mut sub_reader = DicosReader {
            inner: io::Cursor::new(item_bytes),
            explicit_vr,
            transfer_syntax_uid: None,
            in_meta: false,
            max_element_bytes,
            bytes_read: 0,
        };

        let mut ds = Dataset::new();
        sub_reader.parse_elements_until(depth + 1, |_, elem| {
            ds.insert(elem);
            Ok(())
        })?;

        items.push(ds);
    }

    Ok(items)
}

/// Normalizes raw OW/OB pixel data bytes to PixelData::Native after parsing.
///
/// The reader stores fixed-length (7FE0,0010) as Value::Bytes. This converts
/// them to PixelData::Native so Dataset::pixel_data() works for all datasets.
fn normalize_native_pixel_data(ds: &mut Dataset) -> Vec<ParseWarning> {
    let mut warnings = Vec::new();

    // Rows/Columns are now Option; a missing dimension cannot describe a frame
    // grid, so treat absent as 0 here (preserving the prior early-return path).
    let rows = ds.rows().unwrap_or(0) as usize;
    let cols = ds.columns().unwrap_or(0) as usize;
    let num_frames = ds.number_of_frames() as usize;

    if rows == 0 || cols == 0 {
        return warnings;
    }

    let frame_pixels = rows * cols;

    // Check if pixel data is raw bytes (uncompressed, fixed-length)
    let should_normalize = matches!(
        ds.get(tag::PIXEL_DATA).map(|e| &e.value),
        Some(Value::Bytes(_))
    );

    if !should_normalize {
        return warnings;
    }

    let raw_bytes = match ds.remove(tag::PIXEL_DATA) {
        Some(elem) => match elem.value {
            Value::Bytes(b) => b,
            _ => return warnings,
        },
        None => return warnings,
    };

    // Decode as little-endian u16 pixels. A trailing odd byte cannot belong to
    // a 16-bit pixel, so flag it rather than dropping it silently.
    if raw_bytes.len() % 2 != 0 {
        warnings.push(ParseWarning::OddPixelDataLength {
            length: raw_bytes.len(),
        });
    }
    let all_pixels: Vec<u16> = raw_bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();

    let expected = frame_pixels.saturating_mul(num_frames.max(1));
    let frames: Vec<Vec<u16>> = if num_frames > 1 && all_pixels.len() == frame_pixels * num_frames {
        all_pixels
            .chunks(frame_pixels)
            .map(|c| c.to_vec())
            .collect()
    } else {
        if all_pixels.len() != expected {
            warnings.push(ParseWarning::PixelCountMismatch {
                actual: all_pixels.len(),
                expected,
            });
        }
        vec![all_pixels]
    };

    ds.insert(Element::new(
        tag::PIXEL_DATA,
        Vr::OW,
        Value::PixelData(PixelData::Native { frames }),
    ));

    warnings
}

#[cfg(test)]
pub(crate) mod test_util;

#[cfg(test)]
mod tests;
