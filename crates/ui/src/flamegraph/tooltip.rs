//! Tooltip layout helpers for the flame graph hover info box.
//!
//! The info box is painted inside the canvas element itself (not as a sibling
//! overlay element): an overlay element's `paint` bounds turn out not to be
//! the flame viewport in this layout, so anything positioned from there lands
//! off-screen. Drawing from the canvas keeps the box and the hit-testing in
//! the same, proven coordinate space.

use gpui::{Bounds, Hsla, Pixels, Point, ShapedLine, SharedString, TextRun, Window, point, px};

use super::format::format_duration;
use super::state::{FlameGraphState, FrameId};
use super::style::FlameGraphStyle;

/// Shape the tooltip's text and work out its box: everything except the final
/// paint. Returns the box bounds and the shaped lines with their origins.
pub(crate) fn tooltip_layout(
    view: &FlameGraphState,
    fid: FrameId,
    style: &FlameGraphStyle,
    time_format: &super::format::TimeFormatConfig,
    origin: Point<Pixels>,
    width: f32,
    height: f32,
    window: &mut Window,
) -> (Bounds<Pixels>, Vec<(ShapedLine, Point<Pixels>, f32)>) {
    let Some(track) = view.cached.tracks.get(fid.track) else {
        return (Bounds::default(), Vec::new());
    };
    let Some(f) = track.frames.get(fid.flat) else {
        return (Bounds::default(), Vec::new());
    };
    let track_name = &track.info.name;

    let name_line = shape_label(window, f.name.clone().into(), style.tooltip_fg, style.frame_font_size);
    let fmt = |t| format_duration(t, time_format);
    let start_line = shape_label(
        window,
        format!("start  {}", fmt(f.abs_start)).into(),
        style.tooltip_dim,
        style.frame_font_size,
    );
    let dur_line = shape_label(
        window,
        format!("dur    {}", fmt(f.abs_end - f.abs_start)).into(),
        style.tooltip_dim,
        style.frame_font_size,
    );
    let track_line = shape_label(window, format!("track  {track_name}").into(), style.tooltip_dim, style.frame_font_size);

    let pad_x = 8.0;
    let pad_y = 6.0;
    let line_h = 16.0;
    let body_w = [&name_line, &start_line, &dur_line, &track_line]
        .iter()
        .map(|l| l.width().as_f32())
        .fold(0.0, f32::max)
        + pad_x * 2.0;
    let body_h = 4.0 * line_h + pad_y * 2.0;

    // Place below/right of the cursor, keeping the box on-screen.
    let mouse = window.mouse_position();
    let mx = (mouse.x - origin.x).as_f32();
    let my = (mouse.y - origin.y).as_f32();
    let bx = (mx + 12.0).clamp(0.0, (width - body_w).max(0.0));
    let by = (my + 14.0).clamp(0.0, (height - body_h).max(0.0));
    let box_bounds = Bounds::from_corners(
        point(origin.x + px(bx), origin.y + px(by)),
        point(origin.x + px(bx + body_w), origin.y + px(by + body_h)),
    );

    let lines = [&name_line, &start_line, &dur_line, &track_line]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let o = point(
                box_bounds.origin.x + px(pad_x),
                box_bounds.origin.y + px(pad_y + i as f32 * line_h),
            );
            ((**line).clone(), o, line_h)
        })
        .collect();
    (box_bounds, lines)
}

fn shape_label(window: &mut Window, text: SharedString, color: Hsla, font_size: f32) -> ShapedLine {
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
