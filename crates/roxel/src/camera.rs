//! Arcball camera for 3D volume viewing.
//!
//! Implements an orbit camera that rotates around a target point (center of
//! mass of the volume). Supports mouse-drag rotation, screen-space panning,
//! and scroll zoom.

use glam::Vec3;

/// Arcball camera orbiting a target point.
pub struct Camera {
    /// Target point the camera orbits around (normalized volume coords).
    pub target: Vec3,
    /// Azimuth angle in radians (rotation around Y axis).
    pub azimuth: f32,
    /// Elevation angle in radians (rotation around horizontal axis).
    pub elevation: f32,
    /// Distance from target (zoom level).
    pub distance: f32,
    /// Vertical field of view factor (not degrees -- used as a ray spread).
    pub fov: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.5, 0.5, 0.5),
            azimuth: 0.5,    // ~29 degrees from front
            elevation: -0.4, // slight top-down view
            distance: 1.0,
            fov: 0.8,
        }
    }
}

impl Camera {
    /// Minimum allowed orbit distance (closest zoom-in).
    pub const MIN_DISTANCE: f32 = 0.5;
    /// Maximum allowed orbit distance (furthest zoom-out).
    pub const MAX_DISTANCE: f32 = 5.0;

    /// Compute the camera position in world space.
    pub fn position(&self) -> Vec3 {
        let (sin_az, cos_az) = self.azimuth.sin_cos();
        let (sin_el, cos_el) = self.elevation.sin_cos();

        Vec3::new(
            sin_az * cos_el * self.distance + self.target.x,
            sin_el * self.distance + self.target.y,
            cos_az * cos_el * self.distance + self.target.z,
        )
    }

    /// Compute the forward direction (toward target).
    pub fn forward(&self) -> Vec3 {
        (self.target - self.position()).normalize()
    }

    /// Compute the right vector.
    pub fn right(&self) -> Vec3 {
        let (_, cos_az) = self.azimuth.sin_cos();
        let (sin_az, _) = self.azimuth.sin_cos();
        Vec3::new(cos_az, 0.0, -sin_az)
    }

    /// Compute the up vector.
    pub fn up(&self) -> Vec3 {
        self.forward().cross(self.right()).normalize()
    }

    /// Rotate the camera by delta angles (in radians).
    pub fn rotate(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.azimuth += delta_azimuth;
        self.elevation += delta_elevation;
    }

    /// Zoom by a multiplicative factor (>1 zooms in, <1 zooms out).
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance / factor).clamp(Self::MIN_DISTANCE, Self::MAX_DISTANCE);
    }

    /// Pan the camera target in view space using pixel deltas.
    ///
    /// Positive `delta_x` means cursor moved right; positive `delta_y` means
    /// cursor moved down. The target shift keeps motion aligned to screen axes.
    pub fn pan_pixels(&mut self, delta_x: f32, delta_y: f32) {
        let scale = self.distance * 0.0015;
        self.target += (-self.right() * delta_x + self.up() * delta_y) * scale;
    }

    /// Set camera to an axial view (looking down Z axis).
    pub fn set_axial(&mut self) {
        self.azimuth = 0.0;
        self.elevation = 0.0;
    }

    /// Set camera to a coronal view (looking down Y axis from front).
    pub fn set_coronal(&mut self) {
        self.azimuth = 0.0;
        self.elevation = -std::f32::consts::FRAC_PI_2 + 0.01;
    }

    /// Set camera to a sagittal view (looking from the side).
    pub fn set_sagittal(&mut self) {
        self.azimuth = std::f32::consts::FRAC_PI_2;
        self.elevation = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_camera_looks_at_center() {
        let cam = Camera::default();
        assert_eq!(cam.target, Vec3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn position_at_default_angles() {
        let cam = Camera::default();
        let pos = cam.position();
        // Camera orbits target; verify position is at expected distance.
        let dist = (pos - cam.target).length();
        assert!((dist - cam.distance).abs() < 1e-4);
    }

    #[test]
    fn forward_points_toward_target() {
        let cam = Camera::default();
        let fwd = cam.forward();
        // Should point in -Z direction from default position
        assert!(fwd.z < 0.0);
    }

    #[test]
    fn zoom_clamps() {
        let mut cam = Camera::default();
        cam.zoom(100.0); // extreme zoom in
        assert!(cam.distance >= 0.5);
        cam.zoom(0.001); // extreme zoom out
        assert!(cam.distance <= 5.0);
    }

    #[test]
    fn rotation_continuous_elevation() {
        let mut cam = Camera::default();
        let start = cam.elevation;
        cam.rotate(0.0, std::f32::consts::PI);
        assert!((cam.elevation - (start + std::f32::consts::PI)).abs() < 1e-6);
    }

    #[test]
    fn pan_changes_target() {
        let mut cam = Camera::default();
        let start = cam.target;
        cam.pan_pixels(10.0, -8.0);
        assert_ne!(cam.target, start);
    }
}
