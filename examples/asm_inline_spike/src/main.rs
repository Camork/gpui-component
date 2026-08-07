//! Spike: source-inline-asm view built directly on raw GPUI primitives.
//!
//! Validates one architecture hypothesis:
//!
//! > Source rows and asm rows live in two coordinate systems — *logical lines*
//! > (where line numbers live) and *display lines* (where rendering / scrolling /
//! > virtualization live) — wedged by a single flat list. Expanding or collapsing
//! > asm only changes the display-layer row set; it never shifts a source row's
//! > logical line number. Breakpoints and selection interact through *logical
//! > keys* (`RowKey` / `BreakpointKey`), not "which screen row am I on".
//!
//! The editor core (rows, selection, folds, caret) is a plain-GPUI spike. The
//! one piece that *reuses* gpui-component is source-row syntax highlighting:
//! `SyntaxHighlighter` (tree-sitter-c) + `HighlightTheme` are gpui-component's.
//!
//! Implementation notes (deliberate, cheap choices for a spike):
//! - Rows are uniform height, so GPUI's own `uniform_list` is enough; no custom
//!   virtualization was needed.
//! - Column hit-testing uses `ShapedLine::closest_index_for_x` (real glyph
//!   metrics), following `crates/gpui/examples/input.rs`.
//! - The pointer is captured on mouse-down so the origin row keeps receiving
//!   `MouseMoveEvent`s while dragging outside its own bounds.

mod blink;
mod fake_data;
mod highlighter;
mod model;
mod row;
mod theme;
mod view;

use gpui::{Bounds, Focusable, KeyBinding, WindowBounds, WindowOptions, point, prelude::*, px};
use view::{
    AsmInlineView, CopyText, MoveDown, MoveLeft, MoveRight, MoveUp, Quit, SelectDown, SelectLeft,
    SelectRight, SelectUp,
};

fn main() {
    gpui_platform::application().run(move |cx: &mut gpui::App| {
        // Init gpui-component so `PopupMenu`/`ContextMenu` have a global Theme
        // and menu keybindings (their render path reads `cx.theme()`).
        gpui_component::init(cx);

        cx.bind_keys([
            KeyBinding::new("cmd-c", CopyText, Some("asm_view")),
            KeyBinding::new("ctrl-c", CopyText, Some("asm_view")),
            KeyBinding::new("up", MoveUp, Some("asm_view")),
            KeyBinding::new("down", MoveDown, Some("asm_view")),
            KeyBinding::new("left", MoveLeft, Some("asm_view")),
            KeyBinding::new("right", MoveRight, Some("asm_view")),
            KeyBinding::new("shift-up", SelectUp, Some("asm_view")),
            KeyBinding::new("shift-down", SelectDown, Some("asm_view")),
            KeyBinding::new("shift-left", SelectLeft, Some("asm_view")),
            KeyBinding::new("shift-right", SelectRight, Some("asm_view")),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::from_corners(point(px(80.0), px(60.0)), point(px(840.0), px(720.0)));
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(AsmInlineView::new),
            )
            .unwrap();

        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx), cx);
                cx.activate(true);
            })
            .unwrap();
    });
}

#[cfg(test)]
mod tests {
    use super::model::{
        AsmLine, CodeModel, CursorMove, DisplayRow, RowKey, Selection, SelectionAnchor, SourceLine,
    };
    use std::collections::HashMap;

    fn sample_model() -> CodeModel {
        let sources = (0..5)
            .map(|i| SourceLine {
                line_no: (i as u32) + 1,
                text: format!("line {}", i + 1).into(),
            })
            .collect::<Vec<_>>();
        let mut asm = HashMap::new();
        asm.insert(
            1,
            vec![
                AsmLine {
                    addr: 0x100,
                    text: "  mov".into(),
                },
                AsmLine {
                    addr: 0x110,
                    text: "  add".into(),
                },
            ],
        );
        asm.insert(
            3,
            vec![AsmLine {
                addr: 0x200,
                text: "  ret".into(),
            }],
        );
        CodeModel::new(sources, asm)
    }

    fn expand(m: &mut CodeModel, idx: usize) {
        m.expanded.insert(idx);
        m.rebuild_display_rows();
    }

    #[test]
    fn line_numbers_are_stable_under_expand_collapse() {
        let mut m = sample_model();
        let collapsed: Vec<DisplayRow> = (0..5).map(DisplayRow::Source).collect();
        assert_eq!(m.display_rows, collapsed);

        expand(&mut m, 1);
        assert_eq!(
            m.display_rows,
            vec![
                DisplayRow::Source(0),
                DisplayRow::Source(1),
                DisplayRow::Asm {
                    src_idx: 1,
                    asm_idx: 0
                },
                DisplayRow::Asm {
                    src_idx: 1,
                    asm_idx: 1
                },
                DisplayRow::Source(2),
                DisplayRow::Source(3),
                DisplayRow::Source(4),
            ]
        );

        for (di, row) in m.display_rows.iter().enumerate() {
            if let DisplayRow::Source(idx) = *row {
                assert_eq!(m.rowkey_to_di[&RowKey::Source(idx)], di);
                assert_eq!(m.source_lines[idx].line_no as usize, idx + 1);
            }
        }

        m.expanded.remove(&1);
        m.rebuild_display_rows();
        assert_eq!(m.display_rows, collapsed);
    }

    #[test]
    fn highlight_ranges_span_source_and_asm_with_one_algorithm() {
        let mut m = sample_model();
        expand(&mut m, 1);
        expand(&mut m, 3);

        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Source(0),
                column: 1,
            },
            end: SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 3,
                    asm_idx: 0,
                },
                column: 2,
            },
        };
        let frame = m.selection_frame(sel).unwrap();
        assert_eq!(frame, (0, 6, 1, 2));

        let expect = [
            Some((1, 6)),
            Some((0, 6)),
            Some((0, 5)),
            Some((0, 5)),
            Some((0, 6)),
            Some((0, 6)),
            Some((0, 2)),
            None,
        ];
        for (di, want) in expect.iter().enumerate() {
            let text = m.line_text(&m.display_rows[di]);
            assert_eq!(m.highlight_range(frame, di, text.len()), *want, "row {di}");
        }

        let rev = Selection {
            start: SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 1,
                    asm_idx: 1,
                },
                column: 3,
            },
            end: SelectionAnchor {
                row: RowKey::Source(0),
                column: 0,
            },
        };
        let frame = m.selection_frame(rev).unwrap();
        assert_eq!(frame, (0, 3, 0, 3));
        assert_eq!(m.highlight_range(frame, 0, 5), Some((0, 5)));
        assert_eq!(m.highlight_range(frame, 3, 4), Some((0, 3)));
        assert_eq!(m.highlight_range(frame, 4, 5), None);
    }

    #[test]
    fn copy_text_joins_source_and_asm_in_display_order() {
        let mut m = sample_model();
        expand(&mut m, 1);
        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Source(0),
                column: 0,
            },
            end: SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 1,
                    asm_idx: 1,
                },
                column: 7,
            },
        };
        assert_eq!(m.selection_text(sel), "line 1\nline 2\n  mov\n  add");
    }

    #[test]
    fn anchors_snap_to_fold_boundary_on_collapse() {
        let mut m = sample_model();
        expand(&mut m, 1);
        let anchor = SelectionAnchor {
            row: RowKey::Asm {
                src_idx: 1,
                asm_idx: 1,
            },
            column: 2,
        };
        assert_eq!(m.snap_anchor(anchor), anchor);

        m.expanded.remove(&1);
        m.rebuild_display_rows();
        assert_eq!(
            m.snap_anchor(anchor),
            SelectionAnchor {
                row: RowKey::Source(1),
                column: "line 2".len()
            }
        );
    }

    #[test]
    fn cursor_moves_across_source_and_asm_rows() {
        let mut m = sample_model();
        expand(&mut m, 1);
        expand(&mut m, 3);

        let a = SelectionAnchor {
            row: RowKey::Source(1),
            column: "line 2".len(),
        };
        assert_eq!(
            m.move_cursor(a, CursorMove::Right, None),
            SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 1,
                    asm_idx: 0
                },
                column: 0
            }
        );

        let a = SelectionAnchor {
            row: RowKey::Asm {
                src_idx: 1,
                asm_idx: 0,
            },
            column: 0,
        };
        assert_eq!(
            m.move_cursor(a, CursorMove::Left, None),
            SelectionAnchor {
                row: RowKey::Source(1),
                column: "line 2".len()
            }
        );

        let a = SelectionAnchor {
            row: RowKey::Source(4),
            column: 2,
        };
        assert_eq!(
            m.move_cursor(a, CursorMove::Up, Some(2)),
            SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 3,
                    asm_idx: 0
                },
                column: 2
            }
        );
        let a = SelectionAnchor {
            row: RowKey::Source(1),
            column: 3,
        };
        assert_eq!(
            m.move_cursor(a, CursorMove::Down, Some(3)),
            SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 1,
                    asm_idx: 0
                },
                column: 3
            }
        );

        let a = SelectionAnchor {
            row: RowKey::Source(4),
            column: 0,
        };
        assert_eq!(m.move_cursor(a, CursorMove::Down, None), a);
    }
}
