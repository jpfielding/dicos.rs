//! Volume data management.
//!
//! Handles loading DICOS files, extracting volume data, computing center of
//! mass, and preparing data for GPU upload (density + pre-computed gradients).

mod dataset;
mod threats;

pub use dataset::{load_dicos_path, load_dicos_volume, volume_from_dataset};
pub use threats::{
    load_threat_sidecars_from_dir, load_threats_from_file, threat_color_for_index, ThreatBox,
};

/// A loaded 3D volume ready for rendering.
#[derive(Debug)]
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
                        let right = if x < dx - 1 {
                            data[idx + 1] as f32
                        } else {
                            center
                        };
                        (right - left) * 0.5
                    } else {
                        (data[idx + 1] as f32 - data[idx - 1] as f32) * 0.5
                    };

                    let gy = if y_is_edge {
                        let below = if y > 0 {
                            data[idx - stride_y] as f32
                        } else {
                            center
                        };
                        let above = if y < dy - 1 {
                            data[idx + stride_y] as f32
                        } else {
                            center
                        };
                        (above - below) * 0.5
                    } else {
                        (data[idx + stride_y] as f32 - data[idx - stride_y] as f32) * 0.5
                    };

                    let gz = if z_is_edge {
                        let back = if z > 0 {
                            data[idx - stride_z] as f32
                        } else {
                            center
                        };
                        let front = if z < dz - 1 {
                            data[idx + stride_z] as f32
                        } else {
                            center
                        };
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
