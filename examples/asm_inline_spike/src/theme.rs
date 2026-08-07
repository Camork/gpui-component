//! Light-theme palette + layout metrics, mirroring gpui-component's
//! `Default Light` theme so the spike matches the editordemo look.

use gpui::{Hsla, Pixels, px, rgb, rgba};

pub(crate) const GUTTER_WIDTH: f32 = 64.0;
pub(crate) const FOLD_WIDTH: f32 = 22.0;

pub(crate) fn hs(hex: u32) -> Hsla {
    rgb(hex).into()
}
pub(crate) fn has(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub(crate) fn editor_bg() -> Hsla {
    hs(0xffffff)
}
pub(crate) fn gutter_bg() -> Hsla {
    hs(0xf5f5f5)
}
pub(crate) fn asm_row_bg() -> Hsla {
    hs(0xf4f4f6)
}
pub(crate) fn text_source() -> Hsla {
    hs(0x000000)
}
pub(crate) fn text_asm() -> Hsla {
    hs(0x525252)
}
pub(crate) fn gutter_text() -> Hsla {
    hs(0x929292)
}
pub(crate) fn selection_bg() -> Hsla {
    // #55a0fc at ~40% opacity (gpui rgba is packed 0xRRGGBBAA).
    has(0x55a0fc66)
}
pub(crate) fn active_line_bg() -> Hsla {
    // A soft translucent blue tint for active/selected line background.
    has(0x55a0fc24)
}
pub(crate) fn caret_color() -> Hsla {
    hs(0x0a0a0a)
}
pub(crate) fn breakpoint_red() -> Hsla {
    hs(0xdc2626)
}
pub(crate) fn fold_color() -> Hsla {
    hs(0x9ca3af)
}
pub(crate) fn header_bg() -> Hsla {
    hs(0xf8f8f8)
}
pub(crate) fn header_fg() -> Hsla {
    hs(0x171717)
}
pub(crate) fn header_dim() -> Hsla {
    hs(0x737373)
}
pub(crate) fn border_color() -> Hsla {
    hs(0xe5e5e5)
}

/// Text x offset for code rows (gutter + fold gutter + the fold glyph padding).
pub(crate) fn text_x() -> Pixels {
    px(GUTTER_WIDTH + FOLD_WIDTH)
}
