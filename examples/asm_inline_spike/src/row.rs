//! The per-row GPUI element: geometry, prepaint (selection / caret / fold
//! glyph / hitbox), paint, and the unified mouse interactions (breakpoint
//! toggle, fold toggle, selection drag).

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, Entity, GlobalElementId, Hitbox,
    HitboxBehavior, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Path, PathBuilder, Pixels, Point, ShapedLine, Style, TextAlign, TextRun, Window,
    fill, point, px, relative, size,
};

use crate::blink::CURSOR_WIDTH;
use crate::model::{Selection, SelectionAnchor};
use crate::theme;
use crate::view::AsmInlineView;

pub(crate) struct PrepaintState {
    selection_path: Option<Path<Pixels>>,
    fold_glyph: Option<ShapedLine>,
    hitbox: Hitbox,
    cursor_quad: Option<gpui::PaintQuad>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelectionPosition {
    Single,
    Top,
    Middle,
    Bottom,
}

pub(crate) struct CodeRow {
    pub(crate) view: Entity<AsmInlineView>,
    pub(crate) ix: usize,
    pub(crate) is_asm: bool,
    pub(crate) is_active: bool,
    pub(crate) line: ShapedLine,
    pub(crate) gutter_line: ShapedLine,
    /// Some(expanded) for source rows that carry an asm block.
    pub(crate) fold_expanded: Option<bool>,
    pub(crate) has_breakpoint: bool,
    /// (start_col, end_col) highlight for this row.
    pub(crate) selection: Option<(usize, usize)>,
    pub(crate) selection_position: Option<SelectionPosition>,
    /// Column of the caret when this row holds the active cursor.
    pub(crate) cursor_col: Option<usize>,
    pub(crate) text_x: Pixels,
    pub(crate) gutter_width: Pixels,
    pub(crate) row_height: Pixels,
}

impl IntoElement for CodeRow {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CodeRow {
    type RequestLayoutState = ShapedLine;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = self.row_height.into();
        (window.request_layout(style, [], cx), self.line.clone())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        // Snap the row's paint bounds to the device-pixel grid with floor for
        // the top edge and ceil for the bottom. The list may land rows on
        // fractional device offsets (fractional `item_height`), so adjacent
        // rows can drift ±1 device pixel apart. Extending every row's painted
        // box by one device pixel outward makes neighbours overlap by 0-2
        // pixels, so there is never an unpainted seam; at an overlap the later
        // row repaints over the earlier one, keeping the (translucent)
        // selection continuous. Mirror of gpui-component Input, which paints
        // its selection as one continuous path.
        let scale = window.scale_factor();
        let top = px((f32::from(bounds.top()) * scale).floor() / scale);
        let bottom = px((f32::from(bounds.bottom()) * scale).ceil() / scale);

        let selection_path = self.selection.map(|(s, e)| {
            let x0 = bounds.left() + self.text_x + layout.x_for_index(s);
            let x1 = bounds.left() + self.text_x + layout.x_for_index(e);
            // Ensure at least a small width so empty/minuscule selections are
            // still visible (mirrors gpui-component Input's min-width handling).
            let x1 = x1.max(x0 + px(6.0));
            let r = px(2.0);
            let (tl, tr, br, bl) = match self.selection_position {
                Some(SelectionPosition::Single) => (r, r, r, r),
                Some(SelectionPosition::Top) => (r, r, px(0.0), px(0.0)),
                Some(SelectionPosition::Bottom) => (px(0.0), px(0.0), r, r),
                Some(SelectionPosition::Middle) | None => (px(0.0), px(0.0), px(0.0), px(0.0)),
            };
            rounded_rect_corners(
                window,
                Bounds::from_corners(point(x0, top), point(x1, bottom)),
                tl,
                tr,
                br,
                bl,
            )
        });

        let fold_glyph = self.fold_expanded.map(|expanded| {
            let glyph: gpui::SharedString = if expanded { "▾".into() } else { "▸".into() };
            let style = window.text_style();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let mut run = TextRun {
                len: 0,
                font: style.font(),
                color: theme::fold_color(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            run.len = glyph.len();
            window
                .text_system()
                .shape_line(glyph, font_size, &[run], None)
        });

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        if std::env::var_os("ASM_DEBUG_BOUNDS").is_some() {
            eprintln!(
                "row {:>3} asm={} act={} off_y={:.3} top={:.3} bottom={:.3} h={:.3}",
                self.ix,
                self.is_asm as u8,
                self.is_active as u8,
                f32::from(window.element_offset().y),
                f32::from(bounds.top()),
                f32::from(bounds.bottom()),
                f32::from(bounds.bottom() - bounds.top())
            );
        }
        let cursor_quad = self.cursor_col.map(|col| {
            let x = bounds.left() + self.text_x + layout.x_for_index(col);
            fill(
                Bounds::new(
                    point(x, bounds.top()),
                    size(px(CURSOR_WIDTH), self.row_height),
                ),
                theme::caret_color(),
            )
        });
        PrepaintState {
            selection_path,
            fold_glyph,
            hitbox,
            cursor_quad,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Paint all row geometry at the same device-pixel-snapped bounds used
        // for the selection in prepaint, so backgrounds and selection share edges.
        let scale = window.scale_factor();
        let top = px((f32::from(bounds.top()) * scale).floor() / scale);
        let bottom = px((f32::from(bounds.bottom()) * scale).ceil() / scale);
        let row_bounds = Bounds::from_corners(point(bounds.left(), top), point(bounds.right(), bottom));

        // Every row paints an opaque background — asm rows grey, source rows the
        // editor white. A translucent selection from the row above may overlap
        // this row by a device pixel when rows land off-grid; an opaque bg here
        // erases that stray selection before this row draws its own, so the
        // selection can never double-paint (which would show as a darker blue
        // hairline between selected rows).
        let bg = if self.is_asm {
            theme::asm_row_bg()
        } else {
            theme::editor_bg()
        };
        window.paint_quad(fill(row_bounds, bg));
        if self.is_active {
            window.paint_quad(fill(row_bounds, theme::active_line_bg()));
        }

        let gutter_bounds = Bounds::from_corners(
            point(bounds.left(), top),
            point(bounds.left() + self.gutter_width, bottom),
        );
        window.paint_quad(fill(gutter_bounds, theme::gutter_bg()));

        if let Some(q) = prepaint.selection_path.take() {
            // Use the same final filled color as source-tier light selection:
            // a translucent blue, matching gpui-component's `ColorTokens::selection`.
            window.paint_path(q, theme::selection_bg());
        }

        if let Some(q) = prepaint.cursor_quad.take() {
            if self.view.read(cx).show_cursor(window, cx) {
                window.paint_quad(q);
            }
        }

        if self.has_breakpoint {
            let center = point(
                bounds.left() + px(11.0),
                bounds.top() + self.row_height * 0.5,
            );
            window.paint_path(circle_path(center, px(4.0)), theme::breakpoint_red());
        }

        // gutter text, right-aligned in the gutter
        let gutter_origin = point(
            bounds.left() + self.gutter_width - px(7.0) - self.gutter_line.width(),
            bounds.top(),
        );
        self.gutter_line
            .paint(
                gutter_origin,
                self.row_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();

        if let Some(glyph) = &prepaint.fold_glyph {
            glyph
                .paint(
                    bounds.origin + point(self.gutter_width + px(3.0), px(0.0)),
                    self.row_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
        }

        self.line
            .paint(
                bounds.origin + point(self.text_x, px(0.0)),
                self.row_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();

        // cursor: IBeam over the text area, arrow elsewhere on the row
        let m = window.mouse_position();
        let local_x = m.x - bounds.left();
        if local_x >= px(0.0) && local_x <= bounds.size.width {
            window.set_cursor_style(
                if local_x >= self.text_x {
                    CursorStyle::IBeam
                } else {
                    CursorStyle::Arrow
                },
                &prepaint.hitbox,
            );
        }

        self.register_interactions(&prepaint.hitbox, bounds, window, cx);
    }
}

impl CodeRow {
    /// Unified row interaction: breakpoint toggle (gutter), fold toggle (source
    /// rows with asm), and selection drag. One handler chain for both row kinds.
    fn register_interactions(
        &self,
        hitbox: &Hitbox,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut App,
    ) {
        // Mouse-drag threshold (px): a gutter press that moves less than this is
        // a click (toggles the breakpoint); more is a full-line drag-select.
        const DRAG_THRESHOLD: f32 = 4.0;

        let hitbox = hitbox.clone();
        let view = self.view.clone();
        let ix = self.ix;
        let text_x = self.text_x;
        let gutter_width = self.gutter_width;
        let row_height = self.row_height;
        let line = self.line.clone();
        let my_top = bounds.top();
        let my_left = bounds.left();

        let down_hitbox = hitbox.clone();
        let down_view = view.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !down_hitbox.is_hovered(window)
                || e.button != MouseButton::Left
            {
                return;
            }
            let local_x = e.position.x - down_hitbox.bounds.left();

            // right-click in the text area: keep the selection when it covers
            // the click point, otherwise collapse the cursor to the click point.
            if e.button == MouseButton::Right {
                if local_x >= gutter_width {
                    let column = line.closest_index_for_x(local_x - text_x);
                    down_view.update(cx, |view, cx| {
                        let keep = view
                            .selection
                            .map_or(false, |sel| view.model.selection_contains(sel, ix, column));
                        if !keep {
                            if let Some(row) = view.model.rowkey_for_di(ix) {
                                let anchor = SelectionAnchor { row, column };
                                view.selection = Some(Selection {
                                    start: anchor,
                                    end: anchor,
                                });
                                view.cursor = Some(anchor);
                                view.preferred_column = Some(column);
                            }
                        }
                        cx.notify();
                    });
                }
                return;
            }
            if e.button != MouseButton::Left {
                return;
            }

            // gutter press: record the gesture; decide click-vs-drag on mouse-up.
            if local_x < gutter_width {
                down_view.update(cx, |view, cx| {
                    view.gutter_origin = Some(crate::view::GutterOrigin {
                        start_ix: ix,
                        screen_start: e.position,
                        dragged: false,
                    });
                    cx.notify();
                });
                window.capture_pointer(down_hitbox.id);
                return;
            }
            // fold icon: expand/collapse asm for this source row
            if local_x < text_x {
                down_view.update(cx, |view, cx| {
                    if let Some(src_idx) = view.model.foldable_source_for_di(ix) {
                        view.toggle_expanded(src_idx);
                        cx.notify();
                    }
                });
                return;
            }

            // text area: begin a selection anchored at this logical row + column
            let column = line.closest_index_for_x(local_x - text_x);
            let Some(row) = down_view.read(cx).model.rowkey_for_di(ix) else {
                return;
            };
            down_view.update(cx, |view, cx| {
                let anchor = SelectionAnchor { row, column };
                view.selection = Some(Selection {
                    start: anchor,
                    end: anchor,
                });
                view.cursor = Some(anchor);
                view.preferred_column = Some(column);
                view.selecting = true;
                view.gutter_origin = None;
                view.blink.update(cx, |blink, cx| blink.pause(cx));
                cx.notify();
            });
            window.capture_pointer(down_hitbox.id);
        });

        let move_hitbox = hitbox.clone();
        let move_view = view.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !move_hitbox.is_hovered(window) {
                return;
            }
            let selecting = move_view.read(cx).selecting;
            let gutter = move_view.read(cx).gutter_origin;
            if !selecting && gutter.is_none() {
                return;
            }
            // map the pointer to a display row from the origin row's own
            // bounds + index + uniform row height
            let content_top = my_top - row_height * ix;
            let rel_y = e.position.y - content_top;
            let count = move_view.read(cx).model.display_rows.len();
            let target_ix = ((rel_y / row_height).floor() as isize)
                .clamp(0, count.saturating_sub(1) as isize) as usize;

            // gutter drag: crossing the threshold turns it into a full-line select
            if let Some(g) = gutter {
                let dx = (e.position.x - g.screen_start.x).as_f32();
                let dy = (e.position.y - g.screen_start.y).as_f32();
                if !g.dragged && (dx * dx + dy * dy) < DRAG_THRESHOLD * DRAG_THRESHOLD {
                    return;
                }
                let start_row = match move_view.read(cx).model.rowkey_for_di(g.start_ix) {
                    Some(r) => r,
                    None => return,
                };
                let Some(target_row) = move_view.read(cx).model.rowkey_for_di(target_ix) else {
                    return;
                };
                move_view.update(cx, |view, cx| {
                    if let Some(origin) = view.gutter_origin.as_mut() {
                        origin.dragged = true;
                    }
                    view.selecting = true;
                    view.selection = Some(Selection {
                        start: SelectionAnchor {
                            row: start_row,
                            column: 0,
                        },
                        end: SelectionAnchor {
                            row: target_row,
                            column: view.model.line_len(target_row),
                        },
                    });
                    cx.notify();
                });
                return;
            }

            let Some(target_row) = move_view.read(cx).model.rowkey_for_di(target_ix) else {
                return;
            };
            let local_x = e.position.x - (my_left + text_x);
            let column = move_view.read(cx).column_for_x(target_ix, local_x, window);
            move_view.update(cx, |view, cx| {
                if let Some(sel) = view.selection.as_mut() {
                    sel.end = SelectionAnchor {
                        row: target_row,
                        column,
                    };
                }
                cx.notify();
            });
        });

        let up_hitbox = hitbox;
        let up_view = view;
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !up_hitbox.is_hovered(window)
                || e.button != MouseButton::Left
            {
                return;
            }
            up_view.update(cx, |view, cx| {
                // A gutter click (no drag) toggles the breakpoint.
                if let Some(origin) = view.gutter_origin.take() {
                    if !origin.dragged {
                        if let Some(row) = view.model.display_rows.get(origin.start_ix).copied() {
                            view.toggle_breakpoint(row);
                        }
                    }
                }
                view.selecting = false;
                cx.notify();
            });
        });
    }
}
/// Circle path used for the breakpoint dot.
pub(crate) fn circle_path(center: Point<Pixels>, radius: Pixels) -> gpui::Path<Pixels> {
    let n = 24;
    let mut b = PathBuilder::fill();
    let points: Vec<Point<Pixels>> = (0..n)
        .map(|k| {
            let t = std::f32::consts::TAU * k as f32 / n as f32;
            point(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect();
    b.add_polygon(&points, true);
    b.build().unwrap()
}

/// Filled rounded-rect path with independent corner radii, used for selection.
fn rounded_rect_corners(
    window: &Window,
    bounds: Bounds<Pixels>,
    tl: Pixels,
    tr: Pixels,
    br: Pixels,
    bl: Pixels,
) -> Path<Pixels> {
    let scale = window.scale_factor();
    let snap = |p: Pixels| (p * scale).round() / scale;

    let x0 = snap(bounds.left());
    let x1 = snap(bounds.right());
    let y0 = snap(bounds.top());
    let y1 = snap(bounds.bottom());

    if tl == px(0.0) && tr == px(0.0) && br == px(0.0) && bl == px(0.0) {
        let mut b = PathBuilder::fill();
        b.move_to(point(x0, y0));
        b.line_to(point(x1, y0));
        b.line_to(point(x1, y1));
        b.line_to(point(x0, y1));
        b.line_to(point(x0, y0));
        return b.build().unwrap();
    }

    let mut b = PathBuilder::fill();
    b.move_to(point(x0 + tl, y0));
    b.line_to(point(x1 - tr, y0));
    if tr > px(0.0) {
        b.curve_to(point(x1, y0), point(x1, y0 + tr));
    } else {
        b.line_to(point(x1, y0));
    }
    b.line_to(point(x1, y1 - br));
    if br > px(0.0) {
        b.curve_to(point(x1, y1), point(x1 - br, y1));
    } else {
        b.line_to(point(x1, y1));
    }
    b.line_to(point(x0 + bl, y1));
    if bl > px(0.0) {
        b.curve_to(point(x0, y1), point(x0, y1 - bl));
    } else {
        b.line_to(point(x0, y1));
    }
    b.line_to(point(x0, y0 + tl));
    if tl > px(0.0) {
        b.curve_to(point(x0, y0), point(x0 + tl, y0));
    } else {
        b.line_to(point(x0, y0));
    }
    b.build().unwrap()
}
