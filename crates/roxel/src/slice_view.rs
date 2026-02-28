//! CPU-based 2D slice renderer for volume data.
//!
//! Renders single slices or MIP composite projections from a loaded volume
//! into RGBA images for display via egui. No GPU required -- operates directly
//! on the `Volume` struct's voxel data.

use crate::volume::{ThreatBox, Volume};

/// Slice orientation through the volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// XY plane (looking down Z axis).
    Axial,
    /// XZ plane (looking down Y axis).
    Coronal,
    /// YZ plane (looking from the side, X axis).
    Sagittal,
}

impl Orientation {
    /// All orientations for iteration / dropdown display.
    pub const ALL: [Orientation; 3] = [
        Orientation::Axial,
        Orientation::Coronal,
        Orientation::Sagittal,
    ];

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Orientation::Axial => "Axial",
            Orientation::Coronal => "Coronal",
            Orientation::Sagittal => "Sagittal",
        }
    }
}

/// State for the 2D slice/projection panel.
pub struct SliceView {
    /// Current orientation.
    pub orientation: Orientation,
    /// Current slice index (0-based).
    pub slice_index: usize,
    /// Max slices for current orientation (updated when volume changes).
    pub max_slices: usize,
    /// Window center for 2D display.
    pub window_center: f32,
    /// Window width for 2D display.
    pub window_width: f32,
    /// Whether to show alpha-composited projection instead of single slice.
    pub composite: bool,
    /// Which volume to display (index into loaded volumes).
    pub volume_index: usize,
    /// Global alpha scale for composite rendering (synced from 3D opacity).
    pub alpha_scale: f32,
    /// Transfer function for composite rendering (RGBA, 1024 entries).
    /// When set, composite mode uses front-to-back alpha blending with
    /// transfer function colors instead of simple MIP.
    pub transfer_func: Option<Vec<[f32; 4]>>,
    /// Display zoom percentage (100 = fit, 250 = 250%).
    pub zoom: f32,
}

impl Default for SliceView {
    fn default() -> Self {
        Self {
            orientation: Orientation::Axial,
            slice_index: 0,
            max_slices: 1,
            window_center: 32768.0,
            window_width: 65536.0,
            composite: true,
            volume_index: 0,
            alpha_scale: 0.5,
            transfer_func: None,
            zoom: 100.0,
        }
    }
}

impl SliceView {
    /// Update max_slices based on the loaded volume and current orientation.
    pub fn update_for_volume(&mut self, vol: &Volume) {
        self.max_slices = Self::depth_for_orientation(vol, self.orientation);
        if self.slice_index >= self.max_slices {
            self.slice_index = self.max_slices.saturating_sub(1);
        }
    }

    /// Return the depth dimension for a given orientation.
    fn depth_for_orientation(vol: &Volume, orientation: Orientation) -> usize {
        match orientation {
            Orientation::Axial => vol.dim_z,
            Orientation::Coronal => vol.dim_y,
            Orientation::Sagittal => vol.dim_x,
        }
    }

    /// Return the (width, height) of a slice for the given orientation.
    fn slice_dims(vol: &Volume, orientation: Orientation) -> (usize, usize) {
        match orientation {
            Orientation::Axial => (vol.dim_x, vol.dim_y),
            Orientation::Coronal => (vol.dim_x, vol.dim_z),
            Orientation::Sagittal => (vol.dim_y, vol.dim_z),
        }
    }

    /// Sample a voxel from the volume at the given (u, v, slice) in the
    /// current orientation's coordinate system.
    fn sample(vol: &Volume, orientation: Orientation, u: usize, v: usize, s: usize) -> u16 {
        let (x, y, z) = match orientation {
            Orientation::Axial => (u, v, s),
            Orientation::Coronal => (u, s, v),
            Orientation::Sagittal => (s, u, v),
        };
        vol.data[z * vol.dim_y * vol.dim_x + y * vol.dim_x + x]
    }

    /// Render a single slice as a grayscale `egui::ColorImage`.
    ///
    /// Applies window/level to map raw uint16 voxel values to 0-255 grayscale.
    pub fn render_slice(&self, vol: &Volume) -> egui::ColorImage {
        let (w, h) = Self::slice_dims(vol, self.orientation);
        if w == 0 || h == 0 {
            return egui::ColorImage::filled([1, 1], egui::Color32::BLACK);
        }

        let half_width = self.window_width * 0.5;
        let low = self.window_center - half_width;
        let inv_width = if self.window_width > 0.0 {
            255.0 / self.window_width
        } else {
            0.0
        };

        let mut pixels = Vec::with_capacity(w * h);
        for v in 0..h {
            for u in 0..w {
                let raw = Self::sample(vol, self.orientation, u, v, self.slice_index) as f32;
                let gray = ((raw - low) * inv_width).clamp(0.0, 255.0) as u8;
                pixels.push(egui::Color32::from_gray(gray));
            }
        }

        egui::ColorImage::new([w, h], pixels)
    }

    /// Render a front-to-back alpha-composited projection using the transfer
    /// function, matching the Go viewer's composite mode.
    ///
    /// Each voxel along the projection axis is looked up in the transfer
    /// function (color + alpha), then blended front-to-back.  The result is
    /// composited over a white background.
    ///
    /// Falls back to grayscale MIP if no transfer function is set.
    pub fn render_composite(&self, vol: &Volume) -> egui::ColorImage {
        let (w, h) = Self::slice_dims(vol, self.orientation);
        let depth = Self::depth_for_orientation(vol, self.orientation);
        if w == 0 || h == 0 || depth == 0 {
            return egui::ColorImage::filled([1, 1], egui::Color32::BLACK);
        }

        let half_width = self.window_width * 0.5;
        let w_min = self.window_center - half_width;
        let w_range = self.window_width;

        let tf = self.transfer_func.as_deref();
        let tf_len = tf.map_or(0, |t| t.len());
        let alpha_scale = self.alpha_scale;

        let mut pixels = Vec::with_capacity(w * h);
        for v in 0..h {
            for u in 0..w {
                let mut acc_r: f32 = 0.0;
                let mut acc_g: f32 = 0.0;
                let mut acc_b: f32 = 0.0;
                let mut acc_a: f32 = 0.0;

                for s in 0..depth {
                    if acc_a >= 0.98 {
                        break;
                    }

                    let density = Self::sample(vol, self.orientation, u, v, s) as f32;

                    // Window/level normalization.
                    if density <= w_min {
                        continue;
                    }
                    let norm = ((density - w_min) / w_range).clamp(0.0, 1.0);

                    if let Some(tf) = tf {
                        // Transfer function lookup.
                        let idx = (norm * (tf_len - 1) as f32) as usize;
                        let [cr, cg, cb, ca] = tf[idx.min(tf_len - 1)];
                        // Adjust alpha: pow(alpha, 1.5) * alphaScale (matches Go)
                        let alpha = ca.powf(1.5) * alpha_scale;
                        if alpha > 0.0 {
                            acc_r += (1.0 - acc_a) * cr * alpha;
                            acc_g += (1.0 - acc_a) * cg * alpha;
                            acc_b += (1.0 - acc_a) * cb * alpha;
                            acc_a += (1.0 - acc_a) * alpha;
                        }
                    } else {
                        // Fallback: grayscale MIP (take max).
                        let gray = norm;
                        acc_r = acc_r.max(gray);
                        acc_g = acc_g.max(gray);
                        acc_b = acc_b.max(gray);
                        acc_a = 1.0;
                    }
                }

                // Blend over white background.
                let final_r = acc_r + (1.0 - acc_a) * 1.0;
                let final_g = acc_g + (1.0 - acc_a) * 1.0;
                let final_b = acc_b + (1.0 - acc_a) * 1.0;

                pixels.push(egui::Color32::from_rgb(
                    (final_r.clamp(0.0, 1.0) * 255.0) as u8,
                    (final_g.clamp(0.0, 1.0) * 255.0) as u8,
                    (final_b.clamp(0.0, 1.0) * 255.0) as u8,
                ));
            }
        }

        egui::ColorImage::new([w, h], pixels)
    }

    fn draw_threat_rect(
        image: &mut egui::ColorImage,
        mut x0: usize,
        mut y0: usize,
        mut x1: usize,
        mut y1: usize,
        color: egui::Color32,
    ) {
        let w = image.size[0];
        let h = image.size[1];
        if w == 0 || h == 0 {
            return;
        }

        x0 = x0.min(w.saturating_sub(1));
        x1 = x1.min(w.saturating_sub(1));
        y0 = y0.min(h.saturating_sub(1));
        y1 = y1.min(h.saturating_sub(1));
        if x0 > x1 || y0 > y1 {
            return;
        }

        for x in x0..=x1 {
            image.pixels[y0 * w + x] = color;
            image.pixels[y1 * w + x] = color;
        }
        for y in y0..=y1 {
            image.pixels[y * w + x0] = color;
            image.pixels[y * w + x1] = color;
        }
    }

    fn project_threat(
        &self,
        threat: &ThreatBox,
        composite: bool,
    ) -> Option<(usize, usize, usize, usize)> {
        let [x0, y0, z0] = threat.min;
        let [x1, y1, z1] = threat.max;

        let visible = match self.orientation {
            Orientation::Axial => composite || (z0 <= self.slice_index && self.slice_index <= z1),
            Orientation::Coronal => composite || (y0 <= self.slice_index && self.slice_index <= y1),
            Orientation::Sagittal => {
                composite || (x0 <= self.slice_index && self.slice_index <= x1)
            }
        };
        if !visible {
            return None;
        }

        let rect = match self.orientation {
            Orientation::Axial => (x0, y0, x1, y1),
            Orientation::Coronal => (x0, z0, x1, z1),
            Orientation::Sagittal => (y0, z0, y1, z1),
        };
        Some(rect)
    }

    fn overlay_threats(&self, image: &mut egui::ColorImage, vol: &Volume, show_threats: bool) {
        if !show_threats {
            return;
        }
        for threat in &vol.threats {
            if !threat.enabled {
                continue;
            }
            if let Some((x0, y0, x1, y1)) = self.project_threat(threat, self.composite) {
                let color =
                    egui::Color32::from_rgb(threat.color[0], threat.color[1], threat.color[2]);
                Self::draw_threat_rect(image, x0, y0, x1, y1, color);
            }
        }
    }

    /// Render either a single slice or MIP composite depending on `self.composite`.
    pub fn render(&self, vol: &Volume, show_threats: bool) -> egui::ColorImage {
        let mut image = if self.composite {
            self.render_composite(vol)
        } else {
            self.render_slice(vol)
        };
        self.overlay_threats(&mut image, vol, show_threats);
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_volume() -> Volume {
        // 4x3x2 volume with predictable values.
        let dim_x = 4;
        let dim_y = 3;
        let dim_z = 2;
        let mut data = vec![0u16; dim_x * dim_y * dim_z];
        // Fill with z*1000 + y*100 + x so we can verify sampling.
        for z in 0..dim_z {
            for y in 0..dim_y {
                for x in 0..dim_x {
                    data[z * dim_y * dim_x + y * dim_x + x] = (z * 1000 + y * 100 + x) as u16;
                }
            }
        }
        Volume {
            data,
            dim_x,
            dim_y,
            dim_z,
            window_center: 1000.0,
            window_width: 2000.0,
            rescale_intercept: 0.0,
            modality: "CT".into(),
            voxel_spacing: [1.0, 1.0, 1.0],
            threats: Vec::new(),
        }
    }

    #[test]
    fn orientation_labels() {
        assert_eq!(Orientation::Axial.label(), "Axial");
        assert_eq!(Orientation::Coronal.label(), "Coronal");
        assert_eq!(Orientation::Sagittal.label(), "Sagittal");
    }

    #[test]
    fn slice_dims_axial() {
        let vol = test_volume();
        let (w, h) = SliceView::slice_dims(&vol, Orientation::Axial);
        assert_eq!((w, h), (4, 3));
    }

    #[test]
    fn slice_dims_coronal() {
        let vol = test_volume();
        let (w, h) = SliceView::slice_dims(&vol, Orientation::Coronal);
        assert_eq!((w, h), (4, 2));
    }

    #[test]
    fn slice_dims_sagittal() {
        let vol = test_volume();
        let (w, h) = SliceView::slice_dims(&vol, Orientation::Sagittal);
        assert_eq!((w, h), (3, 2));
    }

    #[test]
    fn sample_axial() {
        let vol = test_volume();
        // Axial: (u, v, s) = (x, y, z)
        assert_eq!(SliceView::sample(&vol, Orientation::Axial, 2, 1, 0), 102);
        assert_eq!(SliceView::sample(&vol, Orientation::Axial, 3, 2, 1), 1203);
    }

    #[test]
    fn sample_coronal() {
        let vol = test_volume();
        // Coronal: (u, v, s) = (x, z, y) -> (x=u, y=s, z=v)
        assert_eq!(SliceView::sample(&vol, Orientation::Coronal, 1, 1, 2), 1201);
    }

    #[test]
    fn sample_sagittal() {
        let vol = test_volume();
        // Sagittal: (u, v, s) = (y, z, x) -> (x=s, y=u, z=v)
        assert_eq!(
            SliceView::sample(&vol, Orientation::Sagittal, 2, 1, 3),
            1203
        );
    }

    #[test]
    fn render_slice_dimensions() {
        let vol = test_volume();
        let sv = SliceView {
            orientation: Orientation::Axial,
            slice_index: 0,
            max_slices: 2,
            window_center: 1000.0,
            window_width: 2000.0,
            composite: false,
            volume_index: 0,
            ..SliceView::default()
        };
        let img = sv.render_slice(&vol);
        assert_eq!(img.size, [4, 3]);
        assert_eq!(img.pixels.len(), 12);
    }

    #[test]
    fn render_composite_dimensions() {
        let vol = test_volume();
        let sv = SliceView {
            orientation: Orientation::Axial,
            slice_index: 0,
            max_slices: 2,
            window_center: 1000.0,
            window_width: 2000.0,
            composite: true,
            volume_index: 0,
            ..SliceView::default()
        };
        let img = sv.render_composite(&vol);
        assert_eq!(img.size, [4, 3]);
        assert_eq!(img.pixels.len(), 12);
    }

    #[test]
    fn composite_picks_max() {
        let vol = test_volume();
        let sv = SliceView {
            orientation: Orientation::Axial,
            slice_index: 0,
            max_slices: 2,
            // Wide window so all values map linearly.
            window_center: 1000.0,
            window_width: 3000.0,
            composite: true,
            volume_index: 0,
            ..SliceView::default()
        };
        let img = sv.render_composite(&vol);
        // Without a transfer function, fallback is grayscale MIP over white bg.
        // At (3,2): max across z is 1203. norm = (1203 - (-500))/3000 = 0.567
        // MIP picks max norm per pixel, then blends: final = norm + (1-1)*white = norm
        // result = 0.567 * 255 ≈ 144 as grayscale.
        let pixel = img.pixels[2 * 4 + 3]; // row 2, col 3
                                           // The pixel should not be pure white (255) since the MIP value is mid-range.
        assert!(
            pixel.r() < 200,
            "Expected non-white pixel from MIP, got {}",
            pixel.r()
        );
    }

    #[test]
    fn update_for_volume_clamps_index() {
        let vol = test_volume();
        let mut sv = SliceView {
            orientation: Orientation::Axial,
            slice_index: 100,
            max_slices: 1,
            ..SliceView::default()
        };
        sv.update_for_volume(&vol);
        assert_eq!(sv.max_slices, 2);
        assert_eq!(sv.slice_index, 1); // clamped to max_slices - 1
    }

    #[test]
    fn render_dispatches_correctly() {
        let vol = test_volume();
        let mut sv = SliceView {
            orientation: Orientation::Axial,
            slice_index: 0,
            max_slices: 2,
            window_center: 1000.0,
            window_width: 2000.0,
            composite: false,
            volume_index: 0,
            ..SliceView::default()
        };
        let single = sv.render(&vol, true);
        sv.composite = true;
        let composite = sv.render(&vol, true);
        // Both should have same dimensions but possibly different pixel values.
        assert_eq!(single.size, composite.size);
    }
}
