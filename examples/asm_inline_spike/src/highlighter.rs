//! Source-row syntax highlighting. This is the one place the spike *reuses*
//! gpui-component: the tree-sitter highlighter (language registration, queries,
//! theme lookup) is gpui-component's own `SyntaxHighlighter` + `HighlightTheme`.
//! Only the per-line bucketing into the display model is local.

use gpui::Hsla;
use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_component::input::Rope;

use crate::model::{CodeModel, DisplayRow};
use crate::theme;

/// Pre-computed syntax styles per source line (byte ranges + color). The
/// gpui-component `SyntaxHighlighter` + `HighlightTheme` do the real work;
/// this wrapper just buckets global byte styles back into source lines.
pub(crate) struct SourceHighlighter {
    /// (byte_start, byte_end, color) per source line, sorted.
    styles: Vec<Vec<(usize, usize, Hsla)>>,
}

impl SourceHighlighter {
    pub(crate) fn new(model: &CodeModel) -> Self {
        let document = model.source_document();
        let mut line_starts = Vec::new();
        let mut line_lens = Vec::new();
        let mut cursor = 0usize;
        for line in model.source_lines.iter() {
            line_starts.push(cursor);
            let len = line.text.len();
            line_lens.push(len);
            cursor += len + 1; // +1 for the '\n' joiner
        }

        let rope = Rope::from_str(&document);
        let mut highlighter = SyntaxHighlighter::new("c");
        // `None` builds the full-document first-parse edit internally.
        highlighter.update(None, &rope, None);

        let document_len = document.len();
        let theme = HighlightTheme::default_light();
        let global = highlighter.styles(&(0..document_len), &theme);

        // Bucket global byte styles into per-source-line local ranges.
        let mut styles = vec![Vec::new(); model.source_lines.len()];
        for (range, style) in global {
            let line = line_start_at(&line_starts, range.start);
            if line >= line_starts.len() {
                continue;
            }
            let local_start = range.start.saturating_sub(line_starts[line]);
            // Clamp to the line's own byte length so multi-line tokens (e.g. a
            // comment spanning past EOL) never bleed into the next line.
            let local_end = (range.end.saturating_sub(line_starts[line])).min(line_lens[line]);
            if local_end <= local_start {
                continue;
            }
            let color = style.color.unwrap_or_else(theme::text_source);
            styles[line].push((local_start, local_end, color));
        }

        Self { styles }
    }

    /// Styles for a display row: source rows get per-range colors; asm rows
    /// stay on the plain asm text color (a single full-length style).
    pub(crate) fn row_styles(&self, model: &CodeModel, di: usize) -> Vec<(usize, usize, Hsla)> {
        match model.display_rows[di] {
            DisplayRow::Source(src_idx) => self.styles.get(src_idx).cloned().unwrap_or_default(),
            DisplayRow::Asm { .. } => {
                let len = model.line_text(&model.display_rows[di]).len();
                vec![(0, len, theme::text_asm())]
            }
        }
    }

    /// The default color for a row kind (fallback when no styles match).
    pub(crate) fn default_color(&self, model: &CodeModel, di: usize) -> Hsla {
        match model.display_rows[di] {
            DisplayRow::Source(_) => theme::text_source(),
            DisplayRow::Asm { .. } => theme::text_asm(),
        }
    }
}

/// Find the line index whose start offset <= `offset`.
fn line_start_at(line_starts: &[usize], offset: usize) -> usize {
    line_starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AsmLine, CodeModel, SourceLine};

    fn c_model() -> CodeModel {
        let sources = vec![
            SourceLine {
                line_no: 1,
                text: "int add(int a, int b) {".into(),
            },
            SourceLine {
                line_no: 2,
                text: "    return a + b;  // sum".into(),
            },
        ];
        let mut asm = std::collections::HashMap::new();
        asm.insert(
            0,
            vec![AsmLine {
                addr: 0x100,
                text: "  mov eax, edi".into(),
            }],
        );
        CodeModel::new(sources, asm)
    }

    #[test]
    fn keywords_and_comments_get_distinct_colors() {
        let mut model = c_model();
        model.expanded.insert(0);
        model.rebuild_display_rows();
        let hl = SourceHighlighter::new(&model);
        // Source rows carry styles; the single asm row keeps the flat asm color.
        let src_styles = hl.row_styles(&model, 0);
        assert!(!src_styles.is_empty(), "source row should be highlighted");
        let asm_styles = hl.row_styles(&model, 1);
        assert_eq!(asm_styles.len(), 1, "asm row is a single flat style");
        let (s, e, color) = asm_styles[0];
        assert_eq!((s, e), (0, "  mov eax, edi".len()));
        assert_eq!(color, theme::text_asm());
    }

    #[test]
    fn source_styles_stay_within_line_bounds() {
        let model = c_model();
        let hl = SourceHighlighter::new(&model);
        for (di, src_idx) in [0usize, 1usize].iter().enumerate() {
            let len = model.source_lines[*src_idx].text.len();
            for (s, e, _) in hl.row_styles(&model, di) {
                assert!(e <= len, "style end {e} exceeds line len {len}");
                assert!(s < e, "empty style {s}..{e}");
            }
        }
    }
}
