//! Responsive layout selection.
//!
//! The layout follows the logical size of the window or browser canvas, never a
//! platform flag: a narrow desktop window, a phone browser and a native phone
//! build all get the same answer for the same size.

/// Narrower than this and the three desktop panels cannot coexist.
const PORTRAIT_MAX_WIDTH: f32 = 640.0;
/// Shorter than this (and wide enough for side-by-side panes) the desktop
/// controls shelf would leave no room for the volume.
const LANDSCAPE_MAX_HEIGHT: f32 = 520.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Study, 3D view and 2D slice side by side, each with its own controls.
    #[default]
    Desktop,
    /// Narrow and tall: the selected view on top, its controls in a sheet
    /// above the bottom tab bar.
    PhonePortrait,
    /// Wide and short: the selected view on the left, its controls on the
    /// right, the same tab bar along the bottom.
    PhoneLandscape,
}

impl Layout {
    pub(crate) fn from_size(size: egui::Vec2) -> Self {
        if size.x < PORTRAIT_MAX_WIDTH {
            Layout::PhonePortrait
        } else if size.y < LANDSCAPE_MAX_HEIGHT {
            Layout::PhoneLandscape
        } else {
            Layout::Desktop
        }
    }

    /// Touch-sized controls and one-column content.
    pub(crate) fn is_compact(self) -> bool {
        self != Layout::Desktop
    }

    /// Space kept clear under bottom bars for the phone's home indicator.
    pub(crate) fn home_indicator_inset(self) -> f32 {
        match self {
            Layout::Desktop => 0.0,
            Layout::PhonePortrait => 34.0,
            Layout::PhoneLandscape => 21.0,
        }
    }
}

/// The destinations of the compact layouts' tab bar. The two views each take
/// the main area when selected; study and detections are full pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhoneTab {
    Study,
    Detections,
    Slice,
    View3d,
}

impl PhoneTab {
    pub(crate) const ALL: [PhoneTab; 4] = [
        PhoneTab::Study,
        PhoneTab::Detections,
        PhoneTab::Slice,
        PhoneTab::View3d,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            PhoneTab::Study => "Study",
            PhoneTab::Detections => "Detections",
            PhoneTab::Slice => "2D slice",
            PhoneTab::View3d => "3D view",
        }
    }

    /// The two views have a controls sheet; the pages do not.
    pub(crate) fn is_view(self) -> bool {
        matches!(self, PhoneTab::Slice | PhoneTab::View3d)
    }
}

/// Pages of the 3D controls sheet on compact layouts. Each page fits the sheet
/// without scrolling, so a slider drag is never mistaken for a scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VolumePage {
    Camera,
    Slice,
    Windowing,
    Display,
    Lighting,
}

impl VolumePage {
    pub(crate) const ALL: [VolumePage; 5] = [
        VolumePage::Camera,
        VolumePage::Slice,
        VolumePage::Windowing,
        VolumePage::Display,
        VolumePage::Lighting,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            VolumePage::Camera => "Camera",
            VolumePage::Slice => "Slice",
            VolumePage::Windowing => "Windowing",
            VolumePage::Display => "Display",
            VolumePage::Lighting => "Lighting",
        }
    }
}

/// Pages of the 2D controls sheet on compact layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlicePage {
    Slice,
    Image,
    Volume,
}

impl SlicePage {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SlicePage::Slice => "Slice",
            SlicePage::Image => "Image",
            SlicePage::Volume => "Volume",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_follows_logical_size() {
        for (w, h, expected) in [
            (1440.0, 900.0, Layout::Desktop),
            (1100.0, 720.0, Layout::Desktop),
            (393.0, 852.0, Layout::PhonePortrait),
            (852.0, 393.0, Layout::PhoneLandscape),
            (390.0, 300.0, Layout::PhonePortrait),
            (1024.0, 768.0, Layout::Desktop),
        ] {
            assert_eq!(Layout::from_size(egui::vec2(w, h)), expected, "{w}x{h}");
        }
    }
}
