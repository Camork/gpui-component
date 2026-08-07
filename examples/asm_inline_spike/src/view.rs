//! The asm-inline view: renders a header + a virtualization `uniform_list` of
//! `CodeRow`s. Owns focus, selection, cursor/blink, breakpoints, and folds.

use std::collections::HashSet;
use std::ops::Range;

use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, Hsla, Pixels, Render, ShapedLine,
    SharedString, TextRun, TextStyle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::blink::{BlinkCursor, BlinkEntity};
use crate::highlighter::SourceHighlighter;
use crate::model::{
    BreakpointKey, CodeModel, CursorMove, DisplayRow, RowKey, Selection, SelectionAnchor,
};
use crate::row::{CodeRow, SelectionPosition};
use crate::theme;
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};

actions!(
    SpikeActions,
    [
        CopyText,
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        DebugState,
        Verify,
        Quit
    ]
);

#[derive(Clone, Copy)]
pub(crate) struct GutterOrigin {
    pub(crate) start_ix: usize,
    pub(crate) screen_start: gpui::Point<gpui::Pixels>,
    pub(crate) dragged: bool,
}

pub(crate) struct AsmInlineView {
    focus_handle: FocusHandle,
    pub(crate) model: CodeModel,
    breakpoints: HashSet<BreakpointKey>,
    pub(crate) selection: Option<Selection>,
    pub(crate) selecting: bool,
    pub(crate) gutter_origin: Option<GutterOrigin>,
    pub(crate) cursor: Option<SelectionAnchor>,
    pub(crate) preferred_column: Option<usize>,
    selection_reversed: bool,
    pub(crate) blink: BlinkEntity,
    scroll_handle: gpui::UniformListScrollHandle,
    visible_range: Range<usize>,
    blink_started: bool,
    highlighter: SourceHighlighter,
}

impl AsmInlineView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let (sources, asm) = crate::fake_data::fake_data();
        let blink = cx.new(|_| BlinkCursor::new());
        let focus_handle = cx.focus_handle();
        let model = CodeModel::new(sources, asm);
        let highlighter = SourceHighlighter::new(&model);
        Self {
            focus_handle,
            model,
            breakpoints: HashSet::new(),
            selection: None,
            selecting: false,
            gutter_origin: None,
            cursor: None,
            preferred_column: None,
            selection_reversed: false,
            blink,
            scroll_handle: gpui::UniformListScrollHandle::new(),
            visible_range: 0..0,
            blink_started: false,
            highlighter,
        }
    }

    /// The active cursor anchor — the moving end of a non-empty selection,
    /// otherwise the standalone cursor.
    fn cursor_anchor(&self) -> Option<SelectionAnchor> {
        if let Some(sel) = self.selection {
            return Some(if self.selection_reversed {
                sel.start
            } else {
                sel.end
            });
        }
        self.cursor
    }

    /// Visible when focused, window active, blink on, and a cursor exists.
    pub(crate) fn show_cursor(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.is_focused(window)
            && window.is_window_active()
            && self.cursor_anchor().is_some()
            && self.blink.read(cx).visible()
    }

    /// Move the active cursor one step, mirroring `InputState::left/right/up/down`.
    fn move_cursor(&mut self, dir: CursorMove, cx: &mut Context<Self>) {
        let Some(cur) = self.cursor_anchor() else {
            return;
        };
        let next = self.model.move_cursor(cur, dir, self.preferred_column);
        self.selection = None;
        self.cursor = Some(next);
        self.selection_reversed = false;
        self.preferred_column = Some(next.column);
        self.blink.update(cx, |blink, cx| blink.pause(cx));
        cx.notify();
    }

    /// Shift+arrow: extend the selection from the anchor. The stationary end
    /// (the anchor) keeps its original position; the moving end follows the
    /// arrow. `selection_reversed` tracks whether the moving end has crossed
    /// past the anchor (dragging back upward/leftward).
    fn extend_selection(&mut self, dir: CursorMove, cx: &mut Context<Self>) {
        let Some(moving) = self.cursor_anchor() else {
            return;
        };
        let next = self.model.move_cursor(moving, dir, self.preferred_column);
        let anchor = match self.selection {
            Some(sel) if self.selection_reversed => sel.end,
            Some(sel) => sel.start,
            // No selection yet: the stationary end is where the cursor was.
            None => moving,
        };
        self.selection = Some(match self.model.anchor_order(anchor, next) {
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                self.selection_reversed = false;
                Selection {
                    start: anchor,
                    end: next,
                }
            }
            std::cmp::Ordering::Greater => {
                self.selection_reversed = true;
                Selection {
                    start: next,
                    end: anchor,
                }
            }
        });
        self.cursor = Some(next);
        self.preferred_column = Some(next.column);
        self.blink.update(cx, |blink, cx| blink.pause(cx));
        cx.notify();
    }

    /// (start_di, end_di, start_col, end_col) in display order, if any.
    fn selection_frame(&self) -> Option<(usize, usize, usize, usize)> {
        self.selection.and_then(|s| self.model.selection_frame(s))
    }

    fn reconcile_selection(&mut self) {
        let Some(sel) = self.selection else {
            return;
        };
        self.selection = Some(Selection {
            start: self.model.snap_anchor(sel.start),
            end: self.model.snap_anchor(sel.end),
        });
    }

    fn copy_text(&self) -> String {
        self.selection
            .map_or_else(String::new, |s| self.model.selection_text(s))
    }

    pub(crate) fn toggle_expanded(&mut self, src_idx: usize) {
        if !self.model.asm_by_src.contains_key(&src_idx) {
            return;
        }
        if !self.model.expanded.remove(&src_idx) {
            self.model.expanded.insert(src_idx);
        }
        self.model.rebuild_display_rows();
        self.reconcile_selection();
        // A bare cursor (no selection) is reconciled separately: `reconcile_selection`
        // no-ops when `self.selection` is None, so snap the standalone cursor too or it
        // becomes a dangling `RowKey` pointing into the removed asm block.
        if let Some(cur) = self.cursor {
            self.cursor = Some(self.model.snap_anchor(cur));
        }
        #[cfg(debug_assertions)]
        self.verify_invariants();
    }

    pub(crate) fn toggle_breakpoint(&mut self, row: DisplayRow) {
        let key = self.model.breakpoint_key_for(&row);
        if !self.breakpoints.remove(&key) {
            self.breakpoints.insert(key);
        }
    }

    /// Build the right-click context menu for one display row. Runs lazily when
    /// the menu opens, so it reads live state (selection, fold, breakpoint).
    fn build_row_menu(
        menu: PopupMenu,
        _window: &mut Window,
        cx: &mut Context<PopupMenu>,
        view: Entity<Self>,
        ix: usize,
    ) -> PopupMenu {
        let model = &view.read(cx).model;
        let row = model.display_rows[ix];
        let addr = match row {
            DisplayRow::Asm { src_idx, asm_idx } => Some(model.asm_by_src[&src_idx][asm_idx].addr),
            _ => None,
        };
        let fold_src = model.foldable_source_for_di(ix);
        let key = model.breakpoint_key_for(&row);
        let has_selection = view.read(cx).selection.is_some_and(|s| s.start != s.end);

        let mut menu = menu;
        menu = menu.item(
            PopupMenuItem::new("Copy")
                .disabled(!has_selection)
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        let text = view.read(cx).copy_text();
                        if !text.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    }
                }),
        );
        menu = menu.item(PopupMenuItem::new("Copy Line").on_click({
            let view = view.clone();
            move |_, _, cx| {
                let text = view.read(cx).model.line_text(&row).to_string();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }));
        menu = menu.when_some(addr, |menu, addr| {
            menu.item(
                PopupMenuItem::new(format!("Copy Address 0x{:x}", addr)).on_click({
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(format!("0x{:x}", addr)));
                    }
                }),
            )
        });
        menu = menu.separator();
        menu = menu.when_some(fold_src, |menu, src_idx| {
            let is_expanded = view.read(cx).model.expanded.contains(&src_idx);
            menu.item(
                PopupMenuItem::new(if is_expanded {
                    "Collapse asm"
                } else {
                    "Expand asm"
                })
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |view, cx| {
                            view.toggle_expanded(src_idx);
                            cx.notify();
                        });
                    }
                }),
            )
        });
        menu.item(
            PopupMenuItem::new(if view.read(cx).breakpoints.contains(&key) {
                "Remove Breakpoint"
            } else {
                "Add Breakpoint"
            })
            .on_click({
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |view, cx| {
                        view.toggle_breakpoint(row);
                        cx.notify();
                    });
                }
            }),
        )
    }

    /// Column hit-test against a *target* row (during drag), using real glyph
    /// metrics — shape the line and ask it where `local_x` lands.
    pub(crate) fn column_for_x(&self, di: usize, local_x: Pixels, window: &mut Window) -> usize {
        let row = self.model.display_rows[di];
        let text = self.model.line_text(&row);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let mut run = TextRun {
            len: 0,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        run.len = text.len();
        let line = window
            .text_system()
            .shape_line(text, font_size, &[run], None);
        line.closest_index_for_x(local_x)
    }

    #[cfg(debug_assertions)]
    fn verify_invariants(&self) {
        for (di, row) in self.model.display_rows.iter().enumerate() {
            if let DisplayRow::Source(idx) = *row {
                assert_eq!(
                    self.model.rowkey_to_di.get(&RowKey::Source(idx)),
                    Some(&di),
                    "rowkey_to_di disagrees with display position"
                );
                assert_eq!(
                    self.model.source_lines[idx].line_no as usize,
                    idx + 1,
                    "logical line_no must derive from identity, not display offset"
                );
            }
        }
    }

    // --- actions ---

    fn on_copy(&mut self, _: &CopyText, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.copy_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn on_debug(&mut self, _: &DebugState, _: &mut Window, _cx: &mut Context<Self>) {
        let asm_visible = self
            .model
            .display_rows
            .iter()
            .filter(|r| matches!(r, DisplayRow::Asm { .. }))
            .count();
        eprintln!(
            "[spike] display_rows={} asm_visible={} breakpoints={} selection={:?} visible_range={:?}",
            self.model.display_rows.len(),
            asm_visible,
            self.breakpoints.len(),
            self.selection_frame(),
            self.visible_range,
        );
    }

    fn on_verify(&mut self, _: &Verify, _: &mut Window, _: &mut Context<Self>) {
        #[cfg(debug_assertions)]
        {
            self.verify_invariants();
            eprintln!("[spike] invariants OK");
        }
    }

    fn on_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(CursorMove::Up, cx);
    }
    fn on_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(CursorMove::Down, cx);
    }
    fn on_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(CursorMove::Left, cx);
    }
    fn on_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(CursorMove::Right, cx);
    }
    fn on_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.extend_selection(CursorMove::Up, cx);
    }
    fn on_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.extend_selection(CursorMove::Down, cx);
    }
    fn on_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.extend_selection(CursorMove::Left, cx);
    }
    fn on_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.extend_selection(CursorMove::Right, cx);
    }
}

impl Focusable for AsmInlineView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AsmInlineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_handle.is_focused(window) && !self.blink_started {
            self.blink.update(cx, |blink, cx| blink.start(cx));
            self.blink_started = true;
        }
        // Snap the vertical scroll to whole rows (fixed row-height scrolling,
        // mirroring gpui-component Input which moves its offset by `line_height`
        // steps). GPUI's Scrollable converts wheel deltas with the un-snapped
        // `line_height` (26.0), so a wheel notch lands at 2.95 rows and every
        // scroll position drifts off the row lattice; rounding the *accumulated*
        // offset keeps rows on the device-pixel grid at any scroll position.
        // Clamp to the scrollable range only once it is known (max_offset < 0),
        // otherwise early frames would reset the offset to 0 and break scrolling.
        let scale = window.scale_factor();
        let snap_row = px((f32::from(window.line_height()) * scale).round() / scale);
        let scroll = self.scroll_handle.0.borrow().base_handle.clone();
        let off = scroll.offset();
        let row_dev = f32::from(snap_row) * scale;
        let mut snapped_y = (f32::from(off.y) * scale / row_dev).round() * row_dev / scale;
        let max_off = f32::from(scroll.max_offset().y);
        if max_off < 0.0 {
            snapped_y = snapped_y.max(max_off);
        }
        if snapped_y != f32::from(off.y) {
            scroll.set_offset(gpui::Point::new(off.x, px(snapped_y)));
        }
        let selection_frame = self.selection_frame();
        let view = cx.entity();

        div()
            .id("asm-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::editor_bg())
            .track_focus(&self.focus_handle(cx))
            .key_context("asm_view")
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_debug))
            .on_action(cx.listener(Self::on_verify))
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_select_up))
            .on_action(cx.listener(Self::on_select_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .child(self.header())
            .child(
                // A relative wrapper so the scrollbar overlays the list viewport,
                // matching the layout used by `crates/ui/src/list/list.rs`.
                div()
                    .flex()
                    .flex_col()
                    .flex_grow_1()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            "asm_rows",
                            self.model.display_rows.len(),
                            cx.processor(move |this, range: Range<usize>, window, _cx| {
                        this.visible_range = range.clone();
                        let text_style = window.text_style();
                        let font_size = text_style.font_size.to_pixels(window.rem_size());
        // Snap the row height to whole device pixels. uniform_list
        // stacks rows at `item_height * ix` and gpui snaps each
        // item's origin and height to the device-pixel grid
        // independently; a fractional row height makes the snapped
        // stride and the snapped height disagree, leaving
        // 1-device-pixel unpainted gaps at some row boundaries
        // (white hairlines through backgrounds and selections).
        // Same guarantee as gpui-component Input, which only ever
        // scrolls/positions rows on `line_height` steps.
        let scale = window.scale_factor();
        let raw_line_height: f32 = window.line_height().into();
        let row_height = px((raw_line_height * scale).round() / scale);
                        if std::env::var_os("ASM_DEBUG_BOUNDS").is_some() {
                            let measured_ih = this
                                .scroll_handle
                                .0
                                .borrow()
                                .last_item_size
                                .map(|s| {
                                    s.contents.height.as_f32()
                                        / this.model.display_rows.len().max(1) as f32
                                });
                            eprintln!(
                                "[spike] scale={:.3} raw_line_height={:.4} snapped_row_height={:.4} list_item_height={:?}",
                                scale,
                                raw_line_height,
                                f32::from(row_height),
                                measured_ih
                            );
                        }
                        let frame = selection_frame;
                        let cursor = this.cursor_anchor();
                        range
                            .clone()
                            .map(|ix| {
                                let row = this.model.display_rows[ix];
                                let is_asm = matches!(row, DisplayRow::Asm { .. });
                                let text = this.model.line_text(&row);
                                let styles = this.highlighter.row_styles(&this.model, ix);
                                let line = shape_row_text(
                                    window,
                                    text,
                                    font_size,
                                    &styles,
                                    this.highlighter.default_color(&this.model, ix),
                                    &text_style,
                                );
                                let key = this.model.breakpoint_key_for(&row);
                                let selection = frame.and_then(|f| {
                                    this.model.highlight_range(f, ix, line.text.len())
                                });
                                let cursor_col = cursor
                                    .filter(|c| this.model.rowkey_for_di(ix) == Some(c.row))
                                    .map(|c| c.column);
                                let is_active = cursor_col.is_some();
                                let selection_position = frame.and_then(|(start_di, end_di, _, _)| {
                                    if ix < start_di || ix > end_di {
                                        None
                                    } else if start_di == end_di {
                                        Some(SelectionPosition::Single)
                                    } else if ix == start_di {
                                        Some(SelectionPosition::Top)
                                    } else if ix == end_di {
                                        Some(SelectionPosition::Bottom)
                                    } else {
                                        Some(SelectionPosition::Middle)
                                    }
                                });
                                let row_view = view.clone();
                                div()
                                    .id(format!("asm-row-{}", ix))
                                    .w_full()
                                    .h(row_height)
                                    .context_menu(move |menu, window, cx| {
                                        Self::build_row_menu(menu, window, cx, row_view.clone(), ix)
                                    })
                                    .child(CodeRow {
                                        view: view.clone(),
                                        ix,
                                        is_asm,
                                        is_active,
                                        line,
                                        gutter_line: this.gutter_shaped(ix, window),
                                        fold_expanded: this
                                            .model
                                            .foldable_source_for_di(ix)
                                            .map(|src_idx| this.model.expanded.contains(&src_idx)),
                                        has_breakpoint: this.breakpoints.contains(&key),
                                        selection,
                                        selection_position,
                                        cursor_col,
                                        text_x: theme::text_x(),
                                        gutter_width: px(theme::GUTTER_WIDTH),
                                        row_height,
                                    })
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.scroll_handle)
                .w_full()
                .h_full(),
            )
            .child(
                Scrollbar::vertical(&self.scroll_handle)
                    .scrollbar_show(ScrollbarShow::Always),
            ),
        )
    }
}

impl AsmInlineView {
    fn header(&self) -> impl IntoElement {
        let asm_visible = self
            .model
            .display_rows
            .iter()
            .filter(|r| matches!(r, DisplayRow::Asm { .. }))
            .count();
        div()
            .h(px(30.0))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_2()
            .bg(theme::header_bg())
            .text_color(theme::header_fg())
            .border_b_1()
            .border_color(theme::border_color())
            .child(
                div()
                    .text_color(theme::header_dim())
                    .child("asm-inline spike"),
            )
            .child(format!("rows {}", self.model.display_rows.len()))
            .child(format!("asm {}", asm_visible))
            .child(format!("bp {}", self.breakpoints.len()))
            .child(
                div()
                    .text_color(theme::header_dim())
                    .child("gutter=断点 · 折叠=展开 asm · 拖拽=选中 · 右键=菜单"),
            )
    }

    fn gutter_shaped(&self, di: usize, window: &mut Window) -> ShapedLine {
        let text: SharedString = match self.model.display_rows[di] {
            DisplayRow::Source(idx) => {
                format!("{:>4}", self.model.source_lines[idx].line_no).into()
            }
            DisplayRow::Asm {
                src_idx,
                asm_idx: _,
            } => format!("{:>4}", self.model.source_lines[src_idx].line_no).into(),
        };
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let mut run = TextRun {
            len: 0,
            font: style.font(),
            color: theme::gutter_text(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        run.len = text.len();
        window
            .text_system()
            .shape_line(text, font_size, &[run], None)
    }
}

/// Shape a row's text into a single `ShapedLine`, applying syntax colors as
/// per-range `TextRun`s. Gaps (unstyled bytes) fall back to `default_color`.
fn shape_row_text(
    window: &mut Window,
    text: SharedString,
    font_size: Pixels,
    styles: &[(usize, usize, Hsla)],
    default_color: Hsla,
    text_style: &TextStyle,
) -> ShapedLine {
    let mut runs: Vec<TextRun> = Vec::new();
    let mut cursor = 0usize;
    for (start, end, color) in styles {
        if start >= &text.len() {
            break;
        }
        if start > &cursor {
            runs.push(make_run(&text[cursor..*start], default_color, text_style));
            cursor = *start;
        }
        let end = (*end).min(text.len());
        if end > cursor {
            runs.push(make_run(&text[cursor..end], *color, text_style));
            cursor = end;
        }
    }
    if cursor < text.len() {
        runs.push(make_run(&text[cursor..], default_color, text_style));
    }
    window
        .text_system()
        .shape_line(text, font_size, &runs, None)
}

fn make_run(text: &str, color: Hsla, style: &TextStyle) -> TextRun {
    TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}
