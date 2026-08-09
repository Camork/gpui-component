//! Configurable style for flame graph components.
//!
//! All layout metrics and accent colors are collected into [`FlameGraphStyle`]
//! so callers can override any value. The [`Default`] implementation provides
//! sensible defaults matching the original spike palette.

use gpui::{Hsla, Pixels, px, rgb};

/// Configurable layout metrics and accent colors for the flame graph.
#[derive(Clone)]
pub struct FlameGraphStyle {
    /// Height of each track header row (px).
    pub track_header_height: f32,
    /// Height of each frame row (px).
    pub frame_row_height: f32,
    /// Height of the bottom ruler (px).
    pub ruler_height: f32,
    /// Font size for frame labels.
    pub frame_font_size: f32,
    /// Font size for track header text.
    pub header_font_size: f32,
    /// Font size for ruler tick labels.
    pub ruler_font_size: f32,
    /// Vertical padding between frame rows.
    pub row_padding: f32,
    /// Size of the collapse/expand chevron icon.
    pub header_icon_size: f32,
    /// Border color for the selected frame.
    pub selected_border: Hsla,
    /// Border color for the hovered frame.
    pub hover_border: Hsla,
    /// Overlay color applied to dim non-matching frames during search.
    pub dim_overlay: Hsla,
    /// Color of the t1/t2 measure-band lines.
    pub band_line: Hsla,
    /// Fill color of the translucent band between t1 and t2.
    pub band_fill: Hsla,
    /// Color of ruler tick lines.
    pub ruler_tick_color: Hsla,
    /// Color of ruler tick labels.
    pub ruler_text_color: Hsla,
    /// Tooltip background.
    pub tooltip_bg: Hsla,
    /// Tooltip foreground (primary text).
    pub tooltip_fg: Hsla,
    /// Tooltip secondary/dim text.
    pub tooltip_dim: Hsla,
    /// Tooltip corner radius.
    pub tooltip_radius: Pixels,
    /// Near-black text drawn on top of frame fills.
    pub frame_text_color: Hsla,
}

impl Default for FlameGraphStyle {
    fn default() -> Self {
        Self {
            track_header_height: 28.0,
            frame_row_height: 18.0,
            ruler_height: 26.0,
            frame_font_size: 11.0,
            header_font_size: 12.0,
            ruler_font_size: 11.0,
            row_padding: 1.0,
            header_icon_size: 14.0,
            selected_border: rgb(0x1d4ed8).into(),
            hover_border: rgb(0xf59e0b).into(),
            dim_overlay: Hsla { h: 0.0, s: 0.0, l: 1.0, a: 0.72 },
            band_line: rgb(0x9595fb).into(),
            band_fill: Hsla { h: 0.667, s: 0.93, l: 0.78, a: 0.10 },
            ruler_tick_color: gpui::black(),
            ruler_text_color: gpui::black(),
            tooltip_bg: rgb(0x18181b).into(),
            tooltip_fg: rgb(0xf4f4f5).into(),
            tooltip_dim: rgb(0xa1a1aa).into(),
            tooltip_radius: px(6.0),
            frame_text_color: Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.72 },
        }
    }
}

/// Default time-format configuration: 2 decimal places, nanoseconds as floor.
pub fn default_time_format() -> super::format::TimeFormatConfig {
    super::format::TimeFormatConfig {
        decimals: 2,
        min_unit: super::format::TimeUnit::Ns,
    }
}
