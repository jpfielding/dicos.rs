//! Touch gestures for the 3D viewport.
//!
//! Touchscreens have no hover, wheel, or Shift key, so the camera needs its own
//! mapping: one finger orbits, two fingers pinch to zoom and drag to pan. The
//! tracker is plain data (physical pixels in, camera operations out) so it can be
//! tested without a window.

/// A camera change produced by a touch gesture. Deltas are physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gesture {
    /// One finger dragged.
    Orbit { dx: f32, dy: f32 },
    /// Two fingers moved together.
    Pan { dx: f32, dy: f32 },
    /// Two fingers moved apart (`> 1`) or together (`< 1`).
    Zoom(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Started,
    Moved,
    Ended,
}

/// Follows up to two fingers that began a gesture inside the 3D viewport.
#[derive(Debug, Default)]
pub(crate) struct TouchTracker {
    points: Vec<(u64, [f32; 2])>,
}

impl TouchTracker {
    /// Feed one touch event. `inside_viewport` only matters for the first finger:
    /// a drag that starts on a panel must never move the camera.
    pub(crate) fn update(
        &mut self,
        id: u64,
        phase: Phase,
        pos: [f32; 2],
        inside_viewport: bool,
    ) -> Vec<Gesture> {
        match phase {
            Phase::Started => {
                let accept = if self.points.is_empty() {
                    inside_viewport
                } else {
                    self.points.len() < 2
                };
                if accept && !self.points.iter().any(|(i, _)| *i == id) {
                    self.points.push((id, pos));
                }
                Vec::new()
            }
            Phase::Moved => {
                let Some(index) = self.points.iter().position(|(i, _)| *i == id) else {
                    return Vec::new();
                };
                let before = self.points.clone();
                self.points[index].1 = pos;
                match (before.len(), &self.points[..]) {
                    (1, [(_, new)]) => {
                        let old = before[0].1;
                        vec![Gesture::Orbit {
                            dx: new[0] - old[0],
                            dy: new[1] - old[1],
                        }]
                    }
                    (2, [(_, a1), (_, b1)]) => {
                        let (a0, b0) = (before[0].1, before[1].1);
                        let (d0, d1) = (distance(a0, b0), distance(*a1, *b1));
                        let (m0, m1) = (midpoint(a0, b0), midpoint(*a1, *b1));
                        let mut out = vec![Gesture::Pan {
                            dx: m1[0] - m0[0],
                            dy: m1[1] - m0[1],
                        }];
                        if d0 > 8.0 && d1 > 8.0 {
                            out.push(Gesture::Zoom(d1 / d0));
                        }
                        out
                    }
                    _ => Vec::new(),
                }
            }
            Phase::Ended => {
                self.points.retain(|(i, _)| *i != id);
                Vec::new()
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_active(&self) -> bool {
        !self.points.is_empty()
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_finger_orbits_only_when_it_starts_in_the_viewport() {
        let mut t = TouchTracker::default();
        t.update(1, Phase::Started, [10.0, 10.0], false);
        assert!(!t.is_active());
        assert!(t.update(1, Phase::Moved, [20.0, 10.0], true).is_empty());

        t.update(2, Phase::Started, [100.0, 100.0], true);
        assert_eq!(
            t.update(2, Phase::Moved, [110.0, 95.0], true),
            vec![Gesture::Orbit { dx: 10.0, dy: -5.0 }]
        );
        t.update(2, Phase::Ended, [110.0, 95.0], true);
        assert!(!t.is_active());
    }

    #[test]
    fn two_fingers_pinch_to_zoom_and_pan_together() {
        let mut t = TouchTracker::default();
        t.update(1, Phase::Started, [100.0, 100.0], true);
        // The second finger may land anywhere once a gesture is under way.
        t.update(2, Phase::Started, [200.0, 100.0], false);
        let spread = t.update(2, Phase::Moved, [300.0, 100.0], true);
        assert!(spread.contains(&Gesture::Zoom(2.0)));
        assert!(spread.contains(&Gesture::Pan { dx: 50.0, dy: 0.0 }));

        let slide = t.update(1, Phase::Moved, [100.0, 120.0], true);
        assert!(slide
            .iter()
            .any(|g| matches!(g, Gesture::Pan { dy, .. } if *dy == 10.0)));
    }

    #[test]
    fn a_third_finger_is_ignored() {
        let mut t = TouchTracker::default();
        t.update(1, Phase::Started, [0.0, 0.0], true);
        t.update(2, Phase::Started, [50.0, 0.0], true);
        t.update(3, Phase::Started, [90.0, 0.0], true);
        assert!(t.update(3, Phase::Moved, [99.0, 0.0], true).is_empty());
    }
}
