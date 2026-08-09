//! State management for the flame graph component.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Context, Pixels, Point, Size, point, px, size};

use crate::scroll::ScrollbarHandle;

use super::data::Session;
use super::format::TimeFormatConfig;
use super::source::{FlameDataSource, MemoryDataSource, QueryResult};
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

#[derive(Default)]
pub(crate) struct HScrollState {
    pub(crate) session_dur: f64,
    pub(crate) vp_start: f64,
    pub(crate) vp_dur: f64,
    pub(crate) canvas_w: f32,
    pub(crate) scroll_x_updated: bool,
    pub(crate) new_vp_start: f64,
}

#[derive(Clone)]
pub(crate) struct FlameHScrollHandle(pub(crate) Rc<RefCell<HScrollState>>);

impl ScrollbarHandle for FlameHScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        let s = self.0.borrow();
        let vp_dur = s.vp_dur;
        if s.session_dur <= 0.0 || vp_dur <= 0.0 || vp_dur >= s.session_dur {
            return Point::default();
        }
        let max_start = s.session_dur - vp_dur;
        let fraction = if max_start > 0.0 { (s.vp_start / max_start).clamp(0.0, 1.0) } else { 0.0 };
        let content_w = s.canvas_w as f64 * (s.session_dur / vp_dur);
        let max_scroll = (content_w - s.canvas_w as f64).max(0.0);
        let offset_x = -fraction * max_scroll;
        point(px(offset_x as f32), px(0.0))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut s = self.0.borrow_mut();
        let vp_dur = s.vp_dur;
        if s.session_dur <= 0.0 || vp_dur <= 0.0 || vp_dur >= s.session_dur {
            return;
        }
        let max_start = s.session_dur - vp_dur;
        let content_w = s.canvas_w as f64 * (s.session_dur / vp_dur);
        let max_scroll = (content_w - s.canvas_w as f64).max(1e-6);
        let scroll_px = (-offset.x.as_f32() as f64).clamp(0.0, max_scroll);
        let fraction = scroll_px / max_scroll;
        let clamped_start = fraction * max_start;

        s.vp_start = clamped_start;
        s.new_vp_start = clamped_start;
        s.scroll_x_updated = true;
    }

    fn content_size(&self) -> Size<Pixels> {
        let s = self.0.borrow();
        let vp_dur = s.vp_dur;
        let width = s.canvas_w.max(1.0);
        if s.session_dur <= 0.0 || vp_dur <= 0.0 || vp_dur >= s.session_dur {
            return size(px(width), px(1.0));
        }
        let content_w = (width as f64 * (s.session_dur / vp_dur)) as f32;
        size(px(content_w), px(1.0))
    }
}

pub struct FlameGraphState {
    /// The data source providing frames for the current viewport.
    source: Box<dyn FlameDataSource>,
    /// Cached query result from the last `refresh_query()` call.
    pub(crate) cached: QueryResult,
    /// Per-track collapsed state, managed by the component (not the source).
    pub(crate) collapsed: Vec<bool>,

    pub(crate) viewport: Viewport,
    pub(crate) search: String,
    pub(crate) search_lower: String,
    pub(crate) selected: Option<FrameId>,
    pub(crate) hovered: Option<FrameId>,
    pub(crate) drag: Option<DragState>,
    /// Persistent marker time (seconds, relative to viewport start). Set on
    /// every left press; a drag moves it to the left edge (`t2` stays
    /// transient), and releasing snaps to the `TimeFormat` grid.
    pub(crate) t1: f64,
    pub(crate) scroll: FlameScrollHandle,
    pub(crate) h_scroll: FlameHScrollHandle,
    pub(crate) style: FlameGraphStyle,
    pub(crate) time_format: TimeFormatConfig,
    /// Cached last-known canvas width for re-querying on viewport change.
    pub(crate) last_width: f32,
}

impl FlameGraphState {
    /// Create with the given session data (in-memory source).
    pub fn new(session: Session) -> Self {
        let source = MemoryDataSource::new(session);
        Self::with_source(Box::new(source))
    }

    /// Create with a custom data source.
    pub fn with_source(source: Box<dyn FlameDataSource>) -> Self {
        let session_end = source.session_duration();
        let track_count = source.track_count();
        let viewport = Viewport::full(session_end);
        let collapsed = vec![false; track_count];
        let cached = source.query_visible_frames(
            (viewport.start, viewport.end),
            1000.0,
            "",
        );
        let h_scroll = FlameHScrollHandle(Rc::new(RefCell::new(HScrollState {
            session_dur: session_end,
            vp_start: viewport.start,
            vp_dur: viewport.duration(),
            canvas_w: 1000.0,
            scroll_x_updated: false,
            new_vp_start: 0.0,
        })));

        Self {
            source,
            cached,
            collapsed,
            viewport,
            search: String::new(),
            search_lower: String::new(),
            selected: None,
            hovered: None,
            drag: None,
            t1: 0.0,
            scroll: FlameScrollHandle(Rc::new(RefCell::new(ScrollState::default()))),
            h_scroll,
            style: FlameGraphStyle::default(),
            time_format: default_time_format(),
            last_width: 1000.0,
        }
    }

    /// Replace the data source (in-memory convenience).
    pub fn set_session(&mut self, session: Session, cx: &mut Context<Self>) {
        let source = MemoryDataSource::new(session);
        self.set_source(Box::new(source), cx);
    }

    /// Replace the data source.
    pub fn set_source(&mut self, source: Box<dyn FlameDataSource>, cx: &mut Context<Self>) {
        let session_end = source.session_duration();
        let track_count = source.track_count();
        self.source = source;
        self.viewport = Viewport::full(session_end);
        self.collapsed = vec![false; track_count];
        self.selected = None;
        self.hovered = None;
        self.drag = None;
        self.t1 = 0.0;
        self.scroll.0.borrow_mut().scroll_y = 0.0;
        self.refresh_query();
        cx.notify();
    }

    /// Set the search filter text.
    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let query_str = query.to_string();
        if self.search != query_str {
            self.search_lower = query.to_lowercase();
            self.search = query_str;
            self.refresh_query();
            cx.notify();
        }
    }

    /// Read the current viewport.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Reset viewport to full range.
    pub fn reset_viewport(&mut self, cx: &mut Context<Self>) {
        self.viewport = Viewport::full(self.source.session_duration());
        self.refresh_query();
        cx.notify();
    }

    /// Get the currently selected frame.
    pub fn selected(&self) -> Option<FrameId> {
        self.selected
    }

    /// Access the underlying data source.
    pub fn source(&self) -> &dyn FlameDataSource {
        &*self.source
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

    /// Re-query the data source with the current viewport and search.
    pub(crate) fn refresh_query(&mut self) {
        self.cached = self.source.query_visible_frames(
            (self.viewport.start, self.viewport.end),
            self.last_width,
            &self.search,
        );
        self.sync_h_scroll();
    }

    pub(crate) fn sync_h_scroll(&mut self) {
        let mut s = self.h_scroll.0.borrow_mut();
        s.session_dur = self.source.session_duration();
        s.vp_start = self.viewport.start;
        s.vp_dur = self.viewport.duration();
        s.canvas_w = self.last_width;
    }

    pub(crate) fn check_h_scroll_drag(&mut self, width: f32, cx: &mut Context<Self>) {
        self.last_width = width;
        let pending = {
            let mut s = self.h_scroll.0.borrow_mut();
            s.canvas_w = width;
            if s.scroll_x_updated {
                s.scroll_x_updated = false;
                Some((s.new_vp_start, s.vp_dur))
            } else {
                None
            }
        };

        if let Some((start, _dur)) = pending {
            self.viewport.set_start(start);
            self.refresh_query();
            cx.notify();
        } else {
            self.sync_h_scroll();
        }
    }

    // ── internal methods ──────────────────────────────────────────────────

    pub(crate) fn commit_t1(&mut self) {
        let ns = self.time_format.min_unit.seconds();
        self.t1 = snap_to_grid(self.t1, ns).clamp(0.0, self.source.session_duration());
    }

    pub(crate) fn toggle_collapse(&mut self, track: usize, cx: &mut Context<Self>) {
        if track >= self.collapsed.len() {
            return;
        }
        self.collapsed[track] = !self.collapsed[track];
        if self.collapsed[track] && self.selected.is_some_and(|f| f.track == track) {
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
        self.collapsed.get(track).copied().unwrap_or(false)
    }

    /// (header_y, rows_y, rows_h) in content coordinates for track `track`.
    pub(crate) fn track_geometry(&self, track: usize) -> (f32, f32, f32) {
        let depths: Vec<usize> = self.cached.tracks.iter().map(|t| t.info.max_depth).collect();
        Self::track_layout_y(
            &self.collapsed,
            &depths,
            track,
            self.style.track_header_height,
            self.style.frame_row_height,
        )
    }

    /// Pure geometry helper: y of each track region from collapse flags and
    /// per-track nesting depth. Exposed for unit tests.
    pub fn track_layout_y(
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
        let depths: Vec<usize> = self.cached.tracks.iter().map(|t| t.info.max_depth).collect();
        Self::content_height_for(
            &self.collapsed,
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
        for i in 0..self.cached.tracks.len() {
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
        let depths: Vec<usize> = self.cached.tracks.iter().map(|t| t.info.max_depth).collect();
        for (ti, track) in self.cached.tracks.iter().enumerate() {
            if self.track_collapsed(ti) {
                continue;
            }
            let (_, ry, rh) = Self::track_layout_y(
                &self.collapsed,
                &depths,
                ti,
                self.style.track_header_height,
                self.style.frame_row_height,
            );
            if y < ry || y >= ry + rh {
                continue;
            }
            let row = ((y - ry) / self.style.frame_row_height) as usize;
            if row > track.info.max_depth {
                return None;
            }
            let t = self.viewport.x_to_time(x as f64, width as f64);
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
}
