//! Volume data management.
//!
//! Handles loading DICOS files, extracting volume data, computing center of
//! mass, and preparing data for GPU upload (density + pre-computed gradients).

mod dataset;
mod threats;

pub use dataset::{load_dicos_bytes, volume_from_dataset};
#[cfg(not(target_arch = "wasm32"))]
pub use dataset::{load_dicos_path, load_dicos_volume};
pub use threats::{load_threat_sidecars_from_bytes, threat_color_for_index, ThreatBox};
#[cfg(not(target_arch = "wasm32"))]
pub use threats::{load_threat_sidecars_from_dir, load_threats_from_file};

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
    /// DICOM rescale slope. Defaults to 1.0 when the tag is absent.
    pub rescale_slope: f64,
    /// True when PixelRepresentation is 1 (signed two's-complement samples).
    pub signed_pixels: bool,
    /// Modality string (e.g. "CT", "DX").
    pub modality: String,
    /// Voxel spacing in mm (X, Y, Z). Defaults to (1.0, 1.0, 1.0).
    pub voxel_spacing: [f64; 3],
    /// Parsed threat bounding boxes (if present in metadata).
    pub threats: Vec<ThreatBox>,
}

impl Volume {
    /// Interpret a stored sample as a signed or unsigned integer.
    pub fn stored_sample(&self, raw: u16) -> f64 {
        if self.signed_pixels {
            f64::from(raw as i16)
        } else {
            f64::from(raw)
        }
    }

    /// Convert a stored sample to a modality (display) value:
    /// `stored * RescaleSlope + RescaleIntercept`.
    pub fn modality_value(&self, raw: u16) -> f64 {
        self.stored_sample(raw) * self.rescale_slope + self.rescale_intercept
    }

    /// Compute the density-weighted center of mass in normalized [0,1] coordinates.
    ///
    /// Single linear pass: computes min/max for threshold, then accumulates
    /// weighted coordinates using flat-index arithmetic to avoid nested loops.
    pub fn center_of_mass(&self) -> [f32; 3] {
        if self.data.is_empty() {
            return [0.5, 0.5, 0.5];
        }

        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;
        for &v in &self.data {
            let val = self.stored_sample(v);
            min_val = min_val.min(val);
            max_val = max_val.max(val);
        }
        let threshold = min_val + (max_val - min_val) * 0.1;

        let dx = self.dim_x;
        let dxy = self.dim_x * self.dim_y;
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_z = 0.0f64;
        let mut total_weight = 0.0f64;

        for (i, &v) in self.data.iter().enumerate() {
            let val = self.stored_sample(v);
            if val > threshold {
                // Shift by a negative minimum so weights are never negative
                // (signed data); unsigned data keeps its original weights.
                let w = val - min_val.min(0.0);
                let z = i / dxy;
                let rem = i % dxy;
                let y = rem / dx;
                let x = rem % dx;
                sum_x += x as f64 * w;
                sum_y += y as f64 * w;
                sum_z += z as f64 * w;
                total_weight += w;
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
        // Simple percentile: sample up to 100k voxels for speed.
        // Percentiles are taken in the modality (slope + intercept) domain
        // so the resulting window matches slice and 3D rendering.
        let step = (self.data.len() / 100_000).max(1);
        let mut samples: Vec<f64> = self
            .data
            .iter()
            .step_by(step)
            .map(|&v| self.modality_value(v))
            .collect();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p01 = samples[samples.len() / 100];
        let p99 = samples[samples.len() * 99 / 100];
        if p99 <= p01 {
            return;
        }
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
        // Gradients use the signed stored value so a sample of -1 is not
        // treated as 65535. The density channel must be monotonic in the
        // stored value so the GPU's linear filter blends real values: signed
        // samples are stored offset-binary (stored + 32768), which the shader
        // undoes (see renderer::density_decode).
        let stored = |i: usize| self.stored_sample(data[i]) as f32;
        let density = |raw: u16| {
            if self.signed_pixels {
                raw ^ 0x8000
            } else {
                raw
            }
        };

        for z in 0..dz {
            let z_base = z * stride_z;
            let z_is_edge = z == 0 || z == dz - 1;
            for y in 0..dy {
                let yz_base = z_base + y * stride_y;
                let y_is_edge = y == 0 || y == dy - 1;
                for x in 0..dx {
                    let idx = yz_base + x;
                    let pi = idx * 4;
                    let center = stored(idx);

                    let gx = if x == 0 || x == dx - 1 {
                        let left = if x > 0 { stored(idx - 1) } else { center };
                        let right = if x < dx - 1 { stored(idx + 1) } else { center };
                        (right - left) * 0.5
                    } else {
                        (stored(idx + 1) - stored(idx - 1)) * 0.5
                    };

                    let gy = if y_is_edge {
                        let below = if y > 0 {
                            stored(idx - stride_y)
                        } else {
                            center
                        };
                        let above = if y < dy - 1 {
                            stored(idx + stride_y)
                        } else {
                            center
                        };
                        (above - below) * 0.5
                    } else {
                        (stored(idx + stride_y) - stored(idx - stride_y)) * 0.5
                    };

                    let gz = if z_is_edge {
                        let back = if z > 0 {
                            stored(idx - stride_z)
                        } else {
                            center
                        };
                        let front = if z < dz - 1 {
                            stored(idx + stride_z)
                        } else {
                            center
                        };
                        (front - back) * 0.5
                    } else {
                        (stored(idx + stride_z) - stored(idx - stride_z)) * 0.5
                    };

                    packed[pi] = density(data[idx]);
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
            rescale_slope: 1.0,
            signed_pixels: false,
            modality: "CT".into(),
            voxel_spacing: [1.0, 1.0, 1.0],
            threats: Vec::new(),
        }
    }

    #[test]
    fn automatic_window_uses_the_same_rescaled_domain_as_rendering() {
        let mut vol = test_volume(10);
        vol.data = (0..1000).collect();
        vol.rescale_intercept = -1024.0;
        vol.compute_window_from_data();
        assert_eq!(vol.window_center, -524.0);
        assert_eq!(vol.window_width, 980.0);
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
            rescale_slope: 1.0,
            signed_pixels: false,
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
    fn modality_value_interprets_signed_bits() {
        let mut vol = test_volume(1);
        vol.signed_pixels = true;
        vol.rescale_slope = 1.0;
        vol.rescale_intercept = 0.0;
        assert_eq!(vol.modality_value((-1i16) as u16), -1.0);
        vol.signed_pixels = false;
        assert_eq!(vol.modality_value((-1i16) as u16), 65535.0);
    }

    #[test]
    fn automatic_window_applies_slope() {
        let mut vol = test_volume(10);
        vol.data = (0..1000).collect();
        vol.rescale_slope = 2.0;
        vol.rescale_intercept = -1024.0;
        vol.compute_window_from_data();
        // p01 = 10*2-1024 = -1004, p99 = 990*2-1024 = 956, width = 1960
        assert_eq!(vol.window_center, -24.0);
        assert_eq!(vol.window_width, 1960.0);
    }

    #[test]
    fn packed_gradient_uses_signed_samples() {
        // A 3x1x1 line: -2, 0, 2 as i16. The central gradient should be
        // (2 - (-2)) / 2 = 2, not a wrap from 65534.
        let mut vol = test_volume(1);
        vol.dim_x = 3;
        vol.dim_y = 1;
        vol.dim_z = 1;
        vol.data = vec![(-2i16) as u16, 0, 2];
        vol.signed_pixels = true;
        let packed = vol.pack_for_gpu();
        // Center voxel is index 1; channel 1 is gradient x, packed as unorm
        // (value / 65535 + 0.5).
        let gx = packed[5];
        let expected = ((2.0_f32 / 65535.0 + 0.5) * 65535.0).round() as u16;
        assert_eq!(gx, expected);
    }

    /// Signed density is offset-binary so the GPU's linear filter blends
    /// real values: -1000 next to +40 must decode to about -480, not +32288.
    #[test]
    fn signed_density_is_offset_binary_and_filters_linearly() {
        let mut vol = test_volume(1);
        vol.dim_x = 2;
        vol.data = vec![(-1000i16) as u16, 40];
        vol.signed_pixels = true;
        let packed = vol.pack_for_gpu();
        let (a, b) = (f32::from(packed[0]), f32::from(packed[4]));
        assert_eq!(a - 32768.0, -1000.0);
        assert_eq!(b - 32768.0, 40.0);
        // Hardware linear filtering of a unorm texture is an affine blend.
        let midpoint = (a + b) * 0.5 - 32768.0;
        assert_eq!(midpoint, -480.0);
    }

    /// Unsigned density keeps the raw stored bits.
    #[test]
    fn unsigned_density_is_unchanged() {
        let mut vol = test_volume(1);
        vol.data = vec![40000];
        assert_eq!(vol.pack_for_gpu()[0], 40000);
    }

    /// Negative samples must not pull the centre of mass outside the volume.
    #[test]
    fn center_of_mass_with_negative_samples_stays_inside() {
        let mut vol = test_volume(1);
        vol.dim_x = 3;
        vol.data = vec![(-100i16) as u16, (-2i16) as u16, 3];
        vol.signed_pixels = true;
        let c = vol.center_of_mass();
        assert!((0.0..=1.0).contains(&c[0]), "centre outside volume: {c:?}");
    }

    /// Unsigned data with a nonzero baseline keeps its original weighting.
    #[test]
    fn center_of_mass_unsigned_weights_unchanged() {
        let mut vol = test_volume(1);
        vol.dim_x = 3;
        vol.data = vec![1000, 1200, 2000];
        // threshold = 1100, so voxels 1 and 2 count with weights 1200, 2000.
        let expected = ((1.0 * 1200.0 + 2.0 * 2000.0) / 3200.0 / 3.0) as f32;
        assert_eq!(vol.center_of_mass()[0], expected);
    }
}
