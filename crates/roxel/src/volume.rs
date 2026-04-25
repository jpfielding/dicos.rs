//! Volume data management.
//!
//! Handles loading DICOS files, extracting volume data, computing center of
//! mass, and preparing data for GPU upload (density + pre-computed gradients).

use dicos::error::DicosError;
use dicos::reader;
use dicos::tag;
use dicos::types::{Dataset, PixelData, Value};

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

/// A loaded 3D volume ready for rendering.
pub struct Volume {
    /// Raw voxel data (uint16, 0-65535).
    pub data: Vec<u16>,
    /// Volume dimensions.
    pub dim_x: usize,
    pub dim_y: usize,
    pub dim_z: usize,
    /// Window/level for CT display.
    pub window_center: f64,
    pub window_width: f64,
    /// DICOM rescale intercept.
    pub rescale_intercept: f64,
    /// Modality string (e.g. "CT", "DX").
    pub modality: String,
    /// Voxel spacing in mm (X, Y, Z). Defaults to (1.0, 1.0, 1.0).
    pub voxel_spacing: [f64; 3],
    /// Parsed threat bounding boxes (if present in metadata).
    pub threats: Vec<ThreatBox>,
}

impl Volume {
    /// Compute the density-weighted center of mass in normalized [0,1] coordinates.
    ///
    /// Single linear pass: computes min/max for threshold, then accumulates
    /// weighted coordinates using flat-index arithmetic to avoid nested loops.
    pub fn center_of_mass(&self) -> [f32; 3] {
        if self.data.is_empty() {
            return [0.5, 0.5, 0.5];
        }

        let mut min_val = u16::MAX;
        let mut max_val = u16::MIN;
        for &v in &self.data {
            min_val = min_val.min(v);
            max_val = max_val.max(v);
        }
        let threshold = min_val as f64 + (max_val as f64 - min_val as f64) * 0.1;

        let dx = self.dim_x;
        let dxy = self.dim_x * self.dim_y;
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_z = 0.0f64;
        let mut total_weight = 0.0f64;

        for (i, &v) in self.data.iter().enumerate() {
            let val = v as f64;
            if val > threshold {
                let z = i / dxy;
                let rem = i % dxy;
                let y = rem / dx;
                let x = rem % dx;
                sum_x += x as f64 * val;
                sum_y += y as f64 * val;
                sum_z += z as f64 * val;
                total_weight += val;
            }
        }

        if total_weight == 0.0 {
            return [0.5, 0.5, 0.5];
        }

        [
            (sum_x / total_weight / self.dim_x as f64) as f32,
            (sum_y / total_weight / self.dim_y as f64) as f32,
            (sum_z / total_weight / self.dim_z as f64) as f32,
        ]
    }

    /// Compute window center/width from the actual pixel data distribution.
    ///
    /// Uses the 1st and 99th percentile to avoid outliers, matching the
    /// Go `CalculateWindowFromData` approach.
    pub fn compute_window_from_data(&mut self) {
        if self.data.is_empty() {
            return;
        }
        let min_val = *self.data.iter().min().unwrap_or(&0) as f64;
        let max_val = *self.data.iter().max().unwrap_or(&0) as f64;
        if max_val <= min_val {
            return;
        }
        // Simple percentile: sample up to 100k voxels for speed.
        let step = (self.data.len() / 100_000).max(1);
        let mut samples: Vec<u16> = self.data.iter().step_by(step).copied().collect();
        samples.sort_unstable();
        let p01 = samples[samples.len() / 100] as f64;
        let p99 = samples[samples.len() * 99 / 100] as f64;
        let width = (p99 - p01).max(1.0);
        self.window_center = p01 + width * 0.5;
        self.window_width = width;
    }

    /// Pack volume data for GPU upload as `RGBA16Unorm`.
    ///
    /// Each voxel becomes 4 `u16` values: `[density, grad_x+0.5, grad_y+0.5, grad_z+0.5]`.
    /// Gradients use central differences, offset by 0.5 for unsigned texture range.
    ///
    /// Uses precomputed strides and minimizes bounds checks by handling
    /// interior voxels (no clamping needed) separately from edges.
    pub fn pack_for_gpu(&self) -> Vec<u16> {
        let dx = self.dim_x;
        let dy = self.dim_y;
        let dz = self.dim_z;
        let total = dx * dy * dz;
        let mut packed = vec![0u16; total * 4];

        let inv_range = 1.0 / 65535.0f32;
        let stride_y = dx;
        let stride_z = dx * dy;
        let data = &self.data;

        for z in 0..dz {
            let z_base = z * stride_z;
            let z_is_edge = z == 0 || z == dz - 1;
            for y in 0..dy {
                let yz_base = z_base + y * stride_y;
                let y_is_edge = y == 0 || y == dy - 1;
                for x in 0..dx {
                    let idx = yz_base + x;
                    let pi = idx * 4;
                    let center = data[idx] as f32;

                    let gx = if x == 0 || x == dx - 1 {
                        let left = if x > 0 { data[idx - 1] as f32 } else { center };
                        let right = if x < dx - 1 { data[idx + 1] as f32 } else { center };
                        (right - left) * 0.5
                    } else {
                        (data[idx + 1] as f32 - data[idx - 1] as f32) * 0.5
                    };

                    let gy = if y_is_edge {
                        let below = if y > 0 { data[idx - stride_y] as f32 } else { center };
                        let above = if y < dy - 1 { data[idx + stride_y] as f32 } else { center };
                        (above - below) * 0.5
                    } else {
                        (data[idx + stride_y] as f32 - data[idx - stride_y] as f32) * 0.5
                    };

                    let gz = if z_is_edge {
                        let back = if z > 0 { data[idx - stride_z] as f32 } else { center };
                        let front = if z < dz - 1 { data[idx + stride_z] as f32 } else { center };
                        (front - back) * 0.5
                    } else {
                        (data[idx + stride_z] as f32 - data[idx - stride_z] as f32) * 0.5
                    };

                    packed[pi] = data[idx];
                    packed[pi + 1] = to_unorm16(gx * inv_range + 0.5);
                    packed[pi + 2] = to_unorm16(gy * inv_range + 0.5);
                    packed[pi + 3] = to_unorm16(gz * inv_range + 0.5);
                }
            }
        }

        packed
    }
}

fn to_unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn parse_text(value: &Value) -> Option<String> {
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
    if parsed.is_empty() { None } else { Some(parsed) }
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
    if values.iter().all(|v| v.is_finite()) { Some(values) } else { None }
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

fn parse_numbers(value: &Value) -> Vec<f64> {
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

fn parse_threats(ds: &Dataset, dims: [usize; 3]) -> Vec<ThreatBox> {
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

/// Load a directory of DICOS files as a single volume.
///
/// Each file contributes one or more slices. Files are sorted by name to
/// ensure consistent slice ordering.
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

    files.sort();

    if files.is_empty() {
        return Err(DicosError::InvalidFile(format!(
            "No .dcs or .dcm files found in {}",
            dir.display()
        )));
    }

    log::info!("Loading {} DICOS files from {}", files.len(), dir.display());

    // Load the first file to get dimensions and metadata.
    let first = load_dicos_volume(&files[0])?;
    let cols = first.dim_x;
    let rows = first.dim_y;

    let mut all_pixels = first.data;
    let mut total_slices = first.dim_z;

    // Load remaining files, appending their pixel data.
    for file in &files[1..] {
        match load_dicos_volume(file) {
            Ok(vol) => {
                if vol.dim_x != cols || vol.dim_y != rows {
                    log::warn!(
                        "Skipping {}: dimensions {}x{} don't match expected {}x{}",
                        file.display(),
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
                log::warn!("Skipping {}: {e}", file.display());
            }
        }
    }

    log::info!(
        "Assembled volume: {}x{}x{} from {} files",
        cols,
        rows,
        total_slices,
        files.len()
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
    let cols = ds.columns() as usize;
    let rows = ds.rows() as usize;
    let num_frames = ds.number_of_frames() as usize;

    if cols == 0 || rows == 0 {
        return Err(DicosError::InvalidFile("Rows or Columns is zero".into()));
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
                let decoded = dicos::codec_registry::decode_frame(
                    frame,
                    cols as u32,
                    rows as u32,
                    ts.uid(),
                )?;
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
            expected_pixels, cols, rows, num_frames, all_pixels.len()
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

    fn f32_triplet_bytes(x: f32, y: f32, z: f32) -> Vec<u8> {
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&z.to_le_bytes());
        out
    }

    fn test_volume(dim: usize) -> Volume {
        Volume {
            data: vec![100; dim * dim * dim],
            dim_x: dim,
            dim_y: dim,
            dim_z: dim,
            window_center: 32768.0,
            window_width: 65536.0,
            rescale_intercept: 0.0,
            modality: "CT".into(),
            voxel_spacing: [1.0, 1.0, 1.0],
            threats: Vec::new(),
        }
    }

    #[test]
    fn center_of_mass_uniform() {
        let vol = test_volume(4);
        let com = vol.center_of_mass();
        assert!((com[0] - 0.5).abs() < 0.1);
        assert!((com[1] - 0.5).abs() < 0.1);
        assert!((com[2] - 0.5).abs() < 0.1);
    }

    #[test]
    fn center_of_mass_empty() {
        let vol = Volume {
            data: vec![],
            dim_x: 0,
            dim_y: 0,
            dim_z: 0,
            window_center: 0.0,
            window_width: 1.0,
            rescale_intercept: 0.0,
            modality: String::new(),
            voxel_spacing: [1.0, 1.0, 1.0],
            threats: Vec::new(),
        };
        let com = vol.center_of_mass();
        assert_eq!(com, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn pack_for_gpu_length() {
        let vol = test_volume(4);
        let packed = vol.pack_for_gpu();
        assert_eq!(packed.len(), 4 * 4 * 4 * 4);
    }

    #[test]
    fn pack_for_gpu_density_range() {
        let vol = test_volume(4);
        let packed = vol.pack_for_gpu();
        let expected = 100u16;
        for i in (0..packed.len()).step_by(4) {
            assert_eq!(packed[i], expected);
        }
    }

    #[test]
    fn gradient_uniform_is_zero() {
        let vol = test_volume(4);
        let packed = vol.pack_for_gpu();
        let idx = (2 * 4 * 4 + 2 * 4 + 2) * 4;
        assert!(packed[idx + 1].abs_diff(32768) <= 1);
        assert!(packed[idx + 2].abs_diff(32768) <= 1);
        assert!(packed[idx + 3].abs_diff(32768) <= 1);
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
}
