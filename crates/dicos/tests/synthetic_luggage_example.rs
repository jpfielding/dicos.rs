use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use dicos::reader;
use dicos::tag;
use dicos::types::PixelData;

fn sample_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../testdata/bag_ct.dcs");
    p
}

fn parse_ds() -> Option<dicos::types::Dataset> {
    let p = sample_path();
    if !p.exists() {
        eprintln!("skipping: {p:?} not found");
        return None;
    }
    let f = File::open(&p).expect("open bag CT sample");
    Some(reader::parse(BufReader::new(f)).expect("parse bag CT sample"))
}

fn first_string(ds: &dicos::types::Dataset, t: dicos::tag::Tag) -> String {
    ds.get_string(t).unwrap_or("").to_string()
}

#[test]
fn bag_ct_sample_is_ct() {
    let Some(ds) = parse_ds() else {
        return;
    };
    assert_eq!(
        first_string(&ds, tag::SOP_CLASS_UID),
        "1.2.840.10008.5.1.4.1.1.2"
    );
    assert_eq!(first_string(&ds, tag::MODALITY), "CT");
    assert_eq!(
        first_string(&ds, tag::TRANSFER_SYNTAX_UID),
        "1.2.840.10008.1.2.1"
    );
    assert!(ds.rows() > 0);
    assert!(ds.columns() > 0);
    assert!(ds.number_of_frames() >= 1);
}

#[test]
fn bag_ct_sample_is_public_safe() {
    let Some(ds) = parse_ds() else {
        return;
    };
    let joined = ds
        .iter()
        .filter_map(|(_, e)| e.value.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    assert!(!joined.contains("clearscan"));
    assert!(!joined.contains("cct1218"));
    assert_eq!(first_string(&ds, tag::PATIENT_ID), "BAG-CT-001");
}

#[test]
fn bag_ct_sample_has_non_uniform_pixel_data() {
    let Some(ds) = parse_ds() else {
        return;
    };

    let rows = ds.rows() as usize;
    let cols = ds.columns() as usize;
    let frames = ds.number_of_frames() as usize;

    // After normalization, pixel data is PixelData::Native, not raw bytes.
    let pd = ds.pixel_data().expect("pixel_data() should return Some");
    let native_frames = match pd {
        PixelData::Native { frames: ref f } => f,
        other => panic!("expected native pixel data, got {other:?}"),
    };

    let total_pixels: usize = native_frames.iter().map(|f| f.len()).sum();
    assert_eq!(total_pixels, rows * cols * frames);

    let voxels: Vec<u16> = native_frames
        .iter()
        .flat_map(|f| f.iter().copied())
        .collect();

    let min = voxels.iter().copied().min().expect("non-empty pixels");
    let max = voxels.iter().copied().max().expect("non-empty pixels");
    assert!(max > min, "expected a non-uniform CT volume");
}
