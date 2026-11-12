//! Responsive layout selection.
//!
//! The layout follows the logical size of the window or browser canvas, never a
//! platform flag: a narrow desktop window, a phone browser and a native phone
//! build all get the same answer for the same size.

/// Narrower than this and the three desktop panels cannot coexist.
const PORTRAIT_MAX_WIDTH: f32 = 640.0;
/// Shorter than this (and wide enough for side-by-side panes) the desktop
/// bottom shelf would leave no room for the volume.
const LANDSCAPE_MAX_HEIGHT: f32 = 520.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Library, 3D view and slice inspector side by side with a controls shelf.
    #[default]
    Desktop,
    /// Narrow and tall: volume on top, a tabbed sheet and tab bar at the bottom.
    PhonePortrait,
    /// Wide and short: volume on the left, tabbed panel on the right.
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
}

/// The areas a compact layout offers in place of the desktop's three panels.
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
