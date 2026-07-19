use super::test_util::*;
use super::*;
use crate::tag;
use crate::transfer;

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
    assert_eq!(ds.rows(), Some(512));
    assert_eq!(ds.columns(), Some(256));
    assert_eq!(ds.bits_allocated(), Some(16));
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
    assert!(
        matches!(result, Err(DicosError::BadPreamble { .. })),
        "expected BadPreamble, got: {result:?}"
    );
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

    assert_eq!(ds.rows(), Some(1024));
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

#[test]
fn parse_rejects_big_endian_transfer_syntax() {
    let buf = build_minimal_dicos_with_ts(transfer::EXPLICIT_VR_BIG_ENDIAN);
    let result = parse(io::Cursor::new(buf));
    assert!(
        matches!(result, Err(DicosError::UnsupportedTransferSyntax(_))),
        "Expected UnsupportedTransferSyntax, got: {result:?}"
    );
}

#[test]
fn parse_without_transfer_syntax_defaults_to_implicit_vr_le() {
    // A file missing TransferSyntaxUID in group 0002 must be treated as
    // Implicit VR Little Endian per DICOM PS3.5 §10.1.  After parsing, both
    // the reader state AND Dataset::transfer_syntax() must agree on that
    // value — this was the inconsistency tracked in issue #8.
    let buf = build_dicos_without_transfer_syntax();
    let ds = parse(io::Cursor::new(buf)).expect("parse should succeed");

    // The inferred TS must be materialized in the dataset.
    assert_eq!(
        ds.transfer_syntax().uid(),
        transfer::IMPLICIT_VR_LITTLE_ENDIAN,
        "Dataset::transfer_syntax() should return Implicit VR LE when no TS tag was present"
    );

    // The dataset element should have been parsed correctly under implicit VR LE.
    assert_eq!(ds.rows(), Some(64));
}

// -- Integration tests against real DICOS files --

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

    assert!(ds.rows().is_some_and(|r| r > 0), "rows should be non-zero");
    assert!(
        ds.columns().is_some_and(|c| c > 0),
        "columns should be non-zero"
    );
    assert!(ds.bits_allocated().is_some_and(|b| b > 0));
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

    assert_eq!(ds.rows(), Some(2));
    assert_eq!(ds.columns(), Some(2));

    // Raw OW bytes are normalized to PixelData::Native after parsing
    let pd = ds.pixel_data().expect("pixel_data() should return Some");
    let frames = pd.native_frames().expect("should be native pixel data");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0], vec![100u16, 200, 300, 400]);
}

// -----------------------------------------------------------------------
// Fixed-length SQ parsing tests (issue #3)
// -----------------------------------------------------------------------

/// A well-formed fixed-length SQ with one item containing a valid element
/// must parse correctly and return the expected value.
#[test]
fn parse_fixed_length_sq_valid_item() {
    // Build one item containing (0028,0010) Rows = 128.
    let inner = encode_explicit_elem(&Element::new(tag::ROWS, Vr::US, Value::U16(128)));
    let item_bytes = build_sq_item(&inner);
    let sq_bytes = build_fixed_sq(tag::REFERENCED_IMAGE_SEQUENCE, &item_bytes);

    let mut file = build_minimal_explicit_vr_le(&[]);
    file.extend_from_slice(&sq_bytes);

    let ds = parse(io::Cursor::new(file)).expect("valid fixed-length SQ should parse");
    let sq_elem = ds
        .get(tag::REFERENCED_IMAGE_SEQUENCE)
        .expect("SQ element must be present");

    match &sq_elem.value {
        Value::Sequence(items) => {
            assert_eq!(items.len(), 1, "expected exactly one item");
            assert_eq!(
                items[0].rows(),
                Some(128),
                "inner element should decode rows=128"
            );
        }
        other => panic!("expected Value::Sequence, got {other:?}"),
    }
}

/// A fixed-length SQ whose item length field claims more bytes than are
/// present in the buffer must return an error rather than silently returning
/// partial data.
///
/// This is the primary regression guard for issue #3: before the fix the
/// inner loop would `break` on the I/O error and return an empty Dataset
/// without surfacing the problem to the caller.
#[test]
fn parse_fixed_length_sq_item_length_exceeds_buffer_returns_error() {
    // Item claims 64 bytes of content but only 4 bytes follow.
    let mut items_bytes = Vec::new();
    items_bytes.extend_from_slice(&0xFFFEu16.to_le_bytes()); // item group
    items_bytes.extend_from_slice(&0xE000u16.to_le_bytes()); // item element
    items_bytes.extend_from_slice(&64u32.to_le_bytes()); // claimed length: 64
    items_bytes.extend_from_slice(&[0x00, 0x08, 0x60, 0x00]); // only 4 actual bytes

    // The SQ length matches what we actually wrote (tag 8 bytes + 4 payload bytes).
    let sq_bytes = build_fixed_sq(tag::REFERENCED_IMAGE_SEQUENCE, &items_bytes);

    let mut file = build_minimal_explicit_vr_le(&[]);
    file.extend_from_slice(&sq_bytes);

    let result = parse(io::Cursor::new(file));
    // The error must describe the length mismatch via the typed variant —
    // not a swallowed EOF surfaced as a generic I/O error.
    assert!(
        matches!(result, Err(DicosError::LengthExceedsBuffer { .. })),
        "expected LengthExceedsBuffer, got: {result:?}"
    );
}

/// A fixed-length SQ item whose body is a valid but empty byte slice
/// (zero-length item) must parse as an empty Dataset, not an error.
#[test]
fn parse_fixed_length_sq_empty_item_is_valid() {
    let item_bytes = build_sq_item(&[]);
    let sq_bytes = build_fixed_sq(tag::REFERENCED_IMAGE_SEQUENCE, &item_bytes);

    let mut file = build_minimal_explicit_vr_le(&[]);
    file.extend_from_slice(&sq_bytes);

    let ds = parse(io::Cursor::new(file)).expect("empty SQ item should parse");
    let sq_elem = ds
        .get(tag::REFERENCED_IMAGE_SEQUENCE)
        .expect("SQ element must be present");

    match &sq_elem.value {
        Value::Sequence(items) => {
            assert_eq!(items.len(), 1, "one item expected");
            assert!(items[0].is_empty(), "item dataset should be empty");
        }
        other => panic!("expected Value::Sequence, got {other:?}"),
    }
}

// -----------------------------------------------------------------------
// Nesting-depth guard (stack-overflow DoS protection)
// -----------------------------------------------------------------------

/// A pathologically deep tower of nested sequences must be rejected with a
/// depth error rather than recursing until the stack overflows.
#[test]
fn parse_rejects_excessive_sequence_nesting() {
    let mut payload = Vec::new();
    for _ in 0..(MAX_NESTING_DEPTH + 16) {
        payload = wrap_undefined_length_sq(&payload);
    }

    let mut file = build_minimal_explicit_vr_le(&[]);
    file.extend_from_slice(&payload);

    let result = parse(io::Cursor::new(file));
    assert!(
        matches!(result, Err(DicosError::NestingTooDeep { .. })),
        "deeply nested sequences must error on depth, got: {result:?}"
    );
}

/// Nesting up to (but not beyond) the limit must still parse successfully —
/// the guard must not reject legitimately structured files.
#[test]
fn parse_accepts_nesting_within_limit() {
    let mut payload = Vec::new();
    for _ in 0..(MAX_NESTING_DEPTH - 1) {
        payload = wrap_undefined_length_sq(&payload);
    }

    let mut file = build_minimal_explicit_vr_le(&[]);
    file.extend_from_slice(&payload);

    let ds = parse(io::Cursor::new(file)).expect("nesting within limit should parse");
    assert!(ds.get(tag::REFERENCED_IMAGE_SEQUENCE).is_some());
}

// -----------------------------------------------------------------------
// Truncation semantics
// -----------------------------------------------------------------------

/// A clean EOF exactly at an element boundary is a normal end-of-stream; a
/// partial tag one byte later is a `Truncated` error.
#[test]
fn clean_eof_at_boundary_vs_partial_header() {
    let elements = vec![
        Element::new(tag::ROWS, Vr::US, Value::U16(2)),
        Element::new(tag::COLUMNS, Vr::US, Value::U16(2)),
    ];
    let data = build_minimal_explicit_vr_le(&elements);

    // Preamble (128) + "DICM" (4) = 132: a clean boundary with no elements.
    let at_magic =
        parse(io::Cursor::new(data[..132].to_vec())).expect("clean EOF after magic parses");
    assert!(at_magic.is_empty(), "no elements should be decoded yet");

    // One byte past the magic is a partial tag -> Truncated.
    let partial = parse(io::Cursor::new(data[..133].to_vec()));
    assert!(
        matches!(partial, Err(DicosError::Truncated { .. })),
        "a partial tag must be Truncated, got: {partial:?}"
    );
}

/// Every truncated prefix of a valid file must either error cleanly
/// (`Truncated`/`Io`/`BadPreamble`, never a panic) or, when it happens to
/// end at an element boundary, decode to strictly fewer elements than the
/// full file. A truncation must never silently equal the full parse.
#[test]
fn truncation_at_every_prefix_never_panics_or_silently_matches() {
    let elements = vec![
        Element::new(tag::PATIENT_NAME, Vr::PN, Value::Str("DOE^JOHN".into())),
        Element::new(tag::ROWS, Vr::US, Value::U16(4)),
        Element::new(tag::COLUMNS, Vr::US, Value::U16(4)),
        Element::new(tag::BITS_ALLOCATED, Vr::US, Value::U16(16)),
    ];
    let data = build_minimal_explicit_vr_le(&elements);
    let full_len = parse(io::Cursor::new(data.clone()))
        .expect("full parse should succeed")
        .len();

    for i in 0..data.len() {
        match parse(io::Cursor::new(data[..i].to_vec())) {
            Ok(ds) => assert!(
                ds.len() < full_len,
                "prefix {i} parsed Ok with the full element count ({}); \
                 a truncation silently matched the full parse",
                ds.len()
            ),
            Err(e) => assert!(
                matches!(
                    e,
                    DicosError::Truncated { .. }
                        | DicosError::Io(_)
                        | DicosError::BadPreamble { .. }
                ),
                "prefix {i} produced an unexpected error: {e:?}"
            ),
        }
    }

    assert_eq!(
        parse(io::Cursor::new(data)).unwrap().len(),
        full_len,
        "the full-length file must decode to the full element set"
    );
}

// -----------------------------------------------------------------------
// Parse warnings
// -----------------------------------------------------------------------

#[test]
fn parse_with_warnings_reports_odd_pixel_length() {
    // 2x2 image with an odd (5-byte) native pixel payload.
    let elements = vec![
        Element::new(tag::ROWS, Vr::US, Value::U16(2)),
        Element::new(tag::COLUMNS, Vr::US, Value::U16(2)),
        Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(vec![1, 0, 2, 0, 3])),
    ];
    let data = build_minimal_explicit_vr_le(&elements);

    let (ds, warnings) = parse_with_warnings(io::Cursor::new(data)).expect("parse should succeed");
    assert!(ds.pixel_data().is_some());
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ParseWarning::OddPixelDataLength { length: 5 })),
        "expected OddPixelDataLength warning, got: {warnings:?}"
    );
}

#[test]
fn parse_with_warnings_reports_pixel_count_mismatch() {
    // 2x2 image (4 pixels expected) but only 2 pixels of data.
    let elements = vec![
        Element::new(tag::ROWS, Vr::US, Value::U16(2)),
        Element::new(tag::COLUMNS, Vr::US, Value::U16(2)),
        Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(vec![1, 0, 2, 0])),
    ];
    let data = build_minimal_explicit_vr_le(&elements);

    let (_ds, warnings) = parse_with_warnings(io::Cursor::new(data)).expect("parse should succeed");
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            ParseWarning::PixelCountMismatch {
                actual: 2,
                expected: 4
            }
        )),
        "expected PixelCountMismatch warning, got: {warnings:?}"
    );
}

#[test]
fn parse_without_warnings_still_succeeds_on_clean_file() {
    let elements = vec![
        Element::new(tag::ROWS, Vr::US, Value::U16(2)),
        Element::new(tag::COLUMNS, Vr::US, Value::U16(2)),
        Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::Bytes(vec![1, 0, 2, 0, 3, 0, 4, 0]),
        ),
    ];
    let data = build_minimal_explicit_vr_le(&elements);

    let (ds, warnings) = parse_with_warnings(io::Cursor::new(data)).expect("parse should succeed");
    assert!(warnings.is_empty(), "clean file should have no warnings");
    let pd = ds.pixel_data().expect("pixel data present");
    assert_eq!(pd.native_frames().unwrap()[0], vec![1u16, 2, 3, 4]);
}
