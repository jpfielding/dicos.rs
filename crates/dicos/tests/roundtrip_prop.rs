//! Property-based round-trip tests for the DICOS writer/reader pair.
//!
//! # The round-trip property we assert, and why
//!
//! A naive `parse(write(ds)) == ds` does **not** hold, because both sides apply
//! deliberate, documented normalizations:
//!
//! * The **writer** always injects a `TransferSyntaxUID` (0002,0010) when absent
//!   and always (re)writes a `FileMetaInformationGroupLength` (0002,0000). So the
//!   re-parsed dataset gains elements the original never had.
//! * The **reader** trims trailing padding (` `/`\0`) from string values, splits
//!   backslash-delimited strings into `Value::Strings`, decodes `US`/`UL` bytes
//!   into `Value::U16`/`U32`, and rewrites raw native `PixelData` bytes into
//!   `PixelData::Native { frames }` using the image geometry.
//!
//! The strongest property that holds across *all* well-formed datasets is that
//! the normalized on-disk/in-memory form is a **fixed point** of the
//! write/parse cycle. We assert it three complementary ways:
//!
//! 1. `roundtrip_preserves_every_element` — every element placed into a dataset
//!    (built in already-normalized form) comes back byte-for-byte equal after one
//!    `write` + `parse`. This is the direct "your data survives" guarantee.
//! 2. `parsed_dataset_is_a_fixed_point` — `parse(write(ds))` equals
//!    `parse(write(parse(write(ds))))`: once normalized, further cycles change
//!    nothing (dataset-level idempotence).
//! 3. `rewrite_is_byte_stable` — re-encoding a normalized dataset reproduces the
//!    exact same bytes (wire-level idempotence: reader and writer agree on the
//!    canonical encoding).
//! 4. `native_pixel_values_are_preserved` — a dedicated guard on the trickiest
//!    normalization (multi-frame native pixel reconstruction from geometry).
//!
//! Each property runs at most 100 cases to keep CI fast.

use dicos::{parse, tag, write, Dataset, Element, PixelData, Value, Vr};

use proptest::collection;
use proptest::option;
use proptest::prelude::*;
use std::io::Cursor;

/// Writes `ds` to an in-memory buffer and parses it back.
fn roundtrip(ds: &Dataset) -> Dataset {
    let mut buf = Vec::new();
    write(&mut buf, ds).expect("write should succeed");
    parse(Cursor::new(buf)).expect("parse should succeed")
}

/// Encodes `ds` to bytes.
fn to_bytes(ds: &Dataset) -> Vec<u8> {
    let mut buf = Vec::new();
    write(&mut buf, ds).expect("write should succeed");
    buf
}

// --- Value generators, all producing already-normalized forms ---------------

/// Text with no backslash and no leading/trailing whitespace, so it survives
/// the reader's trim/split normalization unchanged.
fn arb_text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z0-9^._-]{1,8}").expect("valid regex")
}

/// A dotted numeric UID (UI charset).
fn arb_uid() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[0-9]{1,3}(\\.[0-9]{1,3}){1,4}").expect("valid regex")
}

/// A decimal string (DS charset), compared as text, not as a number.
fn arb_decimal() -> impl Strategy<Value = String> {
    proptest::string::string_regex("-?[0-9]{1,4}(\\.[0-9]{1,3})?").expect("valid regex")
}

// --- Element and dataset generators -----------------------------------------

prop_compose! {
    /// A grab-bag of scalar elements, each independently present or absent, on
    /// distinct known tags with matching VRs.
    fn arb_scalars()(
        pn in option::of(arb_text()),
        pid in option::of(arb_text()),
        desc in option::of(arb_text()),
        sop in option::of(arb_uid()),
        intercept in option::of(arb_decimal()),
        slope in option::of(arb_decimal()),
        bits in option::of(any::<u16>()),
        alarms in option::of(any::<u32>()),
        image_type in option::of(collection::vec(arb_text(), 2..=3)),
    ) -> Vec<Element> {
        let mut v = Vec::new();
        if let Some(x) = pn {
            v.push(Element::new(tag::PATIENT_NAME, Vr::PN, Value::Str(x)));
        }
        if let Some(x) = pid {
            v.push(Element::new(tag::PATIENT_ID, Vr::LO, Value::Str(x)));
        }
        if let Some(x) = desc {
            v.push(Element::new(tag::STUDY_DESCRIPTION, Vr::LO, Value::Str(x)));
        }
        if let Some(x) = sop {
            v.push(Element::new(tag::SOP_CLASS_UID, Vr::UI, Value::Str(x)));
        }
        if let Some(x) = intercept {
            v.push(Element::new(tag::RESCALE_INTERCEPT, Vr::DS, Value::Str(x)));
        }
        if let Some(x) = slope {
            v.push(Element::new(tag::RESCALE_SLOPE, Vr::DS, Value::Str(x)));
        }
        if let Some(x) = bits {
            v.push(Element::new(tag::BITS_ALLOCATED, Vr::US, Value::U16(x)));
        }
        if let Some(x) = alarms {
            v.push(Element::new(tag::NUMBER_OF_ALARM_OBJECTS, Vr::UL, Value::U32(x)));
        }
        if let Some(items) = image_type {
            // 2+ values guarantee a backslash so the reader keeps `Strings`.
            v.push(Element::new(tag::IMAGE_TYPE, Vr::CS, Value::Strings(items)));
        }
        v
    }
}

prop_compose! {
    /// A small native pixel volume whose geometry (rows, cols, frames) is
    /// self-consistent with the pixel count, so the reader reconstructs the
    /// exact same frames.
    fn arb_pixels()
        (rows in 1u16..=4u16, cols in 1u16..=4u16, frames in 1usize..=2usize)
        (
            pixels in collection::vec(
                any::<u16>(),
                (rows as usize * cols as usize * frames)..=(rows as usize * cols as usize * frames),
            ),
            rows in Just(rows),
            cols in Just(cols),
            frames in Just(frames),
        ) -> (u16, u16, usize, Vec<u16>)
    {
        (rows, cols, frames, pixels)
    }
}

prop_compose! {
    /// A full dataset: scalars, optional native pixel data, and an optional
    /// one-level sequence of 1-2 scalar-only items.
    fn arb_dataset()(
        scalars in arb_scalars(),
        pixels in option::of(arb_pixels()),
        seq_items in option::of(collection::vec(arb_scalars(), 1..=2)),
    ) -> Dataset {
        let mut ds = Dataset::new();
        for elem in scalars {
            ds.insert(elem);
        }

        if let Some((rows, cols, frames, px)) = pixels {
            ds.put_u16(tag::ROWS, Vr::US, rows);
            ds.put_u16(tag::COLUMNS, Vr::US, cols);
            if frames > 1 {
                ds.put_string(tag::NUMBER_OF_FRAMES, Vr::IS, frames.to_string());
            }
            let frame_pixels = rows as usize * cols as usize;
            let frame_vecs: Vec<Vec<u16>> =
                px.chunks(frame_pixels).map(<[u16]>::to_vec).collect();
            ds.insert(Element::new(
                tag::PIXEL_DATA,
                Vr::OW,
                Value::PixelData(PixelData::native(frame_vecs)),
            ));
        }

        if let Some(items) = seq_items {
            let item_datasets: Vec<Dataset> = items
                .into_iter()
                .map(|elements| {
                    let mut item = Dataset::new();
                    for elem in elements {
                        item.insert(elem);
                    }
                    item
                })
                .collect();
            ds.insert(Element::new(
                tag::REFERENCED_IMAGE_SEQUENCE,
                Vr::SQ,
                Value::Sequence(item_datasets),
            ));
        }

        ds
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Every element placed into a normalized dataset survives one write+parse
    /// byte-for-byte (the re-parse may add File Meta elements, which we ignore).
    #[test]
    fn roundtrip_preserves_every_element(ds in arb_dataset()) {
        let rt = roundtrip(&ds);
        for elem in ds.iter() {
            prop_assert_eq!(
                rt.get(elem.tag),
                Some(elem),
                "element {} did not survive the round trip",
                elem.tag
            );
        }
    }

    /// `parse(write(·))` is idempotent: once normalized, another cycle is a
    /// no-op at the dataset level.
    #[test]
    fn parsed_dataset_is_a_fixed_point(ds in arb_dataset()) {
        let once = roundtrip(&ds);
        let twice = roundtrip(&once);
        prop_assert_eq!(once, twice);
    }

    /// Re-encoding a normalized dataset reproduces identical bytes: the reader
    /// and writer agree on one canonical wire form.
    #[test]
    fn rewrite_is_byte_stable(ds in arb_dataset()) {
        let normalized = roundtrip(&ds);
        let bytes_a = to_bytes(&normalized);
        let reparsed = parse(Cursor::new(bytes_a.clone())).expect("parse should succeed");
        let bytes_b = to_bytes(&reparsed);
        prop_assert_eq!(bytes_a, bytes_b);
    }

    /// The multi-frame native pixel path reconstructs the exact same frames.
    #[test]
    fn native_pixel_values_are_preserved((rows, cols, frames, px) in arb_pixels()) {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, rows);
        ds.put_u16(tag::COLUMNS, Vr::US, cols);
        if frames > 1 {
            ds.put_string(tag::NUMBER_OF_FRAMES, Vr::IS, frames.to_string());
        }
        let frame_pixels = rows as usize * cols as usize;
        let expected: Vec<Vec<u16>> = px.chunks(frame_pixels).map(<[u16]>::to_vec).collect();
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::PixelData(PixelData::native(expected.clone())),
        ));

        let rt = roundtrip(&ds);
        let pd = rt.pixel_data().expect("pixel data should be present");
        let got = pd.native_frames().expect("should be native");
        prop_assert_eq!(got, expected.as_slice());
    }
}
