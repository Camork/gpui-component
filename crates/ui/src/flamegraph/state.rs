//! State management for the flame graph component.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Context, Pixels, Point, Size, point, px, size};

use crate::scroll::ScrollbarHandle;

use super::data::{FlatSession, Session, flatten};
use super::format::TimeFormatConfig;
use super::style::{FlameGraphStyle, default_time_format};
use super::viewport::{Viewport, snap_to_grid};

/// Which frame a click/hover/selection refers to: `(track, flat index)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameId {
    pub track: usize,
    pub flat: usize,
}

/// An in-progress box-select drag (content coordinates).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DragState {
    pub(crate) origin: Point<f32>,
    pub(crate) current: Point<f32>,
    /// True until the pointer moves beyond the click threshold; decides
    /// click-vs-drag on mouse-up.
    pub(crate) pending_click: bool,
    /// Track whose header the mouse went down on, if any.
    pub(crate) down_header: Option<usize>,
    /// Frame the mouse went down on, if any.
    pub(crate) down_frame: Option<FrameId>,
}

/// Shared between the view, the canvas, and the overlay scrollbar.
#[derive(Default)]
pub(crate) struct ScrollState {
    pub(crate) content_h: f32,
    pub(crate) viewport_h: f32,
    pub(crate) scroll_y: f32,
}

#[derive(Clone)]
pub(crate) struct FlameScrollHandle(pub(crate) Rc<RefCell<ScrollState>>);

impl ScrollbarHandle for FlameScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        point(px(0.0), px(-self.0.borrow().scroll_y))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut s = self.0.borrow_mut();
        let overflow = (s.content_h - s.viewport_h).max(0.0);
        s.scroll_y = (-offset.y.as_f32()).clamp(0.0, overflow.max(0.0));
    }

    fn content_size(&self) -> Size<Pixels> {
        let s = self.0.borrow();
        size(px(s.viewport_h.max(0.0)), px(s.content_h.max(0.0)))
    }
}

pub struct FlameGraphState {
    pub(crate) session: Session,
    pub(crate) flat: FlatSession,
    pub(crate) viewport: Viewport,
    pub(crate) search: String,
    pub(crate) search_lower: String,
    pub(crate) selected: Option<FrameId>,
    pub(crate) hovered: Option<FrameId>,
    pub(crate) drag: Option<DragState>,
    /// Persistent marker time (seconds, relative to viewport start). Set on
    /// every left press; a drag moves it to the drag's left edge; snapped to
    /// the `TimeFormat` grid on release.
    pub(crate) t1: f64,
    pub(crate) scroll: FlameScrollHandle,
    pub(crate) style: FlameGraphStyle,
    pub(crate) time_format: TimeFormatConfig,
}

impl FlameGraphState {
    /// Create with the given session data.
    pub fn new(session: Session) -> Self {
        let flat = flatten(&session);
        let viewport = Viewport::full(flat.session_end);

        Self {
            session,
            flat,
            viewport,
            search: String::new(),
            search_lower: String::new(),
            selected: None,
            hovered: None,
            drag: None,
            t1: 0.0,
            scroll: FlameScrollHandle(Rc::new(RefCell::new(ScrollState::default()))),
            style: FlameGraphStyle::default(),
            time_format: default_time_format(),
        }
    }

    /// Replace the data source.
    pub fn set_session(&mut self, session: Session, cx: &mut Context<Self>) {
        self.flat = flatten(&session);
        self.session = session;
        self.viewport = Viewport::full(self.flat.session_end);
        self.selected = None;
        self.hovered = None;
        self.drag = None;
        self.t1 = 0.0;
        self.scroll.0.borrow_mut().scroll_y = 0.0;
        cx.notify();
    }

    /// Set the search filter text.
    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let query_str = query.to_string();
        if self.search != query_str {
            self.search_lower = query.to_lowercase();
            self.search = query_str;
            cx.notify();
        }
    }

    /// Read the current viewport.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Reset viewport to full range.
    pub fn reset_viewport(&mut self, cx: &mut Context<Self>) {
        self.viewport = Viewport::full(self.flat.session_end);
        cx.notify();
    }

    /// Get the currently selected frame.
    pub fn selected(&self) -> Option<FrameId> {
        self.selected
    }

    /// Get the session data.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Get the flattened session.
    pub fn flat(&self) -> &FlatSession {
        &self.flat
    }

    /// Get the current style configuration.
    pub fn style(&self) -> &FlameGraphStyle {
        &self.style
    }

    /// Set custom style.
    pub fn set_style(&mut self, style: FlameGraphStyle) {
        self.style = style;
    }

    /// Get the current time format configuration.
    pub fn time_format(&self) -> &TimeFormatConfig {
        &self.time_format
    }

    /// Set time format configuration.
    pub fn set_time_format(&mut self, config: TimeFormatConfig) {
        self.time_format = config;
    }

    // ── internal methods ──────────────────────────────────────────────────

    pub(crate) fn commit_t1(&mut self) {
        let ns = self.time_format.min_unit.seconds();
        self.t1 = snap_to_grid(self.t1, ns).clamp(0.0, self.flat.session_end);
    }

    pub(crate) fn toggle_collapse(&mut self, track: usize, cx: &mut Context<Self>) {
        if track >= self.session.tracks.len() {
            return;
        }
        let collapsed = &mut self.session.tracks[track].collapsed;
        *collapsed = !*collapsed;
        if *collapsed && self.selected.is_some_and(|f| f.track == track) {
            self.selected = None;
        }
        cx.notify();
    }

    pub(crate) fn scroll_y(&self) -> f32 {
        self.scroll.0.borrow().scroll_y
    }

    pub(crate) fn scroll_by_y(&mut self, dy: f32) {
        let mut scroll = self.scroll.0.borrow_mut();
        let overflow = (scroll.content_h - scroll.viewport_h).max(0.0);
        scroll.scroll_y = (scroll.scroll_y + dy).clamp(0.0, overflow);
    }

    pub(crate) fn track_collapsed(&self, track: usize) -> bool {
        self.session.tracks.get(track).is_some_and(|t| t.collapsed)
    }

    /// (header_y, rows_y, rows_h) in content coordinates for track `track`.
    pub(crate) fn track_geometry(&self, track: usize) -> (f32, f32, f32) {
        let collapsed: Vec<bool> = self.session.tracks.iter().map(|t| t.collapsed).collect();
        let depths: Vec<usize> = self.flat.tracks.iter().map(|t| t.max_depth).collect();
        Self::track_layout_y(
            &collapsed,
            &depths,
            track,
            self.style.track_header_height,
            self.style.frame_row_height,
        )
    }

    /// Pure geometry helper: y of each track region from collapse flags and
    /// per-track nesting depth. Exposed for unit tests.
    pub(crate) fn track_layout_y(
        collapsed: &[bool],
        max_depths: &[usize],
        track: usize,
        header_h: f32,
        row_h: f32,
    ) -> (f32, f32, f32) {
        let mut y = 0.0f32;
        for i in 0..track {
            y += header_h;
            if !collapsed.get(i).copied().unwrap_or(false) {
                y += (max_depths[i] + 1) as f32 * row_h;
            }
        }
        let hy = y;
        let ry = hy + header_h;
        let rh = if collapsed.get(track).copied().unwrap_or(false) {
            0.0
        } else {
            (max_depths[track] + 1) as f32 * row_h
        };
        (hy, ry, rh)
    }

    pub(crate) fn content_height(&self) -> f32 {
        let collapsed: Vec<bool> = self.session.tracks.iter().map(|t| t.collapsed).collect();
        let depths: Vec<usize> = self.flat.tracks.iter().map(|t| t.max_depth).collect();
        Self::content_height_for(
            &collapsed,
            &depths,
            self.style.track_header_height,
            self.style.frame_row_height,
        )
    }

    pub(crate) fn content_height_for(
        collapsed: &[bool],
        max_depths: &[usize],
        header_h: f32,
        row_h: f32,
    ) -> f32 {
        let mut y = 0.0f32;
        for i in 0..collapsed.len() {
            y += header_h;
            if !collapsed[i] {
                y += (max_depths[i] + 1) as f32 * row_h;
            }
        }
        y
    }

    pub(crate) fn time_to_x(&self, t: f64, width: f32) -> f32 {
        self.viewport.time_to_x(t, width as f64) as f32
    }

    /// Find the track whose *header* contains content-y `y`.
    pub(crate) fn header_at(&self, y: f32) -> Option<usize> {
        for i in 0..self.flat.tracks.len() {
            let (hy, _, _) = self.track_geometry(i);
            if y >= hy && y < hy + self.style.track_header_height {
                return Some(i);
            }
        }
        None
    }

    /// Find the frame whose content point `(x, y)` contains (x in px, y in
    /// content px). Pure, so the exact hover-hit logic can be unit-tested.
    pub(crate) fn frame_at(&self, x: f32, y: f32, width: f32) -> Option<FrameId> {
        let collapsed = self
            .session
            .tracks
            .iter()
            .map(|t| t.collapsed)
            .collect::<Vec<_>>();
        Self::frame_hit(
            &self.flat,
            &self.viewport,
            &collapsed,
            x,
            y,
            width,
            self.style.track_header_height,
            self.style.frame_row_height,
        )
    }

    /// Shared by `frame_at` and the tests; also mirrors the mapping used to
    /// draw the hover outline, so outline and hit test can never disagree.
    pub(crate) fn frame_hit(
        flat: &FlatSession,
        viewport: &Viewport,
        collapsed: &[bool],
        x: f32,
        y: f32,
        width: f32,
        header_h: f32,
        row_h: f32,
    ) -> Option<FrameId> {
        for (ti, track) in flat.tracks.iter().enumerate() {
            if collapsed.get(ti).copied().unwrap_or(false) {
                continue;
            }
            let depths: Vec<usize> = flat.tracks.iter().map(|t| t.max_depth).collect();
            let (_, ry, rh) = Self::track_layout_y(collapsed, &depths, ti, header_h, row_h);
            if y < ry || y >= ry + rh {
                continue;
            }
            let row = ((y - ry) / row_h) as usize;
            if row > track.max_depth {
                return None;
            }
            let t = viewport.x_to_time(x as f64, width as f64);
            let eps = 1e-12;
            for (fi, f) in track.frames.iter().enumerate() {
                if f.row == row && t >= f.abs_start - eps && t <= f.abs_end + eps {
                    return Some(FrameId { track: ti, flat: fi });
                }
            }
            return None;
        }
        None
    }

    pub(crate) fn frame_matched(&self, name: &str) -> bool {
        self.search_lower.is_empty() || name.to_lowercase().contains(&self.search_lower)
    }
}
