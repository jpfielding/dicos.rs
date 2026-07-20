//! DICOS dataset extraction and raw-value parsing.
//!
//! Pulls a [`Volume`](super::Volume) out of a parsed DICOS dataset (or a
//! directory of them), plus the generic DICOS value-parsing helpers used
//! along the way (also reused by [`super::threats`] for ROI attributes).

use dicos::error::DicosError;
use dicos::reader;
use dicos::tag;
use dicos::types::{Dataset, PixelData, Value};

use super::threats::parse_threats;
use super::Volume;

pub(super) fn parse_text(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => {
            let trimmed = s.trim().trim_matches('\0');
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Strings(values) => values
            .first()
            .and_then(|s| parse_text(&Value::Str(s.clone()))),
        Value::Bytes(raw) => {
            if raw.is_empty() {
                return None;
            }
            if let Ok(s) = std::str::from_utf8(raw) {
                let trimmed = s.trim().trim_matches('\0');
                let printable = trimmed.chars().all(|c| !c.is_control());
                if !trimmed.is_empty() && printable {
                    return Some(trimmed.to_string());
                }
            }
            if raw.len() == 2 {
                return Some(u16::from_le_bytes([raw[0], raw[1]]).to_string());
            }
            if raw.len() == 4 {
                return Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]).to_string());
            }
            None
        }
        Value::U16(v) => Some(v.to_string()),
        Value::U32(v) => Some(v.to_string()),
        Value::I16(v) => Some(v.to_string()),
        Value::I32(v) => Some(v.to_string()),
        Value::F32(v) => Some(v.to_string()),
        Value::F64(v) => Some(v.to_string()),
        _ => None,
    }
}

/// Try to parse raw bytes as a textual numeric list (backslash/comma/space separated).
fn parse_numeric_text(raw: &[u8]) -> Option<Vec<f64>> {
    let s = std::str::from_utf8(raw).ok()?;
    let parsed: Vec<f64> = s
        .trim()
        .trim_matches('\0')
        .split(['\\', ',', ';', ' '])
        .filter_map(|part| {
            let p = part.trim();
            if p.is_empty() {
                None
            } else {
                p.parse::<f64>().ok()
            }
        })
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

/// Try to parse raw bytes as little-endian f32 triples/values.
fn parse_le_f32s(raw: &[u8]) -> Option<Vec<f64>> {
    if raw.len() % 4 != 0 {
        return None;
    }
    let values: Vec<f64> = raw
        .chunks_exact(4)
        .map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        .collect();
    if values.iter().all(|v| v.is_finite()) {
        Some(values)
    } else {
        None
    }
}

/// Parse raw bytes as little-endian u16 values.
fn parse_le_u16s(raw: &[u8]) -> Vec<f64> {
    if raw.len() % 2 != 0 {
        return Vec::new();
    }
    raw.chunks_exact(2)
        .map(|c| f64::from(u16::from_le_bytes([c[0], c[1]])))
        .collect()
}

pub(super) fn parse_numbers(value: &Value) -> Vec<f64> {
    match value {
        Value::Str(s) => s
            .trim()
            .split('\\')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect(),
        Value::Strings(values) => values
            .iter()
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect(),
        Value::U16(v) => vec![f64::from(*v)],
        Value::U16s(values) => values.iter().map(|v| f64::from(*v)).collect(),
        Value::U32(v) => vec![f64::from(*v)],
        Value::I16(v) => vec![f64::from(*v)],
        Value::I32(v) => vec![f64::from(*v)],
        Value::F32(v) => vec![f64::from(*v)],
        Value::F64(v) => vec![*v],
        Value::F32s(values) => values.iter().map(|v| f64::from(*v)).collect(),
        Value::F64s(values) => values.clone(),
        Value::Bytes(raw) => {
            if raw.is_empty() {
                return Vec::new();
            }
            // Precedence: text > LE f32 > LE u16
            if let Some(v) = parse_numeric_text(raw) {
                return v;
            }
            if let Some(v) = parse_le_f32s(raw) {
                return v;
            }
            parse_le_u16s(raw)
        }
        _ => Vec::new(),
    }
}

/// Load a DICOS file or directory of DICOS files and extract the volume.
///
/// If `path` is a file, loads that single file.
/// If `path` is a directory, scans for `.dcs` and `.dcm` files, sorts them
/// by name, and stacks the frames from each file into a single 3D volume.
///
/// Catches panics from codec code to prevent aborting the application.
pub fn load_dicos_path(path: &std::path::Path) -> Result<Volume, DicosError> {
    let path = path.to_path_buf();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if path.is_dir() {
            load_dicos_directory(&path)
        } else {
            load_dicos_volume(&path)
        }
    })) {
        Ok(result) => result,
        Err(_) => Err(DicosError::InvalidFile(format!(
            "codec panicked while loading {}",
            path.display()
        ))),
    }
}

/// Load a single DICOS file and extract the volume.
pub fn load_dicos_volume(path: &std::path::Path) -> Result<Volume, DicosError> {
    let file = std::fs::File::open(path).map_err(DicosError::Io)?;
    let reader = std::io::BufReader::new(file);
    let dataset = reader::parse(reader)?;

    volume_from_dataset(&dataset)
}

/// Extract a sort key from a dataset for spatial slice ordering.
///
/// Priority:
/// 1. `ImagePositionPatient` Z coordinate (tag 0020,0032) — most accurate.
/// 2. `SliceLocation` (tag 0020,1041).
/// 3. `InstanceNumber` (tag 0020,0013).
/// 4. `filename_fallback` — position in alphabetically sorted file list.
fn slice_sort_key(ds: &Dataset, filename_fallback: f64) -> f64 {
    // Try ImagePositionPatient Z component (backslash-separated DS: "x\y\z").
    if let Some(s) = ds.get_string(tag::IMAGE_POSITION_PATIENT) {
        let parts: Vec<&str> = s.trim().split('\\').collect();
        if parts.len() >= 3 {
            if let Ok(z) = parts[2].trim().parse::<f64>() {
                return z;
            }
        }
    }
    // Try SliceLocation.
    if let Some(s) = ds.get_string(tag::SLICE_LOCATION) {
        if let Ok(v) = s.trim().parse::<f64>() {
            return v;
        }
    }
    // Try InstanceNumber.
    if let Some(n) = ds.get_u16(tag::INSTANCE_NUMBER) {
        return f64::from(n);
    }
    filename_fallback
}

/// Load a directory of DICOS files as a single volume.
///
/// Each file contributes one or more slices. Slices are ordered by DICOM
/// spatial metadata: `ImagePositionPatient` Z coordinate (preferred), then
/// `SliceLocation`, then `InstanceNumber`, falling back to filename order
/// when none of those tags are present.
fn load_dicos_directory(dir: &std::path::Path) -> Result<Volume, DicosError> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(DicosError::Io)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if ext == "dcs" || ext == "dcm" {
                    return Some(path);
                }
            }
            None
        })
        .collect();

    // Sort alphabetically first so filename_fallback indices are stable.
    files.sort();

    if files.is_empty() {
        return Err(DicosError::InvalidFile(format!(
            "No .dcs or .dcm files found in {}",
            dir.display()
        )));
    }

    log::info!("Loading {} DICOS files from {}", files.len(), dir.display());

    // Parse each file once, extract sort key, keep dataset for volume extraction.
    let mut entries: Vec<(f64, std::path::PathBuf, Dataset)> = Vec::with_capacity(files.len());
    for (idx, path) in files.into_iter().enumerate() {
        let file = match std::fs::File::open(&path).map_err(DicosError::Io) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("Skipping {}: {e}", path.display());
                continue;
            }
        };
        let reader = std::io::BufReader::new(file);
        match dicos::reader::parse(reader) {
            Ok(ds) => {
                let key = slice_sort_key(&ds, idx as f64);
                entries.push((key, path, ds));
            }
            Err(e) => {
                log::warn!("Skipping {}: {e}", path.display());
            }
        }
    }

    if entries.is_empty() {
        return Err(DicosError::InvalidFile(
            "No valid DICOS files could be parsed".into(),
        ));
    }

    // Sort by spatial key; use path as tiebreaker for determinism.
    entries.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    // Extract the first volume to get dimensions and shared metadata.
    let (_, first_path, first_ds) = entries.remove(0);
    let first = volume_from_dataset(&first_ds)?;
    let cols = first.dim_x;
    let rows = first.dim_y;

    let mut all_pixels = first.data;
    let mut total_slices = first.dim_z;

    // Extract remaining volumes, appending their pixel data.
    for (_, path, ds) in entries {
        match volume_from_dataset(&ds) {
            Ok(vol) => {
                if vol.dim_x != cols || vol.dim_y != rows {
                    log::warn!(
                        "Skipping {}: dimensions {}x{} don't match expected {}x{}",
                        path.display(),
                        vol.dim_x,
                        vol.dim_y,
                        cols,
                        rows
                    );
                    continue;
                }
                total_slices += vol.dim_z;
                all_pixels.extend_from_slice(&vol.data);
            }
            Err(e) => {
                log::warn!("Skipping {}: {e}", path.display());
            }
        }
    }

    log::info!(
        "Assembled volume: {}x{}x{} from {} files (first: {})",
        cols,
        rows,
        total_slices,
        total_slices,
        first_path.display()
    );

    let mut vol = Volume {
        data: all_pixels,
        dim_x: cols,
        dim_y: rows,
        dim_z: total_slices,
        window_center: first.window_center,
        window_width: first.window_width,
        rescale_intercept: first.rescale_intercept,
        modality: first.modality,
        voxel_spacing: first.voxel_spacing,
        threats: first.threats,
    };

    if vol.window_width >= 65000.0 {
        vol.compute_window_from_data();
    }

    Ok(vol)
}

/// Extract a volume from a parsed DICOS dataset.
pub fn volume_from_dataset(ds: &Dataset) -> Result<Volume, DicosError> {
    // Rows/Columns have no DICOM-defined default: an absent tag is a genuine
    // MissingAttribute, distinct from a present-but-zero value.
    let cols = ds.columns().ok_or(DicosError::MissingAttribute {
        group: tag::COLUMNS.group,
        element: tag::COLUMNS.element,
    })? as usize;
    let rows = ds.rows().ok_or(DicosError::MissingAttribute {
        group: tag::ROWS.group,
        element: tag::ROWS.element,
    })? as usize;
    let num_frames = ds.number_of_frames() as usize;

    if cols == 0 || rows == 0 {
        let (group, element) = if rows == 0 {
            (tag::ROWS.group, tag::ROWS.element)
        } else {
            (tag::COLUMNS.group, tag::COLUMNS.element)
        };
        return Err(DicosError::InvalidValue {
            group,
            element,
            reason: "Rows and Columns must be non-zero".into(),
        });
    }

    let expected_pixels = cols
        .checked_mul(rows)
        .and_then(|v| v.checked_mul(num_frames))
        .ok_or_else(|| {
            DicosError::InvalidFile(format!(
                "pixel count overflow: {cols} x {rows} x {num_frames}"
            ))
        })?;

    // Get window center/width. If metadata provides multiple presets, we'll
    // later fall back to a data-derived window to keep viewer defaults stable.
    let wc_values = ds.get_strs(tag::WINDOW_CENTER);
    let ww_values = ds.get_strs(tag::WINDOW_WIDTH);
    let has_multi_window_values = wc_values.as_ref().is_some_and(|v| v.len() > 1)
        || ww_values.as_ref().is_some_and(|v| v.len() > 1);

    let window_center = wc_values
        .as_ref()
        .and_then(|vals| vals.first().and_then(|s| s.trim().parse::<f64>().ok()))
        .or_else(|| {
            ds.get_string(tag::WINDOW_CENTER)
                .and_then(|s| s.trim().parse::<f64>().ok())
        })
        .unwrap_or(32768.0);

    let window_width = ww_values
        .as_ref()
        .and_then(|vals| vals.first().and_then(|s| s.trim().parse::<f64>().ok()))
        .or_else(|| {
            ds.get_string(tag::WINDOW_WIDTH)
                .and_then(|s| s.trim().parse::<f64>().ok())
        })
        .unwrap_or(65536.0);

    let rescale_intercept = ds
        .get_string(tag::RESCALE_INTERCEPT)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    let modality = ds.modality().to_string();

    // Extract pixel data.
    let pixel_data = ds
        .get(tag::PIXEL_DATA)
        .ok_or_else(|| DicosError::InvalidFile("Missing PixelData (7FE0,0010)".into()))?;

    let mut all_pixels = Vec::with_capacity(expected_pixels);

    match &pixel_data.value {
        Value::PixelData(PixelData::Native { frames }) => {
            for frame in frames {
                all_pixels.extend_from_slice(frame);
            }
        }
        Value::PixelData(PixelData::Encapsulated { frames, .. }) => {
            let ts = ds.transfer_syntax();
            for frame in frames {
                let decoded =
                    dicos::codec_registry::decode_frame(frame, cols as u32, rows as u32, ts.uid())?;
                all_pixels.extend_from_slice(&decoded);
            }
        }
        Value::Bytes(raw) => {
            if raw.len() % 2 != 0 {
                return Err(DicosError::InvalidFile(
                    "native pixel data has odd byte length".into(),
                ));
            }
            for chunk in raw.chunks_exact(2) {
                all_pixels.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
        }
        _ => {
            return Err(DicosError::InvalidFile(
                "PixelData has unexpected type".into(),
            ));
        }
    }

    if all_pixels.len() != expected_pixels {
        return Err(DicosError::InvalidFile(format!(
            "pixel count mismatch: expected {} ({}x{}x{}), got {}",
            expected_pixels,
            cols,
            rows,
            num_frames,
            all_pixels.len()
        )));
    }

    let (dim_x, dim_y, dim_z) = if num_frames > 1 {
        (cols, rows, num_frames)
    } else {
        (cols, rows, 1)
    };

    let expected_pixels = dim_x * dim_y * dim_z;
    if all_pixels.len() != expected_pixels {
        return Err(DicosError::InvalidFile(format!(
            "pixel data has {} pixels but {}×{}×{} = {} expected",
            all_pixels.len(),
            dim_x,
            dim_y,
            dim_z,
            expected_pixels,
        )));
    }

    // Parse pixel spacing from either multi-valued DS or backslash-separated DS.
    let mut voxel_spacing = [1.0f64; 3];
    if let Some(parts) = ds.get_strs(tag::PIXEL_SPACING) {
        if parts.len() >= 2 {
            if let Ok(row_sp) = parts[0].trim().parse::<f64>() {
                voxel_spacing[1] = row_sp; // Row spacing -> Y
            }
            if let Ok(col_sp) = parts[1].trim().parse::<f64>() {
                voxel_spacing[0] = col_sp; // Col spacing -> X
            }
        }
    } else if let Some(ps) = ds.get_string(tag::PIXEL_SPACING) {
        let mut parts = ps.trim().split('\\');
        if let Some(row) = parts.next() {
            if let Ok(row_sp) = row.trim().parse::<f64>() {
                voxel_spacing[1] = row_sp;
            }
        }
        if let Some(col) = parts.next() {
            if let Ok(col_sp) = col.trim().parse::<f64>() {
                voxel_spacing[0] = col_sp;
            }
        }
    }
    // Slice thickness → Z spacing.
    if let Some(st) = ds.get_string(tag::SLICE_THICKNESS) {
        if let Ok(z_sp) = st.trim().parse::<f64>() {
            voxel_spacing[2] = z_sp;
        }
    } else if let Some(sbs) = ds.get_string(tag::SPACING_BETWEEN_SLICES) {
        if let Ok(z_sp) = sbs.trim().parse::<f64>() {
            voxel_spacing[2] = z_sp;
        }
    }

    let mut vol = Volume {
        data: all_pixels,
        dim_x,
        dim_y,
        dim_z,
        window_center,
        window_width,
        rescale_intercept,
        modality,
        voxel_spacing,
        threats: parse_threats(ds, [dim_x, dim_y, dim_z]),
    };

    // If window center/width span the full u16 range (likely uninformative
    // defaults), or metadata carries multiple WC/WW presets, recompute from
    // actual pixel data for a stable initial view.
    if window_width >= 65000.0 || has_multi_window_values {
        vol.compute_window_from_data();
    }

    Ok(vol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicos::types::Element;
    use dicos::vr::Vr;

    /// Build a minimal single-pixel dataset with ImagePositionPatient set to the
    /// given Z coordinate.
    fn ds_with_z(z: f32) -> Dataset {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        ds.put_string(
            tag::IMAGE_POSITION_PATIENT,
            Vr::DS,
            format!("0.0\\0.0\\{z}"),
        );
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::Bytes(vec![0u8, 0u8]),
        ));
        ds
    }

    #[test]
    fn slice_sort_key_image_position_patient() {
        let ds = ds_with_z(-50.0);
        let key = slice_sort_key(&ds, 99.0);
        assert!((key - (-50.0)).abs() < 1e-6, "Expected -50.0, got {key}");
    }

    #[test]
    fn slice_sort_key_falls_back_to_slice_location() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        ds.put_string(tag::SLICE_LOCATION, Vr::DS, "12.5");
        let key = slice_sort_key(&ds, 99.0);
        assert!((key - 12.5).abs() < 1e-6, "Expected 12.5, got {key}");
    }

    #[test]
    fn slice_sort_key_falls_back_to_instance_number() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        ds.put_u16(tag::INSTANCE_NUMBER, Vr::IS, 7);
        let key = slice_sort_key(&ds, 99.0);
        assert!((key - 7.0).abs() < 1e-6, "Expected 7.0, got {key}");
    }

    #[test]
    fn slice_sort_key_uses_filename_fallback() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, 1);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        // No spatial tags at all.
        let key = slice_sort_key(&ds, 3.0);
        assert!((key - 3.0).abs() < 1e-6, "Expected 3.0, got {key}");
    }

    #[test]
    fn volume_from_datasets_sorted_by_z() {
        // Build two 1x1 single-slice datasets with Z = 10 and Z = -10.
        // When sorted by ImagePositionPatient Z, Z=-10 should be first (lower Z).
        let ds_neg = ds_with_z(-10.0);
        let ds_pos = ds_with_z(10.0);

        let key_neg = slice_sort_key(&ds_neg, 0.0);
        let key_pos = slice_sort_key(&ds_pos, 1.0);

        assert!(key_neg < key_pos, "Z=-10 should sort before Z=10");
    }

    // -----------------------------------------------------------------------
    // volume_from_dataset edge-case tests
    // -----------------------------------------------------------------------

    #[test]
    fn volume_from_dataset_zero_rows_errors() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 0);
        ds.put_u16(tag::COLUMNS, Vr::US, 4);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::InvalidValue { group, element, .. })
                    if group == tag::ROWS.group && element == tag::ROWS.element
            ),
            "expected InvalidValue for Rows, got {result:?}"
        );
    }

    #[test]
    fn volume_from_dataset_zero_columns_errors() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 4);
        ds.put_u16(tag::COLUMNS, Vr::US, 0);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::InvalidValue { group, element, .. })
                    if group == tag::COLUMNS.group && element == tag::COLUMNS.element
            ),
            "expected InvalidValue for Columns, got {result:?}"
        );
    }

    #[test]
    fn volume_from_dataset_missing_rows_errors() {
        let mut ds = Dataset::new();
        // ROWS omitted entirely -> genuine MissingAttribute, not a zero value.
        ds.put_u16(tag::COLUMNS, Vr::US, 4);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::MissingAttribute { group, element })
                    if group == tag::ROWS.group && element == tag::ROWS.element
            ),
            "expected MissingAttribute for Rows, got {result:?}"
        );
    }

    #[test]
    fn volume_from_dataset_missing_columns_errors() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 4);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::MissingAttribute { group, element })
                    if group == tag::COLUMNS.group && element == tag::COLUMNS.element
            ),
            "expected MissingAttribute for Columns, got {result:?}"
        );
    }

    #[test]
    fn volume_from_dataset_missing_pixel_data_errors() {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 4);
        ds.put_u16(tag::COLUMNS, Vr::US, 4);
        // No PIXEL_DATA inserted.
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(result, Err(dicos::error::DicosError::InvalidFile(_))),
            "expected InvalidFile for missing pixel data, got {result:?}"
        );
    }
}
