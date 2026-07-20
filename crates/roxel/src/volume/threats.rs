//! Threat ROI parsing from DICOS metadata.
//!
//! Extracts `ThreatBox` entries from `ThreatROISequence` / `PTOSequence`
//! metadata embedded in a volume's dataset, assigns stable display colors,
//! and loads threat-report sidecar files that carry ROIs separately from the
//! voxel data.

use dicos::error::DicosError;
use dicos::reader;
use dicos::tag;
use dicos::types::{Dataset, Value};

use super::dataset::{parse_numbers, parse_text};

/// Threat ROI bounding box extracted from DICOS metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreatBox {
    /// Human-readable name for UI labels.
    pub name: String,
    /// Optional confidence score.
    pub confidence: Option<f64>,
    /// Minimum voxel corner (inclusive): `[x, y, z]`.
    pub min: [usize; 3],
    /// Maximum voxel corner (inclusive): `[x, y, z]`.
    pub max: [usize; 3],
    /// RGB color for this threat.
    pub color: [u8; 3],
    /// Whether this threat is currently enabled for display.
    pub enabled: bool,
}

fn parse_corner(item: &Dataset, t: tag::Tag, z_default: usize) -> Option<[usize; 3]> {
    let elem = item.get(t)?;
    let numbers = parse_numbers(&elem.value);
    if numbers.len() < 2 {
        return None;
    }
    let x = numbers[0].round().max(0.0) as usize;
    let y = numbers[1].round().max(0.0) as usize;
    let z = if numbers.len() >= 3 {
        numbers[2].round().max(0.0) as usize
    } else {
        z_default
    };
    Some([x, y, z])
}

fn clamp_bbox(
    mut min: [usize; 3],
    mut max: [usize; 3],
    dims: [usize; 3],
) -> ([usize; 3], [usize; 3]) {
    for i in 0..3 {
        let bound = dims[i].saturating_sub(1);
        min[i] = min[i].min(bound);
        max[i] = max[i].min(bound);
        if min[i] > max[i] {
            std::mem::swap(&mut min[i], &mut max[i]);
        }
    }
    (min, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

fn threat_color(index: usize) -> [u8; 3] {
    // Golden-ratio hue stepping gives stable, well-separated colors.
    let hue = ((index as f32 * 0.618_034).fract()) * 360.0;
    hsv_to_rgb(hue, 0.82, 0.98)
}

pub fn threat_color_for_index(index: usize) -> [u8; 3] {
    threat_color(index)
}

fn first_number(item: &Dataset, t: tag::Tag) -> Option<f64> {
    let elem = item.get(t)?;
    parse_numbers(&elem.value).into_iter().next()
}

fn text_or_empty(item: &Dataset, t: tag::Tag) -> String {
    item.get(t)
        .and_then(|elem| parse_text(&elem.value))
        .unwrap_or_default()
}

fn build_threat(
    item: &Dataset,
    bbox_item: &Dataset,
    dims: [usize; 3],
    fallback_idx: usize,
) -> Option<ThreatBox> {
    let top_left = parse_corner(bbox_item, tag::BOUNDING_BOX_TOP_LEFT, 0)?;
    let bottom_right = parse_corner(
        bbox_item,
        tag::BOUNDING_BOX_BOTTOM_RIGHT,
        dims[2].saturating_sub(1),
    )?;
    let (min, max) = clamp_bbox(top_left, bottom_right, dims);

    let id = text_or_empty(item, tag::POTENTIAL_THREAT_OBJECT_ID);
    let category = text_or_empty(item, tag::THREAT_CATEGORY_DESCRIPTION);
    let name = if !id.is_empty() && !category.is_empty() {
        format!("{id} ({category})")
    } else if !id.is_empty() {
        id
    } else if !category.is_empty() {
        category
    } else {
        format!("Threat {}", fallback_idx + 1)
    };

    let confidence = first_number(item, tag::THREAT_CONFIDENCE_SCORE)
        .or_else(|| first_number(item, tag::ATD_ASSESSMENT_PROBABILITY));

    Some(ThreatBox {
        name,
        confidence,
        min,
        max,
        color: [0, 0, 0],
        enabled: true,
    })
}

fn recolor_threats(threats: &mut [ThreatBox]) {
    for (i, threat) in threats.iter_mut().enumerate() {
        threat.color = threat_color(i);
    }
}

pub(super) fn parse_threats(ds: &Dataset, dims: [usize; 3]) -> Vec<ThreatBox> {
    let mut threats = Vec::new();

    if let Some(elem) = ds.get(tag::THREAT_ROI_SEQUENCE) {
        if let Value::Sequence(items) = &elem.value {
            for item in items {
                if let Some(threat) = build_threat(item, item, dims, threats.len()) {
                    threats.push(threat);
                }
            }
        }
    }

    // Threat detection report files commonly encode ROIs in PTOSequence /
    // PTORepresentationSequence instead of ThreatROISequence.
    if let Some(elem) = ds.get(tag::PTO_SEQUENCE) {
        if let Value::Sequence(items) = &elem.value {
            for item in items {
                let mut parsed_representation = false;
                if let Some(repr) = item.get(tag::PTO_REPRESENTATION_SEQUENCE) {
                    if let Value::Sequence(reps) = &repr.value {
                        for rep in reps {
                            if let Some(threat) = build_threat(item, rep, dims, threats.len()) {
                                threats.push(threat);
                                parsed_representation = true;
                            }
                        }
                    }
                }

                if !parsed_representation {
                    if let Some(threat) = build_threat(item, item, dims, threats.len()) {
                        threats.push(threat);
                    }
                }
            }
        }
    }

    recolor_threats(&mut threats);
    threats
}

/// Parse threat boxes from a DICOS file without requiring pixel data.
pub fn load_threats_from_file(
    path: &std::path::Path,
    dims: [usize; 3],
) -> Result<Vec<ThreatBox>, DicosError> {
    let file = std::fs::File::open(path).map_err(DicosError::Io)?;
    let reader = std::io::BufReader::new(file);
    let dataset = reader::parse(reader)?;
    Ok(parse_threats(&dataset, dims))
}

/// Load threat report sidecars from a directory.
///
/// Files whose stem includes "threat" (case-insensitive) are parsed and
/// merged. This supports scanner outputs where threat ROIs are stored in
/// separate report objects from the voxel volume.
pub fn load_threat_sidecars_from_dir(dir: &std::path::Path, dims: [usize; 3]) -> Vec<ThreatBox> {
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if !path.is_file() {
                    return None;
                }
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if ext != "dcs" && ext != "dcm" {
                    return None;
                }
                let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
                if !stem.contains("threat") {
                    return None;
                }
                Some(path)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };

    files.sort();

    let mut all = Vec::new();
    for file in files {
        match load_threats_from_file(&file, dims) {
            Ok(mut parsed) => all.append(&mut parsed),
            Err(e) => log::debug!("Skipping threat sidecar {}: {e}", file.display()),
        }
    }

    recolor_threats(&mut all);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::volume_from_dataset;
    use dicos::types::Element;
    use dicos::vr::Vr;

    fn f32_triplet_bytes(x: f32, y: f32, z: f32) -> Vec<u8> {
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&z.to_le_bytes());
        out
    }

    #[test]
    fn parses_threat_bounding_boxes() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 4);
        ds.put_u16(tag::COLUMNS, Vr::US, 4);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");

        let mut bytes = Vec::new();
        for p in 0u16..16u16 {
            bytes.extend_from_slice(&p.to_le_bytes());
        }
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(bytes)));

        let mut roi = Dataset::new();
        roi.put_string(tag::BOUNDING_BOX_TOP_LEFT, Vr::DS, "1\\1\\0");
        roi.put_string(tag::BOUNDING_BOX_BOTTOM_RIGHT, Vr::DS, "3\\2\\0");
        roi.put_string(tag::POTENTIAL_THREAT_OBJECT_ID, Vr::LO, "TH-1");

        ds.insert(Element::new(
            tag::THREAT_ROI_SEQUENCE,
            Vr::SQ,
            Value::Sequence(vec![roi]),
        ));

        let vol = volume_from_dataset(&ds).expect("volume should parse");
        assert_eq!(vol.threats.len(), 1);
        assert_eq!(vol.threats[0].min, [1, 1, 0]);
        assert_eq!(vol.threats[0].max, [3, 2, 0]);
        assert!(vol.threats[0].enabled);
    }

    #[test]
    fn parses_pto_sequence_threat_bytes() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 16);
        ds.put_u16(tag::COLUMNS, Vr::US, 16);
        ds.put_u16(tag::NUMBER_OF_FRAMES, Vr::IS, 16);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");

        let mut bytes = Vec::new();
        for _ in 0..(16 * 16 * 16) {
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(bytes)));

        let mut rep = Dataset::new();
        rep.insert(Element::new(
            tag::BOUNDING_BOX_TOP_LEFT,
            Vr::UN,
            Value::Bytes(f32_triplet_bytes(2.0, 3.0, 4.0)),
        ));
        rep.insert(Element::new(
            tag::OOI_SIZE,
            Vr::UN,
            Value::Bytes(f32_triplet_bytes(8.0, 9.0, 10.0)),
        ));

        let mut pto_item = Dataset::new();
        pto_item.insert(Element::new(
            tag::POTENTIAL_THREAT_OBJECT_ID,
            Vr::UN,
            Value::Bytes(vec![7, 0]),
        ));
        pto_item.insert(Element::new(
            tag::THREAT_CATEGORY_DESCRIPTION,
            Vr::UN,
            Value::Bytes(b"ltr_0 ".to_vec()),
        ));
        pto_item.insert(Element::new(
            tag::PTO_REPRESENTATION_SEQUENCE,
            Vr::SQ,
            Value::Sequence(vec![rep]),
        ));

        ds.insert(Element::new(
            tag::PTO_SEQUENCE,
            Vr::SQ,
            Value::Sequence(vec![pto_item]),
        ));

        let vol = volume_from_dataset(&ds).expect("volume should parse");
        assert_eq!(vol.threats.len(), 1);
        assert_eq!(vol.threats[0].name, "7 (ltr_0)");
        assert_eq!(vol.threats[0].min, [2, 3, 4]);
        assert_eq!(vol.threats[0].max, [8, 9, 10]);
    }

    // -----------------------------------------------------------------------
    // Helper: build a minimal valid in-memory dataset (4x4x1, all zeros).
    // -----------------------------------------------------------------------

    fn minimal_dataset(rows: u16, cols: u16) -> Dataset {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, rows);
        ds.put_u16(tag::COLUMNS, Vr::US, cols);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        let n = (rows as usize) * (cols as usize);
        let mut bytes = vec![0u8; n * 2];
        // make the data non-trivial so compute_window_from_data doesn't short-circuit
        for i in 0..n {
            let v = (i as u16).wrapping_add(1);
            let pair = v.to_le_bytes();
            bytes[i * 2] = pair[0];
            bytes[i * 2 + 1] = pair[1];
        }
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(bytes)));
        ds
    }

    // -----------------------------------------------------------------------
    // load_threat_sidecars_from_dir tests
    // -----------------------------------------------------------------------

    /// Write a minimal DICOS file to `path` using dicos::writer.
    fn write_minimal_dicos(path: &std::path::Path, rows: u16, cols: u16) {
        let ds = minimal_dataset(rows, cols);
        let mut file = std::fs::File::create(path).expect("create test file");
        dicos::writer::write(&mut file, &ds).expect("write test dicos file");
    }

    #[test]
    fn load_threat_sidecars_from_empty_dir() {
        let tmpdir = std::env::temp_dir().join("dicos_test_sidecars_empty");
        std::fs::create_dir_all(&tmpdir).ok();
        // Ensure directory is empty.
        for entry in std::fs::read_dir(&tmpdir).unwrap().flatten() {
            let _ = std::fs::remove_file(entry.path());
        }

        let result = load_threat_sidecars_from_dir(&tmpdir, [4, 4, 1]);
        assert!(
            result.is_empty(),
            "expected no sidecars from empty dir, got {result:?}"
        );

        std::fs::remove_dir_all(&tmpdir).ok();
    }

    #[test]
    fn load_threat_sidecars_ignores_non_dicos_files() {
        let tmpdir = std::env::temp_dir().join("dicos_test_sidecars_nondicos");
        std::fs::create_dir_all(&tmpdir).ok();
        // Place a file that is not .dcs/.dcm.
        std::fs::write(tmpdir.join("threat_report.txt"), b"not dicos").ok();

        let result = load_threat_sidecars_from_dir(&tmpdir, [4, 4, 1]);
        assert!(
            result.is_empty(),
            "expected no sidecars from non-DICOS files, got {result:?}"
        );

        std::fs::remove_dir_all(&tmpdir).ok();
    }

    #[test]
    fn load_threat_sidecars_ignores_files_without_threat_in_stem() {
        let tmpdir = std::env::temp_dir().join("dicos_test_sidecars_nostem");
        std::fs::create_dir_all(&tmpdir).ok();
        // File has .dcs extension but stem doesn't contain "threat".
        let path = tmpdir.join("scan.dcs");
        write_minimal_dicos(&path, 4, 4);

        let result = load_threat_sidecars_from_dir(&tmpdir, [4, 4, 1]);
        assert!(
            result.is_empty(),
            "expected no sidecars for non-threat stem, got {result:?}"
        );

        std::fs::remove_dir_all(&tmpdir).ok();
    }

    #[test]
    fn load_threat_sidecars_returns_empty_for_threat_file_with_no_rois() {
        let tmpdir = std::env::temp_dir().join("dicos_test_sidecars_nothreatdata");
        std::fs::create_dir_all(&tmpdir).ok();
        // File name contains "threat" but dataset has no threat sequences.
        let path = tmpdir.join("threat_report.dcs");
        write_minimal_dicos(&path, 4, 4);

        let result = load_threat_sidecars_from_dir(&tmpdir, [4, 4, 1]);
        // The file is found and parsed, but yields zero ThreatBox entries.
        assert!(
            result.is_empty(),
            "expected no threat boxes from a file with no ROI sequences, got {result:?}"
        );

        std::fs::remove_dir_all(&tmpdir).ok();
    }

    // -----------------------------------------------------------------------
    // merge_unique_threats tests (via pub(crate) in app.rs)
    // -----------------------------------------------------------------------

    fn make_threat(name: &str, min: [usize; 3], max: [usize; 3]) -> ThreatBox {
        ThreatBox {
            name: name.to_string(),
            confidence: None,
            min,
            max,
            color: [255, 0, 0],
            enabled: true,
        }
    }

    #[test]
    fn merge_unique_threats_empty_src_changes_nothing() {
        let mut dst = vec![make_threat("A", [0, 0, 0], [1, 1, 1])];
        let added = crate::app::merge_unique_threats(&mut dst, vec![]);
        assert_eq!(added, 0);
        assert_eq!(dst.len(), 1);
    }

    #[test]
    fn merge_unique_threats_adds_unique_entries() {
        let mut dst = vec![make_threat("A", [0, 0, 0], [1, 1, 1])];
        let src = vec![
            make_threat("B", [2, 2, 2], [3, 3, 3]),
            make_threat("C", [4, 4, 4], [5, 5, 5]),
        ];
        let added = crate::app::merge_unique_threats(&mut dst, src);
        assert_eq!(added, 2);
        assert_eq!(dst.len(), 3);
        assert_eq!(dst[1].name, "B");
        assert_eq!(dst[2].name, "C");
    }

    #[test]
    fn merge_unique_threats_does_not_duplicate_identical_boxes() {
        let t = make_threat("A", [0, 0, 0], [1, 1, 1]);
        let mut dst = vec![t.clone()];
        let src = vec![t];
        let added = crate::app::merge_unique_threats(&mut dst, src);
        assert_eq!(added, 0, "duplicate should not be added");
        assert_eq!(dst.len(), 1);
    }

    #[test]
    fn merge_unique_threats_same_bbox_different_name_is_not_duplicate() {
        let mut dst = vec![make_threat("A", [0, 0, 0], [1, 1, 1])];
        let src = vec![make_threat("B", [0, 0, 0], [1, 1, 1])];
        let added = crate::app::merge_unique_threats(&mut dst, src);
        assert_eq!(added, 1, "different name means not a duplicate");
        assert_eq!(dst.len(), 2);
    }

    #[test]
    fn merge_unique_threats_reassigns_colors() {
        // After merging, all threats should have colors set by threat_color_for_index.
        let mut dst = vec![make_threat("A", [0, 0, 0], [1, 1, 1])];
        let src = vec![make_threat("B", [2, 2, 2], [3, 3, 3])];
        crate::app::merge_unique_threats(&mut dst, src);
        // Color at index 0 should equal threat_color_for_index(0).
        assert_eq!(dst[0].color, threat_color_for_index(0));
        assert_eq!(dst[1].color, threat_color_for_index(1));
    }
}
