//! Time-axis ruler element for the flame graph.
//!
//! `RulerCanvas`: the time-axis ruler pinned below the tracks. It uses the
//! exact same `time → x` mapping as the flame canvas, so ticks stay aligned
//! with the frames while panning/zooming. It is a paint-only element (no
//! hitbox, no gestures).

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, ShapedLine, Style, TextAlign, TextRun, Window, fill, point, px,
    relative, size,
};
use crate::ActiveTheme;

use super::format::format_tick;
use super::style::FlameGraphStyle;
use super::state::FlameGraphState;

const TICK_MIN_SPACING_PX: f32 = 80.0;

pub(crate) struct RulerCanvas {
    pub(crate) view: gpui::Entity<FlameGraphState>,
}

impl IntoElement for RulerCanvas {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RulerCanvas {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        let ruler_height = self.view.read(cx).style.ruler_height;
        style.size.width = relative(1.).into();
        style.size.height = px(ruler_height).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        ()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.read(cx);
        let origin = bounds.origin;
        let width = bounds.size.width.as_f32();
        
        let style = view.style.clone();
        let ruler_height = style.ruler_height;
        let ruler_font_h = style.ruler_font_size + 2.0;
        let time_format = view.time_format;

        window.paint_quad(fill(bounds, cx.theme().muted));
        // top border
        window.paint_quad(fill(
            Bounds::new(origin, size(px(width), px(1.0))),
            cx.theme().border,
        ));

        let vp = view.viewport;
        // Copy the marker state out of the view right away: painting later
        // borrows `cx` mutably (shaped-text paint), which would otherwise
        // collide with a live immutable borrow of the view.
        let drag = view.drag;
        let t1 = view.t1;
        // Tick steps are additionally clamped to the configured minimum unit:
        // below one nanosecond the labels would only ever repeat "0 ns" (the
        // fraction is below the resolution floor), so a sub-ns step is useless.
        let min_step = time_format.min_unit.seconds();
        let step =
            nice_step(vp.duration() / width as f64 * TICK_MIN_SPACING_PX as f64).max(min_step);
        let mut t = (vp.start / step).floor() * step;
        let mut ticks = Vec::new();
        while t <= vp.end + step * 0.5 {
            let x = vp.time_to_x(t, width as f64);
            if x >= -10.0 && x <= width as f64 + 10.0 {
                ticks.push((t, x as f32));
            }
            t += step;
        }

        let scale = window.scale_factor();
        for (t, x) in ticks {
            let device_x = (x * scale).round();
            let snapped_x = (device_x / scale).clamp(0.0, width - 4.0);
            window.paint_quad(fill(
                Bounds::new(
                    point(origin.x + px(snapped_x), origin.y + px(5.0)),
                    size(px(1.0), px(ruler_height - 5.0)),
                ),
                style.ruler_tick_color,
            ));
            let label = shape_label(
                window,
                format_tick(t, step, &time_format).into(),
                style.ruler_text_color,
                style.ruler_font_size,
            );
            label
                .paint(
                    origin + point(px(x + 4.0), px(ruler_height - ruler_font_h)),
                    px(ruler_font_h),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
        }

        // `t1` marker. At rest: a single line + label for the pinned `t1`.
        // While dragging: lines at both edges of the `[t1..t2]` range plus
        // `t1`, delta and `t2` labels, all sharing the mapping above so they
        // stay glued to the flame canvas. Labels use the ruler's own
        // step-derived unit (`format_tick`) so they stay consistent with the
        // tick text instead of re-deriving units from each value's magnitude.
        if let Some(d) = drag {
            let ta = vp.x_to_time(d.origin.x as f64, width as f64);
            let tb = vp.x_to_time(d.current.x as f64, width as f64);
            let t1 = ta.min(tb);
            let t2 = ta.max(tb);
            let x1 = vp.time_to_x(t1, width as f64) as f32;
            let x2 = vp.time_to_x(t2, width as f64) as f32;
            paint_ruler_line(window, origin, x1, &style);
            paint_ruler_line(window, origin, x2, &style);

            let l1 = shape_label(
                window,
                format_tick(t1, step, &time_format).into(),
                style.band_line,
                style.ruler_font_size,
            );
            let ld = shape_label(
                window,
                format_tick(t2 - t1, step, &time_format).into(),
                style.band_line,
                style.ruler_font_size,
            );
            let l2 = shape_label(
                window,
                format_tick(t2, step, &time_format).into(),
                style.band_line,
                style.ruler_font_size,
            );
            let w1 = l1.width().as_f32();
            let wd = ld.width().as_f32();
            let w2 = l2.width().as_f32();
            paint_ruler_label(
                window,
                cx,
                &l1,
                origin,
                origin.x + px((x1 + 4.0).clamp(0.0, (width - w1 - 4.0).max(0.0))),
                ruler_font_h,
            );
            paint_ruler_label(
                window,
                cx,
                &l2,
                origin,
                origin.x + px((x2 + 4.0).clamp(0.0, (width - w2 - 4.0).max(0.0))),
                ruler_font_h,
            );
            paint_ruler_label(
                window,
                cx,
                &ld,
                origin,
                origin.x + px(((x1 + x2) * 0.5 - wd * 0.5).clamp(0.0, (width - wd).max(0.0))),
                ruler_font_h,
            );
        } else if t1 >= vp.start && t1 <= vp.end {
            let x = vp.time_to_x(t1, width as f64) as f32;
            paint_ruler_line(window, origin, x, &style);
            let label = shape_label(
                window,
                format_tick(t1, step, &time_format).into(),
                style.band_line,
                style.ruler_font_size,
            );
            let lx = (x + 4.0).clamp(0.0, (width - label.width().as_f32() - 4.0).max(0.0));
            paint_ruler_label(window, cx, &label, origin, origin.x + px(lx), ruler_font_h);
        }
    }
}

/// Pick a "nice" tick step (1/2/5 × 10^n) that is >= `min_step`.
fn nice_step(min_step: f64) -> f64 {
    if min_step <= 0.0 {
        return 1e-3;
    }
    let exponent = min_step.log10().floor();
    let base = 10f64.powf(exponent);
    let mag = min_step / base;
    let nice = if mag <= 1.0 {
        1.0
    } else if mag <= 2.0 {
        2.0
    } else if mag <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * base
}

fn shape_label(window: &mut Window, text: gpui::SharedString, color: Hsla, font_size: f32) -> ShapedLine {
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

/// Full-height accent line at `x` for the `t1`/`t2` markers.
fn paint_ruler_line(window: &mut Window, origin: Point<Pixels>, x: f32, style: &FlameGraphStyle) {
    window.paint_quad(fill(
        Bounds::new(
            point(origin.x + px(x), origin.y),
            size(px(1.5), px(style.ruler_height)),
        ),
        style.band_line,
    ));
}

fn paint_ruler_label(
    window: &mut Window,
    cx: &mut App,
    line: &ShapedLine,
    origin: Point<Pixels>,
    sx: Pixels,
    ruler_font_h: f32,
) {
    line.paint(
        point(sx, origin.y + px(0.0)),
        px(ruler_font_h),
        TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
}
