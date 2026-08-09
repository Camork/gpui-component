//! Viewport (zoom / pan) math for the flame graph time axis.

pub const MIN_UNIT_SEC: f64 = 1e-9; // 1 ns, the smallest time unit we render.
pub const MIN_DURATION: f64 = 2.0 * MIN_UNIT_SEC; // 2 ns, guaranteeing at least 3 ticks (start, mid, end) at max zoom.

/// The currently visible time window. Purely numeric so the math is unit
/// testable without any gpui plumbing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub start: f64,
    pub end: f64,
    pub session_end: f64,
}

impl Viewport {
    pub fn full(session_end: f64) -> Self {
        let end = session_end.max(MIN_DURATION);
        Self {
            start: 0.0,
            end,
            session_end: end,
        }
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    pub fn x_to_time(&self, x: f64, width: f64) -> f64 {
        self.start + (x / width.max(1.0)) * self.duration()
    }

    pub fn time_to_x(&self, t: f64, width: f64) -> f64 {
        (t - self.start) / self.duration() * width
    }

    /// Pan by a pixel delta (`dx_px` positive moves forward in time, matching
    /// the sign of a vertical scroll wheel: wheel-down reveals later content).
    pub fn pan_pixels(&mut self, dx_px: f64, width: f64) {
        self.shift(dx_px / width.max(1.0) * self.duration());
    }

    pub fn shift(&mut self, dt: f64) {
        let dur = self.duration();
        let max_start = (self.session_end - dur).max(0.0);
        let mut new_start = (self.start + dt).clamp(0.0, max_start);
        if (dur - MIN_DURATION).abs() < 1e-12 {
            new_start = snap_to_grid(new_start, MIN_UNIT_SEC).clamp(0.0, max_start);
        }
        self.start = new_start;
        self.end = self.start + dur;
    }

    /// Set the viewport start time while preserving the current duration.
    pub fn set_start(&mut self, start_time: f64) {
        let dur = self.duration();
        let max_start = (self.session_end - dur).max(0.0);
        let mut new_start = start_time.clamp(0.0, max_start);
        if (dur - MIN_DURATION).abs() < 1e-12 {
            new_start = snap_to_grid(new_start, MIN_UNIT_SEC).clamp(0.0, max_start);
        }
        self.start = new_start;
        self.end = self.start + dur;
    }

    /// Zoom by `factor` (< 1 zooms in) anchored at the pixel position `x_px`,
    /// clamping the window into `[0, session_end]` and to a minimum duration.
    pub fn zoom_at_pixels(&mut self, x_px: f64, width: f64, factor: f64) {
        let anchor = self.x_to_time(x_px, width);
        let max_dur = self.session_end.max(MIN_DURATION);
        let new_dur = (self.duration() * factor).clamp(MIN_DURATION, max_dur);
        let frac = (x_px / width.max(1.0)).clamp(0.0, 1.0);
        let mut new_start = anchor - frac * new_dur;
        let max_start = (max_dur - new_dur).max(0.0);
        new_start = new_start.clamp(0.0, max_start);
        if (new_dur - MIN_DURATION).abs() < 1e-12 {
            new_start = snap_to_grid(new_start, MIN_UNIT_SEC).clamp(0.0, max_start);
        }
        self.start = new_start;
        self.end = new_start + new_dur;
    }

    /// Shrink the window to `[a, b]` (the drag-released zoom target). This is
    /// a *shrink-only* operation: it refuses to expand the current view, so a
    /// drag zoom can frame a region but never zoom out past the current window.
    pub fn set_range(&mut self, a: f64, b: f64) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let dur = b - a;
        if dur <= 0.0 || dur >= self.duration() {
            return;
        }
        self.zoom_to_range(a, b);
    }

    /// Rescale the viewport to display `[a, b]`. Unlike [`set_range`], this method
    /// supports both zooming in (when `[a, b]` is smaller than the current window)
    /// and zooming out (when `[a, b]` is larger than the current window).
    pub fn zoom_to_range(&mut self, a: f64, b: f64) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let dur = b - a;
        if dur <= 0.0 {
            return;
        }
        let max_dur = self.session_end.max(MIN_DURATION);
        let new_dur = dur.clamp(MIN_DURATION, max_dur);
        let max_start = (self.session_end - new_dur).max(0.0);
        let mut new_start = a.clamp(0.0, max_start);
        if (new_dur - MIN_DURATION).abs() < 1e-12 {
            new_start = snap_to_grid(new_start, MIN_UNIT_SEC).clamp(0.0, max_start);
        }
        self.start = new_start;
        self.end = new_start + new_dur;
    }

    pub fn reset(&mut self) {
        *self = Self::full(self.session_end);
    }
}

/// Round a time to the nearest multiple of `grid` seconds (a `TimeUnit`'s
/// quantum). Used to snap the persistent `t1` marker onto the sub-second
/// ladder so cursor+duration readouts don't show uncontrolled decimals.
pub(crate) fn snap_to_grid(t: f64, grid: f64) -> f64 {
    if grid <= 0.0 || t.is_nan() || t.is_infinite() {
        t
    } else {
        (t / grid).round() * grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_round_trips_x_and_time() {
        let vp = Viewport::full(10.0);
        let width = 800.0;
        for x in [0.0, 123.0, 400.0, 799.0] {
            let t = vp.x_to_time(x, width);
            let back = vp.time_to_x(t, width);
            assert!((back - x).abs() < 1e-6, "x={x} back={back}");
        }
    }

    #[test]
    fn zoom_keeps_anchor_and_clamps_to_session() {
        let mut vp = Viewport::full(10.0);
        let width = 800.0;
        let anchor_t = vp.x_to_time(600.0, width);
        vp.zoom_at_pixels(600.0, width, 0.5);
        let anchor_after = vp.x_to_time(600.0, width);
        assert!((anchor_after - anchor_t).abs() < 1e-9);
        assert!((vp.duration() - 5.0).abs() < 1e-9);
        assert!(vp.start >= 0.0 && vp.end <= 10.0 + 1e-9);

        // zooming out far at the left edge still clamps into the session
        let mut vp = Viewport::full(1.0);
        vp.zoom_at_pixels(0.0, width, 100.0);
        assert!(vp.start >= -1e-9 && vp.end <= 1.0 + 1e-9);
    }

    #[test]
    fn max_zoom_ns_limit_has_at_least_three_ticks_at_start_mid_end() {
        let mut vp = Viewport::full(1.0);
        let width = 800.0;
        for _ in 0..50 {
            vp.zoom_at_pixels(400.0, width, 0.1);
        }
        assert!((vp.duration() - MIN_DURATION).abs() < 1e-12);
        assert!((vp.duration() - 2e-9).abs() < 1e-12);

        let rem = (vp.start / MIN_UNIT_SEC).round() * MIN_UNIT_SEC - vp.start;
        assert!(rem.abs() < 1e-12, "vp.start={}", vp.start);

        let step = MIN_UNIT_SEC;
        let mut t = (vp.start / step).floor() * step;
        let mut ticks = Vec::new();
        while t <= vp.end + step * 0.5 {
            let x = vp.time_to_x(t, width as f64);
            if x >= -10.0 && x <= width as f64 + 10.0 {
                ticks.push((t, x as f32));
            }
            t += step;
        }

        assert_eq!(ticks.len(), 3, "Expected 3 ticks at max zoom");
        assert!((ticks[0].1 - 0.0).abs() < 1e-3, "First tick at start (x=0)");
        assert!((ticks[1].1 - 400.0).abs() < 1e-3, "Second tick at middle (x=400)");
        assert!((ticks[2].1 - 800.0).abs() < 1e-3, "Third tick at end (x=800)");
    }

    #[test]
    fn pan_clamps_to_session_bounds() {
        let mut vp = Viewport::full(10.0);
        vp.pan_pixels(-10000.0, 800.0); // far before the start
        assert_eq!(vp.start, 0.0);
        vp.pan_pixels(100000.0, 800.0); // far past the end
        assert!(vp.end <= 10.0 + 1e-9);
        assert_eq!(vp.duration(), 10.0);
    }

    #[test]
    fn snap_to_grid_rounds_to_the_quantum() {
        let ns = 1e-9;
        assert_eq!(snap_to_grid(1.2345e-9, ns), 1e-9);
        assert_eq!(snap_to_grid(0.6e-9, ns), 1e-9);
        assert_eq!(snap_to_grid(0.4e-9, ns), 0.0);
        assert_eq!(snap_to_grid(0.0, ns), 0.0);
        let us = 1e-6;
        let snapped = snap_to_grid(14.7e-6, us);
        assert!((snapped - 15e-6).abs() < 1e-12);
    }

    #[test]
    fn zoom_to_range_zooms_in_and_out() {
        let mut vp = Viewport::full(10.0);
        
        // Zoom in to [2.0, 4.0] from full [0.0, 10.0]
        vp.zoom_to_range(2.0, 4.0);
        assert_eq!(vp.start, 2.0);
        assert_eq!(vp.end, 4.0);
        assert_eq!(vp.duration(), 2.0);

        // Zoom out to [1.0, 8.0] from current [2.0, 4.0]
        vp.zoom_to_range(1.0, 8.0);
        assert_eq!(vp.start, 1.0);
        assert_eq!(vp.end, 8.0);
        assert_eq!(vp.duration(), 7.0);
    }
}
