//! Custom GPUI Element for rendering the flame graph canvas.
//!
//! Drag measures a time range *and* zooms into it on release: every left
//! press pins `t1` at the press time, dragging shows a transient `[t1..t2]`
//! band (two full-height lines plus a fill) with live `t1`/`Δt`/`t2` labels in
//! the ruler, and releasing commits `t1` to the left edge, drops the
//! transient `t2`, and shrinks the viewport to the measured window.
//!
//! All geometry is computed in *content* coordinates (px) and offset by
//! `-scroll_y` when painted; hit-testing converts cursor positions back to
//! content coordinates with the same `scroll_y`. The canvas fills the flame
//! area viewport and the parent clips it, so off-viewport rows simply skip
//! painting.
//!
//! `paint` is split into two phases: it first reads all live view state into
//! owned values (quads, shaped lines, cursor) while holding an immutable
//! borrow of the view, then paints them with `&mut App`. This keeps the
//! borrow-checker happy: `Entity::read` holds the app context borrowed, and
//! shaped-text `paint` needs the app context mutably.

use gpui::{
    App, Bounds, Corners, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, SharedString, Style,
    TextAlign, TextRun, TransformationMatrix, Window, fill, point, px, relative, size,
};
use crate::ActiveTheme;
use crate::{IconName, IconNamed};

use super::source::QueryTrack;
use super::style::FlameGraphStyle;
use super::state::{DragState, FlameGraphState, FrameId};

const LABEL_MIN_WIDTH: f32 = 30.0;
/// Movement (px) before a press becomes a drag instead of a click.
const CLICK_THRESHOLD: f32 = 4.0;
/// Minimum drag width (px) for the release to zoom into the measured range.
const DRAG_MIN_WIDTH: f32 = 3.0;
/// Thickness (px) of the `t1`/`t2` measure lines.
const BAND_LINE_W: f32 = 1.5;

#[derive(Clone)]
pub(crate) struct CanvasPrepaint {
    hitbox: Hitbox,
    width: f32,
    height: f32,
}

/// Collects paint output for one frame, plus the theme colors needed by the
/// per-element helpers (snapshotted once per paint so helpers never touch
/// `&mut App` while the view is borrowed).
struct PaintSink<'a> {
    fills: &'a mut Vec<(Bounds<Pixels>, Hsla, Pixels)>,
    outlines: &'a mut Vec<(Bounds<Pixels>, Hsla, Pixels)>,
    texts: &'a mut Vec<(ShapedLine, Point<Pixels>, f32)>,
    /// Chevron/icon SVGs: (bounds, asset path, tint). Painted via the repo's
    /// icon assets so the collapse glyph matches the rest of the library.
    svgs: &'a mut Vec<(Bounds<Pixels>, SharedString, Hsla)>,
    background: Hsla,
    muted: Hsla,
    border: Hsla,
    fg: Hsla,
    muted_fg: Hsla,
}

pub(crate) struct FlameCanvas {
    pub(crate) view: gpui::Entity<FlameGraphState>,
}

impl IntoElement for FlameCanvas {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FlameCanvas {
    type RequestLayoutState = ();
    type PrepaintState = CanvasPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Tell vertical & horizontal scrollbars the viewport size and check for scrollbar drag.
        let width = bounds.size.width.as_f32();
        let height = bounds.size.height.as_f32();
        self.view.update(cx, |view, cx| {
            let scroll = view.scroll.0.clone();
            scroll.borrow_mut().viewport_h = height;
            view.check_h_scroll_drag(width, cx);
        });

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        CanvasPrepaint {
            hitbox,
            width,
            height,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let origin = bounds.origin;
        let width = prepaint.width;
        let height = prepaint.height;

        // `on_mouse_event` listeners may only be registered during the paint
        // phase (they live for the current frame only), so this must happen
        // here rather than in prepaint.
        self.register_interactions(&prepaint.hitbox, width, window, cx);

        let mut fills: Vec<(Bounds<Pixels>, Hsla, Pixels)> = Vec::new();
        let mut outlines: Vec<(Bounds<Pixels>, Hsla, Pixels)> = Vec::new();
        let mut texts: Vec<(ShapedLine, Point<Pixels>, f32)> = Vec::new();
        let mut svgs: Vec<(Bounds<Pixels>, SharedString, Hsla)> = Vec::new();
        let cursor;
        let tooltip_box;
        let tooltip_lines;
        let band_fill_rect;
        let band_lines;

        let style = self.view.read(cx).style.clone();

        // Snapshot the theme before borrowing the view, so helpers below only
        // need owned colors (no `&mut App`) while the view is read.
        let mut sink = PaintSink {
            fills: &mut fills,
            outlines: &mut outlines,
            texts: &mut texts,
            svgs: &mut svgs,
            background: cx.theme().background,
            muted: cx.theme().muted,
            border: cx.theme().border,
            fg: cx.theme().foreground,
            muted_fg: cx.theme().muted_foreground,
        };

        // ── phase 1: read the view into owned values ──────────────────────
        {
            let view = self.view.read(cx);
            let scroll_y = view.scroll_y();
            sink.fills.push((bounds, sink.background, px(0.0)));

            for (ti, track) in view.cached.tracks.iter().enumerate() {
                let (hy, ry, _rh) = view.track_geometry(ti);
                let collapsed = view.track_collapsed(ti);

                // Track header row.
                let header_y = hy - scroll_y;
                if header_y + style.track_header_height > 0.0 && header_y < height {
                    collect_track_header(
                        track, collapsed, origin, header_y, width, window, &mut sink, &style
                    );
                }
                if collapsed {
                    continue;
                }

                // Frame rows: skip the whole track if it is off-screen vertically.
                let rows_y = ry - scroll_y;
                let rh = (track.info.max_depth + 1) as f32 * style.frame_row_height;
                if rows_y + rh <= 0.0 || rows_y >= height {
                    continue;
                }
                let first_row = if rows_y < 0.0 {
                    (-rows_y / style.frame_row_height).ceil() as usize
                } else {
                    0
                };
                let last_row = if rows_y + rh > height {
                    (((height - rows_y) / style.frame_row_height).ceil() as usize).min(track.info.max_depth + 1)
                } else {
                    track.info.max_depth + 1
                };

                for (fi, f) in track.frames.iter().enumerate() {
                    if f.row < first_row || f.row >= last_row {
                        continue;
                    }
                    let x0 = view.time_to_x(f.abs_start, width);
                    let x1 = view.time_to_x(f.abs_end, width);
                    if x1 <= 0.0 || x0 >= width {
                        continue;
                    }

                    // Clamp rendering coordinates to avoid f32 precision loss when zoomed in deep (e.g. ns level).
                    let draw_x0 = x0.clamp(-100.0, width + 100.0);
                    let draw_x1 = x1.clamp(-100.0, width + 100.0);

                    let frame_y = rows_y + f.row as f32 * style.frame_row_height;
                    let rect = Bounds::from_corners(
                        point(origin.x + px(draw_x0), origin.y + px(frame_y)),
                        point(
                            origin.x + px(draw_x1),
                            origin.y + px(frame_y + style.frame_row_height - style.row_padding),
                        ),
                    );
                    sink.fills.push((rect, f.color, px(0.0)));

                    if !f.matched {
                        sink.fills.push((rect, style.dim_overlay, px(0.0)));
                    }
                    let fid = FrameId {
                        track: ti,
                        flat: fi,
                    };
                    if view.selected == Some(fid) {
                        sink.outlines.push((rect, style.selected_border, px(1.5)));
                    }

                    // Sticky label positioning: keep text visible when frame starts off-screen.
                    let vis_x0 = x0.max(0.0);
                    let vis_x1 = x1.min(width);
                    let vis_w = vis_x1 - vis_x0;

                    if vis_w >= LABEL_MIN_WIDTH && f.matched {
                        let line = shape_label(
                            window,
                            truncate_label(&f.name, vis_w - 6.0),
                            style.frame_font_size,
                            style.frame_text_color,
                        );
                        sink.texts.push((
                            line,
                            point(origin.x + px(vis_x0 + 3.0), origin.y + px(frame_y)),
                            style.frame_row_height,
                        ));
                    }
                }
            }

            // `t1` measure band: while dragging, two full-height lines at the
            // drag edges plus a translucent fill between them; at rest, the
            // single persistent `t1` line. In both cases the splash is above
            // the frame fills but below the tooltip.
            if let Some(d) = view.drag {
                let x0 = d.origin.x.min(d.current.x);
                let x1 = d.origin.x.max(d.current.x);
                band_fill_rect = Some(Bounds::from_corners(
                    point(origin.x + px(x0), origin.y),
                    point(origin.x + px(x1), origin.y + px(height)),
                ));
                band_lines = vec![
                    Bounds::from_corners(
                        point(origin.x + px(x0), origin.y),
                        point(origin.x + px(x0 + BAND_LINE_W), origin.y + px(height)),
                    ),
                    Bounds::from_corners(
                        point(origin.x + px(x1), origin.y),
                        point(origin.x + px(x1 + BAND_LINE_W), origin.y + px(height)),
                    ),
                ];
            } else if view.t1 >= view.viewport.start && view.t1 <= view.viewport.end {
                let x = view.time_to_x(view.t1, width);
                band_fill_rect = None;
                band_lines = vec![Bounds::from_corners(
                    point(origin.x + px(x), origin.y),
                    point(origin.x + px(x + BAND_LINE_W), origin.y + px(height)),
                )];
            } else {
                band_fill_rect = None;
                band_lines = Vec::new();
            }

            // Cursor: pointing hand over track headers, crosshair over frames.
            let mouse = window.mouse_position();
            let local = point(mouse.x - origin.x, mouse.y - origin.y);
            let cy = local.y.as_f32() + scroll_y;
            cursor = if view.header_at(cy).is_some() {
                CursorStyle::PointingHand
            } else if view.frame_at(local.x.as_f32(), cy, width).is_some() {
                CursorStyle::Crosshair
            } else {
                CursorStyle::Arrow
            };

            // Tooltip snapshot: same `origin`/`mouse_position` space as the
            // cursor logic above, which is why the box lines up with the
            // pointer here but never did when painted from a sibling overlay.
            match view.hovered {
                Some(fid) => {
                    let (b, l) =
                        super::tooltip::tooltip_layout(view, fid, &view.style, &view.time_format, origin, width, height, window);
                    tooltip_box = b;
                    tooltip_lines = l;
                }
                None => {
                    tooltip_box = Bounds::default();
                    tooltip_lines = Vec::new();
                }
            }
        }

        // ── phase 2: paint the snapshot ───────────────────────────────────
        for (b, c, r) in fills {
            let mut quad = fill(b, c);
            if r > px(0.0) {
                quad = quad.corner_radii(Corners::all(r));
            }
            window.paint_quad(quad);
        }
        for (b, c, t) in outlines {
            paint_outline(window, b, c, t);
        }
        for (line, o, lh) in texts {
            line.paint(o, px(lh), TextAlign::Left, None, window, cx)
                .ok();
        }
        for (b, path, color) in svgs {
            window
                .paint_svg(b, path, None, TransformationMatrix::default(), color, cx)
                .ok();
        }
        // `t1` band splash: above the frame fills, below the tooltip.
        if let Some(f) = band_fill_rect {
            window.paint_quad(fill(f, style.band_fill));
        }
        for r in band_lines {
            window.paint_quad(fill(r, style.band_line));
        }

        if !tooltip_lines.is_empty() {
            window.paint_quad(
                fill(tooltip_box, style.tooltip_bg)
                    .corner_radii(Corners::all(style.tooltip_radius))
                    .border_widths(px(1.0))
                    .border_color(style.tooltip_dim),
            );
            for (line, o, lh) in tooltip_lines {
                line.paint(o, px(lh), TextAlign::Left, None, window, cx)
                    .ok();
            }
        }
        window.set_cursor_style(cursor, &prepaint.hitbox);
    }
}

impl FlameCanvas {
    /// Register all mouse/wheel handlers for the canvas hitbox. Everything
    /// reads live view state, so a `cx.notify()` from any of them re-renders
    /// the view and re-paints the canvas.
    fn register_interactions(
        &self,
        hitbox: &Hitbox,
        width: f32,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let view = self.view.clone();
        let hitbox = hitbox.clone();

        // ── mouse down: begin a click/drag; double-click resets ──────────
        let down_view = view.clone();
        let down_hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !down_hitbox.is_hovered(window)
                || e.button != MouseButton::Left
            {
                return;
            }
            let local = e.position - down_hitbox.bounds.origin;
            let cx_px = local.x.as_f32();
            let cy_px = local.y.as_f32();
            let height = down_hitbox.bounds.size.height.as_f32();
            let content_h = down_view.read(cx).content_height();

            // Ignore presses on the rightmost 12px strip when content overflows,
            // letting the sibling vertical Scrollbar component handle mouse interaction.
            if cx_px >= width - 12.0 && content_h > height + 1.0 {
                return;
            }

            // Ignore presses on the bottommost 16px strip when horizontally zoomed in,
            // letting the sibling horizontal Scrollbar component handle mouse interaction.
            let view = down_view.read(cx);
            if cy_px >= height - 16.0 && view.viewport.duration() < view.source().session_duration() - 1e-6 {
                return;
            }

            let scroll_y = view.scroll_y();
            let cy = cy_px + scroll_y;
            let press_time = view.viewport.x_to_time(cx_px as f64, width as f64);

            // Any left press pins `t1` at the press time.
            down_view.update(cx, |view, _| view.t1 = press_time);

            // Double-click on a frame zooms into [frame.abs_start..frame.abs_end];
            // on a header it toggles collapse/expand;
            // on empty background it resets to the full trace.
            if e.click_count >= 2 {
                down_view.update(cx, |view, cx| {
                    if let Some(ti) = view.header_at(cy) {
                        view.toggle_collapse(ti, cx);
                    } else if let Some(fid) = view.frame_at(cx_px, cy, width) {
                        if let Some(track) = view.cached.tracks.get(fid.track) {
                            if let Some(frame) = track.frames.get(fid.flat) {
                                view.viewport.zoom_to_range(frame.abs_start, frame.abs_end);
                                view.t1 = frame.abs_start;
                                view.commit_t1();
                                view.selected = Some(fid);
                                view.last_width = width;
                                view.refresh_query();
                            }
                        }
                    } else {
                        view.viewport.reset();
                        view.refresh_query();
                    }
                    view.drag = None;
                    cx.notify();
                });
                return;
            }

            let header = down_view.read(cx).header_at(cy);
            let frame = down_view.read(cx).frame_at(cx_px, cy, width);
            down_view.update(cx, |view, cx| {
                view.drag = Some(DragState {
                    origin: point(cx_px, cy),
                    current: point(cx_px.clamp(0.0, width), cy),
                    pending_click: true,
                    down_header: header,
                    down_frame: frame,
                });
                cx.notify();
            });
            window.capture_pointer(down_hitbox.id);
        });

        // ── mouse move: update the drag rect or the hovered frame ─────────
        let move_view = view.clone();
        let move_hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !move_hitbox.is_hovered(window) {
                return;
            }
            let local = e.position - move_hitbox.bounds.origin;
            let scroll_y = move_view.read(cx).scroll_y();
            let cx_px = local.x.as_f32();
            let cy = local.y.as_f32() + scroll_y;

            if move_view.read(cx).drag.is_some() {
                move_view.update(cx, |view, cx| {
                    if let Some(d) = view.drag.as_mut() {
                        let dx = cx_px - d.origin.x;
                        let dy = cy - d.origin.y;
                        if d.pending_click && dx * dx + dy * dy > CLICK_THRESHOLD * CLICK_THRESHOLD
                        {
                            d.pending_click = false;
                        }
                        d.current = point(cx_px.clamp(0.0, width), cy);
                        cx.notify();
                    }
                });
            } else {
                let frame = move_view.read(cx).frame_at(cx_px, cy, width);
                move_view.update(cx, |view, cx| {
                    if view.hovered != frame {
                        view.hovered = frame;
                    }
                    cx.notify();
                });
            }
        });

        // ── mouse up: click (select / collapse) or drag (measure + zoom) ──
        let up_view = view.clone();
        let up_hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !up_hitbox.is_hovered(window)
                || e.button != MouseButton::Left
            {
                return;
            }
            up_view.update(cx, |view, cx| {
                if let Some(d) = view.drag.take() {
                    if d.pending_click {
                        if let Some(ti) = d.down_header {
                            view.toggle_collapse(ti, cx);
                        } else if let Some(fid) = d.down_frame {
                            view.selected = Some(fid);
                        } else {
                            view.selected = None;
                        }
                        // A bare click pins `t1` at the press time (snapped).
                        view.commit_t1();
                    } else {
                        // Drag: commit `t1` to the left edge (`t2` stays
                        // transient) and, if the range is wide enough, zoom the
                        // viewport into `[a..b]` (shrink-only).
                        let a = view.viewport.x_to_time(d.origin.x as f64, width as f64);
                        let b = view.viewport.x_to_time(d.current.x as f64, width as f64);
                        view.t1 = a.min(b);
                        view.commit_t1();
                        if (d.current.x - d.origin.x).abs() >= DRAG_MIN_WIDTH {
                            view.viewport.set_range(a, b);
                            view.last_width = width;
                            view.refresh_query();
                        }
                    }
                    cx.notify();
                }
            });
        });

        // ── scroll wheel: v-scroll (plain), pan (shift), or zoom (ctrl) ────
        let wheel_view = view.clone();
        let wheel_hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &gpui::ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !wheel_hitbox.should_handle_scroll(window) {
                return;
            }
            let delta = e.delta.pixel_delta(window.line_height());
            let dx = delta.x.as_f32() as f64;
            let dy = delta.y.as_f32() as f64;
            let local = e.position - wheel_hitbox.bounds.origin;
            wheel_view.update(cx, |view, cx| {
                if e.modifiers.control {
                    let step = if dy != 0.0 { dy } else { dx };
                    let factor = (-step as f32 * 0.004).exp() as f64;
                    view.viewport
                        .zoom_at_pixels(local.x.as_f32() as f64, width as f64, factor);
                    view.last_width = width;
                    view.refresh_query();
                } else if e.modifiers.shift {
                    let step_dx = if dx != 0.0 { dx } else { dy };
                    view.viewport.pan_pixels(step_dx, width as f64);
                    view.last_width = width;
                    view.refresh_query();
                } else {
                    if dy != 0.0 {
                        view.scroll_by_y(-dy as f32);
                    }
                    if dx != 0.0 {
                        view.viewport.pan_pixels(dx, width as f64);
                        view.last_width = width;
                        view.refresh_query();
                    }
                }
                view.hovered = None;
                cx.notify();
            });
            cx.stop_propagation();
        });

        // ── mouse exit: drop the hover highlight ──────────────────────────
        let exit_view = view.clone();
        window.on_mouse_event(move |_e: &MouseExitEvent, phase, _window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            exit_view.update(cx, |view, cx| {
                if view.hovered.is_some() {
                    view.hovered = None;
                    cx.notify();
                }
            });
        });
    }
}

// ── painting helpers ────────────────────────────────────────────────────────

fn collect_track_header(
    track: &QueryTrack,
    collapsed: bool,
    origin: Point<Pixels>,
    y: f32,
    width: f32,
    window: &mut Window,
    sink: &mut PaintSink,
    style: &FlameGraphStyle,
) {
    let rect = Bounds::from_corners(
        point(origin.x, origin.y + px(y)),
        point(origin.x + px(width), origin.y + px(y + style.track_header_height)),
    );
    sink.fills.push((rect, sink.muted, px(0.0)));
    // bottom border
    sink.fills.push((
        Bounds::new(
            point(origin.x, origin.y + px(y + style.track_header_height) - px(1.0)),
            size(px(width), px(1.0)),
        ),
        sink.border,
        px(0.0),
    ));

    // collapse chevron — reuses the repo's Lucide icon assets (same visual as
    // the accordion/select chevrons), centered in a fixed icon box.
    let icon_size = style.header_icon_size;
    let icon_y = y + (style.track_header_height - icon_size) * 0.5;
    let icon_bounds = Bounds::new(
        point(origin.x + px(10.0), origin.y + px(icon_y)),
        size(px(icon_size), px(icon_size)),
    );
    let chevron = if collapsed {
        IconName::ChevronRight
    } else {
        IconName::ChevronDown
    };
    sink.svgs.push((icon_bounds, chevron.path(), sink.muted_fg));

    // track name
    const NAME_X: f32 = 34.0;
    let name_line = shape_label(window, track.info.name.clone().into(), style.header_font_size, sink.fg);
    sink.texts.push((
        name_line.clone(),
        origin + point(px(NAME_X), px(y)),
        style.track_header_height,
    ));

    // dim stats
    let dim = format!(
        " · {} frames · {} rows",
        track.frames.len(),
        track.info.max_depth + 1
    );
    sink.texts.push((
        shape_label(window, dim.into(), style.header_font_size, sink.muted_fg),
        origin + point(px(NAME_X + name_line.width().as_f32() + 8.0), px(y)),
        style.track_header_height,
    ));
}

fn paint_outline(window: &mut Window, bounds: Bounds<Pixels>, color: Hsla, thickness: Pixels) {
    let (x0, y0) = (bounds.origin.x, bounds.origin.y);
    let (w, h) = (bounds.size.width, bounds.size.height);
    window.paint_quad(fill(Bounds::new(point(x0, y0), size(w, thickness)), color));
    window.paint_quad(fill(
        Bounds::new(point(x0, y0 + h - thickness), size(w, thickness)),
        color,
    ));
    window.paint_quad(fill(Bounds::new(point(x0, y0), size(thickness, h)), color));
    window.paint_quad(fill(
        Bounds::new(point(x0 + w - thickness, y0), size(thickness, h)),
        color,
    ));
}

/// Truncated frame label, shaped but not yet positioned.
fn truncate_label(text: &str, max_width: f32) -> gpui::SharedString {
    let avail = max_width.max(12.0);
    let max_chars = (avail / 6.6).max(3.0) as usize;
    let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    if text.chars().count() > max_chars {
        s.push('…');
    }
    s.into()
}

// ── text shaping ────────────────────────────────────────────────────────────

fn shape_label(
    window: &mut Window,
    text: gpui::SharedString,
    font_size: f32,
    color: Hsla,
) -> ShapedLine {
    let mut run = TextRun {
        len: 0,
        font: window.text_style().font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    run.len = text.len();
    window
        .text_system()
        .shape_line(text, px(font_size), &[run], None)
}
