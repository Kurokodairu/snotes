//! Hold-to-snap detection: a pen held still at the end of a roughly straight
//! stroke turns the stroke into a straight line (OneNote behaviour).
//! Pure logic over pen samples, no engine or GTK types.

use crate::engine::PenSample;
use std::time::{Duration, Instant};

/// The pen counts as still while it stays within this distance of where it stopped.
pub const SNAP_HOLD_RADIUS_PX: f64 = 3.0;
/// How long the pen must stay still before the stroke snaps.
pub const SNAP_HOLD_MS: u64 = 400;
/// Largest allowed deviation from the start-to-end chord, as a fraction of the chord length.
pub const SNAP_STRAIGHTNESS: f64 = 0.08;
/// Strokes whose chord is shorter than this never snap (a held dot is not a line).
const SNAP_MIN_CHORD_PX: f64 = 10.0;

#[derive(Debug)]
pub enum SnapEvent {
    None,
    /// The stroke snapped; the line starts at `start`.
    Snapped {
        start: PenSample,
    },
}

#[derive(Debug, Default)]
pub struct SnapDetector {
    path: Vec<PenSample>,
    /// Where and when the pen last stopped moving.
    anchor: Option<(PenSample, Instant)>,
    snapped: bool,
}

fn dist(a: PenSample, b: PenSample) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

impl SnapDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the current stroke. Call on every pen-up and proximity loss.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Feed the next sample of the stroke that is currently down.
    pub fn push(&mut self, sample: PenSample, at: Instant) -> SnapEvent {
        if self.snapped {
            return SnapEvent::None;
        }
        self.path.push(sample);
        let since = match self.anchor {
            Some((a, t)) if dist(a, sample) <= SNAP_HOLD_RADIUS_PX => t,
            _ => {
                self.anchor = Some((sample, at));
                return SnapEvent::None;
            }
        };
        if at.saturating_duration_since(since) < Duration::from_millis(SNAP_HOLD_MS) {
            return SnapEvent::None;
        }
        let start = self.path[0];
        let chord = dist(start, sample);
        if chord < SNAP_MIN_CHORD_PX {
            return SnapEvent::None;
        }
        // Perpendicular distance of every point from the start-to-end chord.
        let (dx, dy) = ((sample.x - start.x) / chord, (sample.y - start.y) / chord);
        let max_dev = self
            .path
            .iter()
            .map(|p| ((p.x - start.x) * dy - (p.y - start.y) * dx).abs())
            .fold(0.0, f64::max);
        // Position of every point along the chord: the stroke must not run past
        // either end (out and back along one line is not a line).
        let (min_t, max_t) = self
            .path
            .iter()
            .map(|p| (p.x - start.x) * dx + (p.y - start.y) * dy)
            .fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let slack = SNAP_STRAIGHTNESS * chord;
        if max_dev > slack || min_t < -slack || max_t > chord + slack {
            return SnapEvent::None;
        }
        self.snapped = true;
        SnapEvent::Snapped { start }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f64, y: f64) -> PenSample {
        PenSample {
            x,
            y,
            pressure: 0.5,
        }
    }

    /// Push `pts` 10 ms apart from `t0`; returns the time after the last one.
    fn drive(d: &mut SnapDetector, t0: Instant, pts: &[(f64, f64)]) -> (Instant, Vec<bool>) {
        let mut t = t0;
        let mut snaps = Vec::new();
        for &(x, y) in pts {
            snaps.push(matches!(d.push(s(x, y), t), SnapEvent::Snapped { .. }));
            t += Duration::from_millis(10);
        }
        (t, snaps)
    }

    fn line(n: usize, wobble: f64) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let w = if i % 2 == 0 { wobble } else { -wobble };
                (i as f64 * 5.0, w)
            })
            .collect()
    }

    fn hold(d: &mut SnapDetector, t: Instant, at: (f64, f64), ms: u64) -> SnapEvent {
        d.push(s(at.0, at.1), t + Duration::from_millis(ms))
    }

    #[test]
    fn steady_straight_line_snaps_after_hold() {
        let mut d = SnapDetector::new();
        let pts = line(21, 0.0);
        let (t, snaps) = drive(&mut d, Instant::now(), &pts);
        assert!(!snaps.iter().any(|&b| b));
        let end = *pts.last().unwrap();
        let t_stop = t - Duration::from_millis(10);
        assert!(matches!(hold(&mut d, t_stop, end, 200), SnapEvent::None));
        assert!(matches!(
            hold(&mut d, t_stop, end, 400),
            SnapEvent::Snapped { start } if start.x == 0.0
        ));
    }

    #[test]
    fn wobbly_line_does_not_snap() {
        let mut d = SnapDetector::new();
        let pts = line(21, 15.0);
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t - Duration::from_millis(10), end, 500),
            SnapEvent::None
        ));
    }

    #[test]
    fn curve_does_not_snap() {
        let mut d = SnapDetector::new();
        let pts: Vec<_> = (0..=20)
            .map(|i| {
                let a = i as f64 / 20.0 * std::f64::consts::PI;
                (50.0 - 50.0 * a.cos(), 50.0 * a.sin())
            })
            .collect();
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t - Duration::from_millis(10), end, 500),
            SnapEvent::None
        ));
    }

    #[test]
    fn release_before_hold_time_does_not_snap() {
        let mut d = SnapDetector::new();
        let pts = line(21, 0.0);
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t - Duration::from_millis(10), end, SNAP_HOLD_MS - 1),
            SnapEvent::None
        ));
    }

    #[test]
    fn moving_pen_restarts_hold_timer() {
        let mut d = SnapDetector::new();
        let pts = line(21, 0.0);
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let t_stop = t - Duration::from_millis(10);
        let end = *pts.last().unwrap();
        // 300 ms still, then a 10 px move, then 300 ms still: no snap.
        assert!(matches!(hold(&mut d, t_stop, end, 300), SnapEvent::None));
        let moved = (end.0 + 10.0, end.1);
        assert!(matches!(hold(&mut d, t_stop, moved, 310), SnapEvent::None));
        assert!(matches!(hold(&mut d, t_stop, moved, 610), SnapEvent::None));
        assert!(matches!(
            hold(&mut d, t_stop, moved, 720),
            SnapEvent::Snapped { .. }
        ));
    }

    #[test]
    fn no_second_snap_until_reset() {
        let mut d = SnapDetector::new();
        let pts = line(21, 0.0);
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let t_stop = t - Duration::from_millis(10);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t_stop, end, 400),
            SnapEvent::Snapped { .. }
        ));
        assert!(matches!(hold(&mut d, t_stop, end, 900), SnapEvent::None));
    }

    #[test]
    fn held_without_motion_does_not_snap() {
        let mut d = SnapDetector::new();
        let t = Instant::now();
        assert!(matches!(d.push(s(5.0, 5.0), t), SnapEvent::None));
        assert!(matches!(hold(&mut d, t, (5.0, 5.0), 500), SnapEvent::None));
    }

    #[test]
    fn out_and_back_along_the_chord_does_not_snap() {
        let mut d = SnapDetector::new();
        let mut pts = line(61, 0.0);
        pts.extend((3..60).rev().map(|i| (i as f64 * 5.0, 0.0)));
        // Ends 15 px from where it started, after going 300 px out.
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t - Duration::from_millis(10), end, 500),
            SnapEvent::None
        ));
    }

    #[test]
    fn reset_clears_state() {
        let mut d = SnapDetector::new();
        let pts = line(21, 0.0);
        let (t, _) = drive(&mut d, Instant::now(), &pts);
        let t_stop = t - Duration::from_millis(10);
        let end = *pts.last().unwrap();
        assert!(matches!(
            hold(&mut d, t_stop, end, 400),
            SnapEvent::Snapped { .. }
        ));
        d.reset();
        // A new stroke starting at the same spot must not inherit the old anchor.
        assert!(matches!(hold(&mut d, t_stop, end, 450), SnapEvent::None));
        assert!(matches!(hold(&mut d, t_stop, end, 900), SnapEvent::None));
    }
}
