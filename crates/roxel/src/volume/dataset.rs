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

/// Parse a single DS / numeric tag, including a multi-valued first component.
fn parse_ds_f64(ds: &Dataset, t: dicos::tag::Tag) -> Option<f64> {
    ds.get_strs(t)
        .and_then(|vals| vals.first().and_then(|s| s.trim().parse().ok()))
        .or_else(|| ds.get_string(t).and_then(|s| s.trim().parse().ok()))
        .or_else(|| ds.get_f64(t))
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
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
pub fn load_dicos_volume(path: &std::path::Path) -> Result<Volume, DicosError> {
    let bytes = std::fs::read(path).map_err(DicosError::Io)?;
    load_dicos_bytes(&bytes)
}

/// Parse a single in-memory DICOS file and extract the volume.
pub fn load_dicos_bytes(bytes: &[u8]) -> Result<Volume, DicosError> {
    let dataset = reader::parse(std::io::Cursor::new(bytes))?;
    volume_from_dataset(&dataset)
}

/// Parse a multi-valued DS attribute into floats.
///
/// After a real parse, multi-valued DS is stored as `Value::Strings`, so
/// `get_string` only returns the first component. Use `get_strs` and also
/// split a single backslash-separated string so in-memory unsplit values
/// work too.
#[cfg(not(target_arch = "wasm32"))]
fn ds_numbers(ds: &Dataset, t: tag::Tag) -> Option<Vec<f64>> {
    let parts = ds.get_strs(t)?;
    parts
        .iter()
        .flat_map(|p| p.split('\\'))
        .map(|s| s.trim().parse::<f64>().ok())
        .collect()
}

/// Compute spatial sort keys for a whole stack of slices.
///
/// One strategy is chosen for the entire stack so keys are never a mix of
/// millimetres and ordinals:
/// 1. Every slice has `ImagePositionPatient`: project it onto the slice
///    normal (`row x column` from the first slice's
///    `ImageOrientationPatient` when every slice has one; otherwise +Z).
///    This orders axial, sagittal, coronal and oblique stacks alike.
/// 2. Every slice has `SliceLocation`.
/// 3. Every slice has `InstanceNumber`.
/// 4. Otherwise the input (alphabetical filename) order.
#[cfg(not(target_arch = "wasm32"))]
fn slice_sort_keys(datasets: &[&Dataset]) -> Vec<f64> {
    let positions: Option<Vec<[f64; 3]>> = datasets
        .iter()
        .map(|ds| {
            let v = ds_numbers(ds, tag::IMAGE_POSITION_PATIENT)?;
            (v.len() >= 3).then(|| [v[0], v[1], v[2]])
        })
        .collect();
    if let Some(positions) = positions {
        let orientations: Option<Vec<Vec<f64>>> = datasets
            .iter()
            .map(|ds| ds_numbers(ds, tag::IMAGE_ORIENTATION_PATIENT).filter(|v| v.len() >= 6))
            .collect();
        let normal = orientations
            .and_then(|o| o.into_iter().next())
            .map(|o| {
                let (r, c) = ([o[0], o[1], o[2]], [o[3], o[4], o[5]]);
                [
                    r[1] * c[2] - r[2] * c[1],
                    r[2] * c[0] - r[0] * c[2],
                    r[0] * c[1] - r[1] * c[0],
                ]
            })
            .filter(|n| n.iter().any(|v| v.abs() > 1e-9))
            .unwrap_or([0.0, 0.0, 1.0]);
        return positions
            .iter()
            .map(|p| p[0] * normal[0] + p[1] * normal[1] + p[2] * normal[2])
            .collect();
    }

    let locations: Option<Vec<f64>> = datasets
        .iter()
        .map(|ds| {
            ds.get_string(tag::SLICE_LOCATION)?
                .trim()
                .parse::<f64>()
                .ok()
        })
        .collect();
    if let Some(locations) = locations {
        return locations;
    }

    let instances: Option<Vec<f64>> = datasets
        .iter()
        .map(|ds| ds.get_u16(tag::INSTANCE_NUMBER).map(f64::from))
        .collect();
    if let Some(instances) = instances {
        return instances;
    }

    (0..datasets.len()).map(|i| i as f64).collect()
}

/// Load a directory of DICOS files as a single volume.
///
/// Each file contributes one or more slices. Slices are ordered by DICOM
/// spatial metadata using one strategy for the whole stack; see
/// [`slice_sort_keys`].
#[cfg(not(target_arch = "wasm32"))]
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
    let mut parsed: Vec<(std::path::PathBuf, Dataset)> = Vec::with_capacity(files.len());
    for path in files {
        let file = match std::fs::File::open(&path).map_err(DicosError::Io) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("Skipping {}: {e}", path.display());
                continue;
            }
        };
        let reader = std::io::BufReader::new(file);
        match dicos::reader::parse(reader) {
            Ok(ds) => parsed.push((path, ds)),
            Err(e) => {
                log::warn!("Skipping {}: {e}", path.display());
            }
        }
    }

    if parsed.is_empty() {
        return Err(DicosError::InvalidFile(
            "No valid DICOS files could be parsed".into(),
        ));
    }

    let keys = slice_sort_keys(&parsed.iter().map(|(_, ds)| ds).collect::<Vec<_>>());
    let mut entries: Vec<(f64, std::path::PathBuf, Dataset)> = keys
        .into_iter()
        .zip(parsed)
        .map(|(key, (path, ds))| (key, path, ds))
        .collect();

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
                // One intensity transform applies to the whole volume, so a
                // slice with a different one would be displayed wrongly.
                if vol.rescale_slope != first.rescale_slope
                    || vol.rescale_intercept != first.rescale_intercept
                    || vol.signed_pixels != first.signed_pixels
                {
                    return Err(DicosError::InvalidFile(format!(
                        "{}: RescaleSlope/RescaleIntercept/PixelRepresentation \
                         ({}, {}, signed={}) differ from {} ({}, {}, signed={})",
                        path.display(),
                        vol.rescale_slope,
                        vol.rescale_intercept,
                        vol.signed_pixels,
                        first_path.display(),
                        first.rescale_slope,
                        first.rescale_intercept,
                        first.signed_pixels
                    )));
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
        rescale_slope: first.rescale_slope,
        signed_pixels: first.signed_pixels,
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

    let rescale_intercept = parse_ds_f64(ds, tag::RESCALE_INTERCEPT).unwrap_or(0.0);
    let rescale_slope = parse_ds_f64(ds, tag::RESCALE_SLOPE).unwrap_or(1.0);
    if !rescale_slope.is_finite() {
        return Err(DicosError::InvalidValue {
            group: tag::RESCALE_SLOPE.group,
            element: tag::RESCALE_SLOPE.element,
            reason: format!("RescaleSlope must be a finite number, got {rescale_slope}"),
        });
    }

    let pixel_representation = ds.pixel_representation().unwrap_or(0);
    if pixel_representation != 0 && pixel_representation != 1 {
        return Err(DicosError::InvalidValue {
            group: tag::PIXEL_REPRESENTATION.group,
            element: tag::PIXEL_REPRESENTATION.element,
            reason: format!(
                "PixelRepresentation {pixel_representation} is unsupported (expected 0 unsigned or 1 signed)"
            ),
        });
    }
    let signed_pixels = pixel_representation == 1;
    let bits_allocated = ds.bits_allocated().unwrap_or(16);
    if signed_pixels && bits_allocated != 8 && bits_allocated != 16 {
        return Err(DicosError::InvalidValue {
            group: tag::BITS_ALLOCATED.group,
            element: tag::BITS_ALLOCATED.element,
            reason: format!(
                "signed pixel data with BitsAllocated={bits_allocated} is unsupported (expected 8 or 16)"
            ),
        });
    }

    // Signed samples are sign-extended from BitsAllocated. DICOM does not
    // guarantee the unused high bits carry the sign, so a narrower BitsStored
    // (e.g. 12-bit signed in 16-bit cells) cannot be interpreted correctly
    // here; reject it rather than show wrong densities.
    if signed_pixels {
        if let Some(bits_stored) = ds.get_u16(tag::BITS_STORED) {
            if bits_stored != bits_allocated {
                return Err(DicosError::InvalidValue {
                    group: tag::BITS_STORED.group,
                    element: tag::BITS_STORED.element,
                    reason: format!(
                        "signed pixel data with BitsStored={bits_stored} < BitsAllocated={bits_allocated} is unsupported"
                    ),
                });
            }
        }
    }

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

    // Sign-extend 8-bit signed samples so the rest of the viewer can treat
    // stored bits as i16 (0xFF becomes -1, not 255).
    if signed_pixels && bits_allocated == 8 {
        for sample in &mut all_pixels {
            *sample = *sample as u8 as i8 as i16 as u16;
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
        rescale_slope,
        signed_pixels,
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

    /// Sort key of a single-slice stack (index fallback is 0).
    fn slice_sort_key(ds: &Dataset, _filename_fallback: f64) -> f64 {
        slice_sort_keys(&[ds])[0]
    }

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
        // No spatial tags at all: keys follow input (filename) order.
        let keys = slice_sort_keys(&[&ds, &ds, &ds]);
        assert_eq!(keys, vec![0.0, 1.0, 2.0]);
    }

    /// Sagittal stack: X varies, Z is constant. Projecting IPP onto the
    /// slice normal from ImageOrientationPatient orders by X.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn slice_sort_keys_use_orientation_normal_for_sagittal() {
        let mk = |x: f32| {
            let mut ds = Dataset::new();
            ds.put_string(tag::IMAGE_POSITION_PATIENT, Vr::DS, format!("{x}\\0\\5"));
            // Row direction (0,1,0), column direction (0,0,-1): normal (-1,0,0).
            ds.put_string(tag::IMAGE_ORIENTATION_PATIENT, Vr::DS, "0\\1\\0\\0\\0\\-1");
            ds
        };
        let (a, b, c) = (mk(2.0), mk(1.0), mk(0.0));
        let keys = slice_sort_keys(&[&a, &b, &c]);
        // normal (-1,0,0): keys are -x, so x=2 sorts first, all distinct.
        assert_eq!(keys, vec![-2.0, -1.0, 0.0]);
    }

    /// When only some slices have IPP, mixing mm keys with ordinals is wrong;
    /// the whole stack falls back to InstanceNumber.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn slice_sort_keys_partial_ipp_falls_back_for_whole_stack() {
        let mut a = ds_with_z(-300.0);
        a.put_u16(tag::INSTANCE_NUMBER, Vr::IS, 1);
        let mut b = Dataset::new();
        b.put_u16(tag::INSTANCE_NUMBER, Vr::IS, 2);
        let mut c = ds_with_z(-100.0);
        c.put_u16(tag::INSTANCE_NUMBER, Vr::IS, 3);
        assert_eq!(slice_sort_keys(&[&a, &b, &c]), vec![1.0, 2.0, 3.0]);
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

    /// Review finding 4: sort after a real write/parse, which splits IPP
    /// into Value::Strings so get_string is only the first component.
    #[test]
    fn slice_sort_key_reads_parsed_image_position_patient() {
        fn parse_ds(ds: &Dataset) -> Dataset {
            let mut buf = Vec::new();
            dicos::write(&mut buf, ds).expect("write should succeed");
            dicos::reader::parse(std::io::Cursor::new(buf)).expect("parse should succeed")
        }

        let parsed_neg = parse_ds(&ds_with_z(-10.0));
        let parsed_pos = parse_ds(&ds_with_z(10.0));

        // The accessor mismatch the review described: get_string is "0.0".
        assert_eq!(
            parsed_neg.get_string(tag::IMAGE_POSITION_PATIENT),
            Some("0.0")
        );
        let strs = parsed_neg
            .get_strs(tag::IMAGE_POSITION_PATIENT)
            .expect("IPP should be present");
        assert_eq!(strs.len(), 3);
        assert_eq!(strs[2].trim(), "-10");

        let key_neg = slice_sort_key(&parsed_neg, 0.0);
        let key_pos = slice_sort_key(&parsed_pos, 1.0);
        assert!(
            (key_neg - (-10.0)).abs() < 1e-6,
            "expected Z=-10 after parse, got {key_neg}"
        );
        assert!(
            (key_pos - 10.0).abs() < 1e-6,
            "expected Z=10 after parse, got {key_pos}"
        );
        assert!(
            key_neg < key_pos,
            "Z=-10 should sort before Z=10 after parse"
        );
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

    fn dataset_with_pixels(
        pixels: &[u16],
        bits: u16,
        signed: bool,
        slope: &str,
        intercept: &str,
    ) -> Dataset {
        let mut ds = Dataset::new();
        ds.put_u16(tag::ROWS, Vr::US, 1);
        ds.put_u16(tag::COLUMNS, Vr::US, pixels.len() as u16);
        ds.put_u16(tag::BITS_ALLOCATED, Vr::US, bits);
        ds.put_u16(
            tag::PIXEL_REPRESENTATION,
            Vr::US,
            if signed { 1 } else { 0 },
        );
        ds.put_string(tag::RESCALE_SLOPE, Vr::DS, slope);
        ds.put_string(tag::RESCALE_INTERCEPT, Vr::DS, intercept);
        ds.put_string(tag::MODALITY, Vr::CS, "CT");
        let mut bytes = Vec::new();
        if bits == 8 {
            for &p in pixels {
                bytes.push(p as u8);
            }
            if bytes.len() % 2 == 1 {
                bytes.push(0);
            }
        } else {
            for &p in pixels {
                bytes.extend_from_slice(&p.to_le_bytes());
            }
        }
        ds.insert(Element::new(tag::PIXEL_DATA, Vr::OW, Value::Bytes(bytes)));
        ds
    }

    fn volume_after_parse(ds: &Dataset) -> Volume {
        let mut buf = Vec::new();
        dicos::write(&mut buf, ds).expect("write should succeed");
        load_dicos_bytes(&buf).expect("load should succeed")
    }

    /// Finding 7: a stored -1 (0xFFFF) with PixelRepresentation=1 is -1, not 65535.
    #[test]
    fn signed_pixels_after_parse_are_i16() {
        let ds = dataset_with_pixels(&[(-1i16) as u16], 16, true, "1", "0");
        let vol = volume_after_parse(&ds);
        assert!(vol.signed_pixels);
        assert_eq!(vol.rescale_slope, 1.0);
        assert_eq!(vol.modality_value(vol.data[0]), -1.0);
    }

    /// Finding 7: RescaleSlope is applied with the intercept.
    #[test]
    fn rescale_slope_applied_after_parse() {
        let ds = dataset_with_pixels(&[1000], 16, false, "2", "-1000");
        let vol = volume_after_parse(&ds);
        assert!(!vol.signed_pixels);
        assert_eq!(vol.rescale_slope, 2.0);
        assert_eq!(vol.rescale_intercept, -1000.0);
        assert_eq!(vol.modality_value(vol.data[0]), 1000.0);
    }

    /// Finding 7: 8-bit signed 0xFF is sign-extended to -1.
    /// Uses in-memory Native frames, which is what the loader sees after
    /// a correctly decoded 8-bit parse.
    #[test]
    fn signed_8bit_is_sign_extended() {
        let mut ds = dataset_with_pixels(&[0x00FF], 8, true, "1", "0");
        ds.insert(Element::new(
            tag::PIXEL_DATA,
            Vr::OW,
            Value::PixelData(dicos::types::PixelData::native(vec![vec![255]])),
        ));
        let vol = volume_from_dataset(&ds).expect("8-bit signed volume should load");
        assert!(vol.signed_pixels);
        assert_eq!(vol.data[0], (-1i16) as u16);
        assert_eq!(vol.modality_value(vol.data[0]), -1.0);
    }

    #[test]
    fn unsupported_pixel_representation_is_rejected() {
        let mut ds = dataset_with_pixels(&[1], 16, false, "1", "0");
        ds.put_u16(tag::PIXEL_REPRESENTATION, Vr::US, 2);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::InvalidValue { group, element, .. })
                    if group == tag::PIXEL_REPRESENTATION.group
                        && element == tag::PIXEL_REPRESENTATION.element
            ),
            "expected InvalidValue for PixelRepresentation, got {result:?}"
        );
    }

    /// Signed data whose BitsStored is narrower than BitsAllocated cannot be
    /// sign-extended correctly from the cell width, so it is rejected.
    #[test]
    fn signed_partial_bits_stored_is_rejected() {
        let mut ds = dataset_with_pixels(&[0x0FFF], 16, true, "1", "0");
        ds.put_u16(tag::BITS_STORED, Vr::US, 12);
        let result = volume_from_dataset(&ds);
        assert!(
            matches!(
                result,
                Err(dicos::error::DicosError::InvalidValue { group, element, .. })
                    if group == tag::BITS_STORED.group && element == tag::BITS_STORED.element
            ),
            "expected InvalidValue for BitsStored, got {result:?}"
        );

        // Full-width BitsStored is still accepted.
        ds.put_u16(tag::BITS_STORED, Vr::US, 16);
        assert!(volume_from_dataset(&ds).is_ok());
    }

    /// One intensity transform applies to the assembled volume, so slices
    /// with a different RescaleSlope are rejected instead of mis-displayed.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn directory_with_mixed_rescale_slope_is_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "roxel-mixed-slope-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        for (i, slope) in ["1", "2", "3"].iter().enumerate() {
            let mut ds = dataset_with_pixels(&[10 * (i as u16 + 1)], 16, false, slope, "0");
            ds.put_u16(tag::INSTANCE_NUMBER, Vr::IS, i as u16 + 1);
            let mut buf = Vec::new();
            dicos::write(&mut buf, &ds).expect("write should succeed");
            std::fs::write(dir.join(format!("s{i}.dcm")), buf).expect("write slice");
        }
        let result = load_dicos_directory(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(result, Err(dicos::error::DicosError::InvalidFile(ref m)) if m.contains("RescaleSlope")),
            "expected mixed-slope rejection, got {result:?}"
        );
    }
}
