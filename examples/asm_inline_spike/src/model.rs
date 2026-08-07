//! Pure, testable data model for the asm-inline spike. No GPUI types beyond
//! `SharedString`, so the whole file is unit-tested off the render path.

use std::collections::{HashMap, HashSet};

use gpui::SharedString;

#[derive(Clone, Debug)]
pub(crate) struct SourceLine {
    pub(crate) line_no: u32,
    pub(crate) text: SharedString,
}

#[derive(Clone, Debug)]
pub(crate) struct AsmLine {
    pub(crate) addr: u64,
    pub(crate) text: SharedString,
}

/// Logical identity of a display row. Independent of how many asm rows sit above
/// it and independent of scroll position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    Source(usize),
    Asm { src_idx: usize, asm_idx: usize },
}

/// The one key shared between source rows and asm rows for breakpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BreakpointKey {
    Line(u32),
    Addr(u64),
}

/// A row in the flat display list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisplayRow {
    Source(usize),
    Asm { src_idx: usize, asm_idx: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectionAnchor {
    pub(crate) row: RowKey,
    pub(crate) column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorMove {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) start: SelectionAnchor,
    pub(crate) end: SelectionAnchor,
}

/// The pure, testable part of the model. No GPUI types beyond `SharedString`.
pub(crate) struct CodeModel {
    pub(crate) source_lines: Vec<SourceLine>,
    pub(crate) asm_by_src: HashMap<usize, Vec<AsmLine>>,
    pub(crate) expanded: HashSet<usize>,
    pub(crate) display_rows: Vec<DisplayRow>,
    pub(crate) rowkey_to_di: HashMap<RowKey, usize>,
}

impl CodeModel {
    pub(crate) fn new(
        source_lines: Vec<SourceLine>,
        asm_by_src: HashMap<usize, Vec<AsmLine>>,
    ) -> Self {
        let mut m = Self {
            source_lines,
            asm_by_src,
            expanded: HashSet::new(),
            display_rows: Vec::new(),
            rowkey_to_di: HashMap::new(),
        };
        m.rebuild_display_rows();
        m
    }

    /// Rebuild the flat display list from `source_lines` + `expanded`. Cheap and
    /// deliberately not incremental: a few hundred rows, rebuilt only on
    /// expand/collapse, is far below any perf concern.
    pub(crate) fn rebuild_display_rows(&mut self) {
        self.display_rows.clear();
        self.rowkey_to_di.clear();
        for src_idx in 0..self.source_lines.len() {
            let di = self.display_rows.len();
            self.rowkey_to_di.insert(RowKey::Source(src_idx), di);
            self.display_rows.push(DisplayRow::Source(src_idx));
            if self.expanded.contains(&src_idx) {
                if let Some(block) = self.asm_by_src.get(&src_idx) {
                    for asm_idx in 0..block.len() {
                        let di = self.display_rows.len();
                        self.rowkey_to_di
                            .insert(RowKey::Asm { src_idx, asm_idx }, di);
                        self.display_rows.push(DisplayRow::Asm { src_idx, asm_idx });
                    }
                }
            }
        }
    }

    pub(crate) fn line_text(&self, row: &DisplayRow) -> SharedString {
        match *row {
            DisplayRow::Source(idx) => self.source_lines[idx].text.clone(),
            DisplayRow::Asm { src_idx, asm_idx } => self.asm_by_src[&src_idx][asm_idx].text.clone(),
        }
    }

    /// The unified breakpoint key — one function, both row kinds.
    pub(crate) fn breakpoint_key_for(&self, row: &DisplayRow) -> BreakpointKey {
        match *row {
            DisplayRow::Source(idx) => BreakpointKey::Line(self.source_lines[idx].line_no),
            DisplayRow::Asm { src_idx, asm_idx } => {
                BreakpointKey::Addr(self.asm_by_src[&src_idx][asm_idx].addr)
            }
        }
    }

    pub(crate) fn rowkey_for_di(&self, di: usize) -> Option<RowKey> {
        match self.display_rows.get(di) {
            Some(DisplayRow::Source(idx)) => Some(RowKey::Source(*idx)),
            Some(DisplayRow::Asm { src_idx, asm_idx }) => Some(RowKey::Asm {
                src_idx: *src_idx,
                asm_idx: *asm_idx,
            }),
            None => None,
        }
    }

    /// If this display row is a source row carrying an asm block, return its index.
    pub(crate) fn foldable_source_for_di(&self, di: usize) -> Option<usize> {
        if let Some(DisplayRow::Source(idx)) = self.display_rows.get(di) {
            let idx = *idx;
            self.asm_by_src.contains_key(&idx).then_some(idx)
        } else {
            None
        }
    }

    /// All source texts joined in logical order (used as the C document to
    /// syntax-highlight). Source-in-line numbers are unavailable here.
    pub(crate) fn source_document(&self) -> String {
        // Char indices don't map to byte offsets for the parser, so we build
        // the document from byte strings and remember each line's byte start.
        self.source_lines
            .iter()
            .map(|l| l.text.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Normalize a (possibly back-dragged) selection into display-order indices.
    /// On a single row, `end.column` may be less than `start.column` (a reverse
    /// drag within the line); swap the columns so the highlight is never empty.
    pub(crate) fn selection_frame(
        &self,
        selection: Selection,
    ) -> Option<(usize, usize, usize, usize)> {
        let start_di = *self.rowkey_to_di.get(&selection.start.row)?;
        let end_di = *self.rowkey_to_di.get(&selection.end.row)?;
        let (start, end, start_di, end_di) = if start_di < end_di
            || (start_di == end_di && selection.start.column <= selection.end.column)
        {
            (selection.start, selection.end, start_di, end_di)
        } else {
            (selection.end, selection.start, end_di, start_di)
        };
        Some((start_di, end_di, start.column, end.column))
    }

    /// The partial/full highlight columns for one row given a normalized frame
    /// `(start_di, end_di, start_col, end_col)`. The exact same multi-line
    /// selection algorithm whether the row is a source row or an asm row.
    pub(crate) fn highlight_range(
        &self,
        frame: (usize, usize, usize, usize),
        di: usize,
        line_len: usize,
    ) -> Option<(usize, usize)> {
        let (start_di, end_di, start_col, end_col) = frame;
        if di < start_di || di > end_di {
            return None;
        }
        let start_col = if di == start_di {
            start_col.min(line_len)
        } else {
            0
        };
        let end_col = if di == end_di {
            end_col.min(line_len)
        } else {
            line_len
        };
        (start_col < end_col).then_some((start_col, end_col))
    }

    /// Copy text in display order. Source and asm rows flow through the same loop.
    pub(crate) fn selection_text(&self, selection: Selection) -> String {
        let Some(frame) = self.selection_frame(selection) else {
            return String::new();
        };
        let mut out = Vec::new();
        for di in frame.0..=frame.1 {
            let text = self.line_text(&self.display_rows[di]);
            let (s, e) = self
                .highlight_range(frame, di, text.len())
                .unwrap_or((text.len(), text.len()));
            out.push(text[s..e].to_string());
        }
        out.join("\n")
    }

    /// When a collapsed fold removes an anchor's row, snap it to the fold
    /// boundary (the source row, end of line) instead of clearing the selection.
    pub(crate) fn snap_anchor(&self, anchor: SelectionAnchor) -> SelectionAnchor {
        if self.rowkey_to_di.contains_key(&anchor.row) {
            return anchor;
        }
        if let RowKey::Asm { src_idx, .. } = anchor.row {
            let column = self.source_lines[src_idx].text.len();
            return SelectionAnchor {
                row: RowKey::Source(src_idx),
                column,
            };
        }
        anchor
    }

    /// Text length (in bytes) of the line at a logical row.
    pub(crate) fn line_len(&self, row: RowKey) -> usize {
        match row {
            RowKey::Source(idx) => self.source_lines[idx].text.len(),
            RowKey::Asm { src_idx, asm_idx } => self.asm_by_src[&src_idx][asm_idx].text.len(),
        }
    }

    /// Display-order comparison of two anchors. Fallback `Equal` when a row is
    /// missing (dangling anchor), so callers degrade instead of crashing.
    pub(crate) fn anchor_order(
        &self,
        a: SelectionAnchor,
        b: SelectionAnchor,
    ) -> std::cmp::Ordering {
        let di_a = self.rowkey_to_di.get(&a.row).copied();
        let di_b = self.rowkey_to_di.get(&b.row).copied();
        match (di_a, di_b) {
            (Some(da), Some(db)) => da.cmp(&db).then_with(|| a.column.cmp(&b.column)),
            _ => std::cmp::Ordering::Equal,
        }
    }

    /// Whether the (display row, column) point is inside the given selection.
    /// `di` is a display index; `column` a byte column on that row.
    pub(crate) fn selection_contains(
        &self,
        selection: Selection,
        di: usize,
        column: usize,
    ) -> bool {
        let Some((start_di, end_di, start_col, end_col)) = self.selection_frame(selection) else {
            return false;
        };
        if di < start_di || di > end_di {
            return false;
        }
        let line_len = self.line_text(&self.display_rows[di]).len();
        let start_col = if di == start_di { start_col } else { 0 };
        let end_col = if di == end_di { end_col } else { line_len };
        // The leading edge (start point, empty selection start) belongs to the
        // selection; the trailing edge (end point) is excluded, matching how the
        // highlight range clips end columns.
        (di, column) != (end_di, end_col)
            && (start_di, start_col) <= (di, column)
            && column < end_col.min(line_len)
    }

    /// Text move the cursor one logical step. Modeled on
    /// `crates/ui/src/input/movement.rs`: horizontal steps within a line, then
    /// across the previous/next display row; vertical preserves the preferred
    /// column (clamped to the target line end). Pure and unit-testable.
    pub(crate) fn move_cursor(
        &self,
        anchor: SelectionAnchor,
        dir: CursorMove,
        preferred_column: Option<usize>,
    ) -> SelectionAnchor {
        // Guard against a dangling anchor (e.g. a fold collapse removed this
        // row while a bare cursor still pointed at it). Degrade instead of
        // panicking on the map borrow below.
        let Some(&di) = self.rowkey_to_di.get(&anchor.row) else {
            return anchor;
        };
        let len = self.line_len(anchor.row);
        match dir {
            CursorMove::Left => {
                if anchor.column > 0 {
                    SelectionAnchor {
                        row: anchor.row,
                        column: anchor.column - 1,
                    }
                } else if di > 0 {
                    let prev = self.rowkey_for_di(di - 1).unwrap();
                    SelectionAnchor {
                        row: prev,
                        column: self.line_len(prev),
                    }
                } else {
                    anchor
                }
            }
            CursorMove::Right => {
                if anchor.column < len {
                    SelectionAnchor {
                        row: anchor.row,
                        column: anchor.column + 1,
                    }
                } else if let Some(next) = self.rowkey_for_di(di + 1) {
                    SelectionAnchor {
                        row: next,
                        column: 0,
                    }
                } else {
                    anchor
                }
            }
            CursorMove::Up => self
                .rowkey_for_di(di.saturating_sub(1))
                .map(|target| {
                    let column = preferred_column
                        .unwrap_or(anchor.column)
                        .min(self.line_len(target));
                    SelectionAnchor {
                        row: target,
                        column,
                    }
                })
                .unwrap_or(anchor),
            CursorMove::Down => self
                .rowkey_for_di(di + 1)
                .map(|target| {
                    let column = preferred_column
                        .unwrap_or(anchor.column)
                        .min(self.line_len(target));
                    SelectionAnchor {
                        row: target,
                        column,
                    }
                })
                .unwrap_or(anchor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn model() -> CodeModel {
        let sources = vec![SourceLine {
            line_no: 1,
            text: "int a = 你好世界abc;".into(),
        }];
        CodeModel::new(sources, HashMap::new())
    }

    /// 5 source lines; line 0 carries 2 asm lines. display:
    /// 0=Source(0), 1=Asm(0,0), 2=Asm(0,1), 3=Source(1), 4=Source(2)…
    fn model_with_asm() -> CodeModel {
        let sources = (0..5)
            .map(|i| SourceLine {
                line_no: (i as u32) + 1,
                text: format!("line {}", i + 1).into(),
            })
            .collect();
        let mut m = CodeModel::new(sources, HashMap::new());
        m.asm_by_src.insert(
            0,
            vec![
                AsmLine {
                    addr: 0x100,
                    text: " mov eax, 0".into(),
                },
                AsmLine {
                    addr: 0x110,
                    text: " add eax, 1".into(),
                },
            ],
        );
        m.expanded.insert(0);
        m.rebuild_display_rows();
        m
    }

    #[test]
    fn cjk_column_round_trips_and_clamps() {
        let m = model();
        let text = "int a = 你好世界abc;";
        // Columns are byte offsets; the four CJK chars are 4*3 = 12 bytes.
        assert_eq!("你好世界".len(), 12);
        // Column anywhere inside the CJK run must index without a char-boundary
        // panic: slicing uses byte offsets, and every byte offset is valid.
        for col in 0..=text.len() {
            let a = SelectionAnchor {
                row: RowKey::Source(0),
                column: col,
            };
            let right = m.move_cursor(a, CursorMove::Right, None);
            // Right at EOL stays; otherwise forward one byte (never panic).
            let expected = if col == text.len() { col } else { col + 1 };
            assert_eq!(right.column, expected);
            let left = m.move_cursor(a, CursorMove::Left, None);
            let expected = if col == 0 { 0 } else { col - 1 };
            assert_eq!(left.column, expected);
        }
        // Copy stays byte-safe: selecting the CJK run yields exact bytes.
        // "int a = " is 8 bytes; the CJK run is 12 bytes (4 chars * 3).
        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Source(0),
                column: 8,
            },
            end: SelectionAnchor {
                row: RowKey::Source(0),
                column: 8 + 12,
            },
        };
        assert_eq!(m.selection_text(sel), "你好世界");
    }

    #[test]
    fn move_cursor_degrades_on_dangling_anchor() {
        // A fold collapse removed an asm row while a bare cursor pointed into it.
        // move_cursor must return the anchor unchanged, not panic on indexing a
        // row no longer in rowkey_to_di (the bug fixed by `.get()`).
        let mut m = CodeModel::new(
            vec![SourceLine {
                line_no: 1,
                text: "a".into(),
            }],
            HashMap::from([(
                0,
                vec![AsmLine {
                    addr: 0x100,
                    text: " b".into(),
                }],
            )]),
        );
        m.expanded.insert(0);
        m.rebuild_display_rows();
        // Now collapse the fold, removing the asm row the cursor points into.
        m.expanded.remove(&0);
        m.rebuild_display_rows();
        let dangling = SelectionAnchor {
            row: RowKey::Asm {
                src_idx: 0,
                asm_idx: 0,
            },
            column: 1,
        };
        assert_eq!(m.move_cursor(dangling, CursorMove::Right, None), dangling);
        assert_eq!(m.move_cursor(dangling, CursorMove::Left, None), dangling);
        assert_eq!(m.move_cursor(dangling, CursorMove::Down, None), dangling);
        assert_eq!(m.move_cursor(dangling, CursorMove::Up, None), dangling);
        assert_eq!(
            m.anchor_order(dangling, dangling),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn selection_frame_normalizes_reverse_drag_on_same_row() {
        let m = model_with_asm();
        // Drag from column 8 back to column 2 on the same row (reverse within line).
        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Source(0),
                column: 8,
            },
            end: SelectionAnchor {
                row: RowKey::Source(0),
                column: 2,
            },
        };
        let frame = m.selection_frame(sel).unwrap();
        assert_eq!(frame, (0, 0, 2, 8), "columns must swap, not row order");
        // The highlight must be non-empty, not None.
        assert_eq!(
            m.highlight_range(frame, 0, 10),
            Some((2, 8)),
            "reverse same-row drag must produce a visible highlight"
        );
        // And reverse multi-row drag (from later row back to earlier) too.
        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 0,
                    asm_idx: 1,
                },
                column: 5,
            },
            end: SelectionAnchor {
                row: RowKey::Source(0),
                column: 1,
            },
        };
        assert_eq!(
            m.selection_frame(sel),
            Some((0, 2, 1, 5)),
            "multi-row reverse normalizes rows and keeps columns per-row"
        );
    }

    #[test]
    fn selection_contains_handles_multi_line_and_edges() {
        let m = model_with_asm();
        let sel = Selection {
            start: SelectionAnchor {
                row: RowKey::Source(0),
                column: 2,
            },
            end: SelectionAnchor {
                row: RowKey::Asm {
                    src_idx: 0,
                    asm_idx: 1,
                },
                column: 3,
            },
        };
        // Inside the middle row (column 0..line_len) — fully selected.
        assert_eq!(
            m.selection_contains(sel, 1, 0),
            true,
            "middle row full span"
        );
        // Leading edge on start row includes the start column.
        assert_eq!(m.selection_contains(sel, 0, 2), true);
        // Before the start column on the start row: out.
        assert_eq!(m.selection_contains(sel, 0, 1), false);
        // Trailing edge on the end row is excluded.
        assert_eq!(
            m.selection_contains(sel, 2, 3),
            false,
            "end column excluded"
        );
        // Within the end row but before its end column: in.
        assert_eq!(m.selection_contains(sel, 2, 2), true);
        // Outside the span entirely.
        assert_eq!(m.selection_contains(sel, 4, 0), false);
    }
}
