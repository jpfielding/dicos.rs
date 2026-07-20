//! Transfer function generation for volume rendering.
//!
//! Implements five-band material-based color mapping:
//! Air (transparent), Organic (orange), Inorganic (green), Metal (blue),
//! Dense (dark blue). Each band has configurable thresholds and opacity.

/// Number of entries in the 1D transfer function texture.
pub const TRANSFER_SIZE: usize = 1024;

/// A single color band in the transfer function.
#[derive(Debug, Clone)]
pub struct ColorBand {
    /// Human-readable name.
    pub name: &'static str,
    /// RGB color (0-255).
    pub color: [u8; 3],
    /// Upper density threshold (0-65535).
    pub threshold: u16,
    /// Whether this band is transparent (air/background).
    pub is_transparent: bool,
    /// Per-band opacity multiplier (0.0-1.0).
    pub alpha: f32,
}

/// Transfer function preset type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPreset {
    /// Default preset: orange/green/blue material bands.
    Default,
    /// Red monochrome for threat highlighting.
    Threat,
    /// Grayscale.
    Monochrome,
}

/// Default material color bands matching the Go reference implementation.
///
/// The `alpha` field is a per-band multiplier (default 1.0 = use baseline opacity).
/// Actual opacity comes from the default opacity map interpolated by density.
pub fn default_bands() -> Vec<ColorBand> {
    vec![
        ColorBand {
            name: "Air",
            color: [250, 200, 110],
            threshold: 8000,
            is_transparent: true,
            alpha: 1.0,
        },
        ColorBand {
            name: "Organic",
            color: [230, 150, 50],
            threshold: 15000,
            is_transparent: false,
            alpha: 1.0,
        },
        ColorBand {
            name: "Inorganic",
            color: [80, 200, 40],
            threshold: 20000,
            is_transparent: false,
            alpha: 1.0,
        },
        ColorBand {
            name: "Metal",
            color: [15, 165, 200],
            threshold: 25000,
            is_transparent: false,
            alpha: 1.0,
        },
        ColorBand {
            name: "Dense",
            color: [40, 48, 180],
            threshold: 30000,
            is_transparent: false,
            alpha: 1.0,
        },
    ]
}

/// Linearly interpolate an opacity value from a sorted (density, alpha) map.
fn interpolate_opacity(map: &[(f32, f32)], density: f32) -> f32 {
    if map.is_empty() {
        return 0.0;
    }
    if density <= map[0].0 {
        return map[0].1;
    }
    if density >= map[map.len() - 1].0 {
        return map[map.len() - 1].1;
    }
    for i in 0..map.len() - 1 {
        let (d0, a0) = map[i];
        let (d1, a1) = map[i + 1];
        if density >= d0 && density < d1 {
            let t = (density - d0) / (d1 - d0);
            return a0 + t * (a1 - a0);
        }
    }
    0.0
}

/// An RGBA transfer function table (1024 entries, 4 bytes each).
///
/// # Opacity model
///
/// Final on-screen opacity is the product of four independent, multiplicative
/// factors. Each answers a different question and lives in a different
/// place, so reach for the right one rather than fighting the others:
///
/// 1. **`OPACITY_MAP` density curve** (shape) -- the base opacity-vs-density
///    ramp baked into [`from_bands`](TransferFunction::from_bands) (see the
///    `OPACITY_MAP` constant). This is the overall "how see-through is bone
///    vs. metal vs. air" silhouette. Reach for this when the *shape* of the
///    density-to-opacity ramp is wrong (e.g. everything above a threshold
///    should fade in faster/slower).
/// 2. **Per-band alpha** (material-selective gain) -- [`ColorBand::alpha`],
///    a multiplier applied on top of the density curve for one specific
///    material band (Air, Organic, Inorganic, Metal, Dense). This is
///    retained state, not redundant with the other factors: it is the only
///    per-material knob. Reach for this to make e.g. only "Metal" more or
///    less opaque without touching any other material or the global scale.
/// 3. **Global opacity** -- [`SliceView::alpha_scale`](crate::slice_view::SliceView::alpha_scale)
///    on the CPU composite path, kept in sync with
///    [`RenderSettings::global_opacity`](crate::state::RenderSettings::global_opacity)
///    (`u.alpha_scale` on the GPU raycast path). A single uniform scale
///    applied to every sample regardless of material. Reach for this for a
///    single "make the whole volume more/less see-through" slider.
/// 4. **Shader gradient edge boost** -- WGSL-only (`raycast.wgsl`, the
///    `alpha *= 1.0 + grad_mag * 0.5` line), boosts alpha at high-gradient
///    voxels (surfaces/edges) to make boundaries pop visually. This has no
///    CPU-composite equivalent and is not user-configurable; it is a fixed
///    rendering heuristic, not a control to reach for.
///
/// These compose as: `alpha = curve(density) * band.alpha * global_opacity *
/// gradient_boost`. Factors 1-3 apply to both the CPU composite path
/// ([`crate::slice_view::SliceView::render_composite`]) and the GPU raycast
/// path (`raycast.wgsl`); factor 4 is GPU-only.
pub struct TransferFunction {
    /// RGBA data: `[R, G, B, A]` for each of `TRANSFER_SIZE` entries.
    pub data: Vec<[f32; 4]>,
}

impl TransferFunction {
    /// Generate a transfer function from color bands with the default opacity map.
    ///
    /// Colors come from the band definitions; opacity is interpolated from the
    /// reference default opacity map. The per-band `alpha`
    /// field acts as a multiplier on the base opacity (1.0 = no change).
    pub fn from_bands(bands: &[ColorBand]) -> Self {
        // Default opacity map: (density, alpha) pairs from config.go.
        const OPACITY_MAP: &[(f32, f32)] = &[
            (0.0, 0.0),
            (499.0, 0.0),
            (500.0, 0.005),
            (800.0, 0.008),
            (1449.0, 0.008),
            (1450.0, 0.01),
            (3000.0, 0.01),
            (4500.0, 0.01),
            (6500.0, 0.02),
            (6600.0, 0.03),
            (7000.0, 0.05),
            (9000.0, 0.05),
            (9500.0, 0.05),
            (10100.0, 0.08),
            (15000.0, 0.08),
            (30000.0, 0.3),
            (35000.0, 0.4),
        ];

        let mut data = vec![[0.0f32; 4]; TRANSFER_SIZE];
        if bands.is_empty() {
            return Self { data };
        }

        let max_density = bands.last().map(|b| b.threshold).unwrap_or(30000) as f32;

        for (i, entry) in data.iter_mut().enumerate() {
            let density = (i as f32 / (TRANSFER_SIZE - 1) as f32) * max_density;

            // Find which band this density falls into.
            let mut band_idx = 0;
            let mut prev_threshold = 0.0f32;
            for (bi, band) in bands.iter().enumerate() {
                if density <= band.threshold as f32 {
                    band_idx = bi;
                    break;
                }
                prev_threshold = band.threshold as f32;
                if bi == bands.len() - 1 {
                    band_idx = bi;
                }
            }

            let band = &bands[band_idx];

            if band.is_transparent {
                *entry = [0.0, 0.0, 0.0, 0.0];
                continue;
            }

            // Progress within the band (0.0 to 1.0).
            let band_range = band.threshold as f32 - prev_threshold;
            let progress = if band_range > 0.0 {
                ((density - prev_threshold) / band_range).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Apply brightness gradient within band.
            let brightness = 0.85 + 0.3 * progress;
            let r = (band.color[0] as f32 / 255.0) * brightness;
            let g = (band.color[1] as f32 / 255.0) * brightness;
            let b = (band.color[2] as f32 / 255.0) * brightness;

            // Interpolate opacity from the default opacity map.
            let base_alpha = interpolate_opacity(OPACITY_MAP, density);
            // Apply per-band multiplier.
            let alpha = base_alpha * band.alpha;

            *entry = [r.min(1.0), g.min(1.0), b.min(1.0), alpha];
        }

        Self { data }
    }

    /// Generate a threat-mode transfer function (red monochrome).
    pub fn threat() -> Self {
        let mut data = vec![[0.0f32; 4]; TRANSFER_SIZE];
        let threshold_idx = (700.0 / 30000.0 * (TRANSFER_SIZE - 1) as f32) as usize;

        for (i, entry) in data.iter_mut().enumerate() {
            if i < threshold_idx {
                *entry = [0.0, 0.0, 0.0, 0.0];
            } else {
                let progress =
                    (i - threshold_idx) as f32 / (TRANSFER_SIZE - 1 - threshold_idx) as f32;
                let alpha = 0.03 + 0.17 * progress;
                *entry = [188.0 / 255.0, 25.0 / 255.0, 30.0 / 255.0, alpha];
            }
        }

        Self { data }
    }

    /// Generate a monochrome (grayscale) transfer function.
    pub fn monochrome() -> Self {
        let mut data = vec![[0.0f32; 4]; TRANSFER_SIZE];

        for (i, entry) in data.iter_mut().enumerate() {
            let t = i as f32 / (TRANSFER_SIZE - 1) as f32;
            // Grayscale ramp: 0 -> 0.67 (170/255)
            let gray = 0.67 * t;
            let alpha = 0.5 * t;
            *entry = [gray, gray, gray, alpha];
        }

        Self { data }
    }

    /// Generate a transfer function for the given preset.
    pub fn from_preset(preset: TransferPreset) -> Self {
        match preset {
            TransferPreset::Default => Self::from_bands(&default_bands()),
            TransferPreset::Threat => Self::threat(),
            TransferPreset::Monochrome => Self::monochrome(),
        }
    }

    /// Pack the transfer function into a flat `Vec<f32>` for GPU upload (RGBA32F).
    pub fn as_rgba_f32(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(TRANSFER_SIZE * 4);
        for entry in &self.data {
            out.extend_from_slice(entry);
        }
        out
    }

    /// Pack the transfer function into `Vec<u8>` for GPU upload (RGBA8Unorm).
    pub fn as_rgba_u8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(TRANSFER_SIZE * 4);
        for entry in &self.data {
            out.push((entry[0] * 255.0).clamp(0.0, 255.0) as u8);
            out.push((entry[1] * 255.0).clamp(0.0, 255.0) as u8);
            out.push((entry[2] * 255.0).clamp(0.0, 255.0) as u8);
            out.push((entry[3] * 255.0).clamp(0.0, 255.0) as u8);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bands_count() {
        let bands = default_bands();
        assert_eq!(bands.len(), 5);
        assert!(bands[0].is_transparent);
        assert!(!bands[1].is_transparent);
    }

    #[test]
    fn transfer_function_size() {
        let tf = TransferFunction::from_bands(&default_bands());
        assert_eq!(tf.data.len(), TRANSFER_SIZE);
    }

    #[test]
    fn air_band_is_transparent() {
        let tf = TransferFunction::from_bands(&default_bands());
        // First entries (air band) should be transparent.
        assert_eq!(tf.data[0][3], 0.0);
        assert_eq!(tf.data[1][3], 0.0);
    }

    #[test]
    fn dense_band_has_opacity() {
        let tf = TransferFunction::from_bands(&default_bands());
        // Last entry should have non-zero alpha.
        let last = tf.data[TRANSFER_SIZE - 1];
        assert!(last[3] > 0.0);
    }

    #[test]
    fn threat_below_threshold_is_transparent() {
        let tf = TransferFunction::threat();
        assert_eq!(tf.data[0][3], 0.0);
    }

    #[test]
    fn monochrome_gradient() {
        let tf = TransferFunction::monochrome();
        // First entry should be dark.
        assert!(tf.data[0][0] < 0.01);
        // Last entry should be brighter.
        assert!(tf.data[TRANSFER_SIZE - 1][0] > 0.5);
    }

    #[test]
    fn as_rgba_u8_length() {
        let tf = TransferFunction::from_preset(TransferPreset::Default);
        let bytes = tf.as_rgba_u8();
        assert_eq!(bytes.len(), TRANSFER_SIZE * 4);
    }

    #[test]
    fn as_rgba_f32_length() {
        let tf = TransferFunction::from_preset(TransferPreset::Default);
        let floats = tf.as_rgba_f32();
        assert_eq!(floats.len(), TRANSFER_SIZE * 4);
    }

    #[test]
    fn preset_roundtrip() {
        for preset in [
            TransferPreset::Default,
            TransferPreset::Threat,
            TransferPreset::Monochrome,
        ] {
            let tf = TransferFunction::from_preset(preset);
            assert_eq!(tf.data.len(), TRANSFER_SIZE);
        }
    }
}
