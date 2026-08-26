//! Column arithmetic and line layout.
//!
//! Every renderer measures text through this module and none of them counts
//! `char`s. A CJK ideograph occupies two columns, a combining mark occupies
//! none, and a tab occupies whatever it takes to reach the next tab stop — get
//! any of them wrong in a two-pane layout and the right-hand pane shears.
//!
//! Layout also owns emphasis, because the two problems are the same problem:
//! the emphasised regions arrive as byte ranges into the original text, and
//! tab expansion, wrapping and truncation all move those bytes.

use anstyle::Color;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::Span;

/// The visual width of a string in terminal columns.
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// A run of text that is uniformly emphasised and uniformly coloured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub emphasis: bool,
    /// The syntax colour of this run, if the language was recognised. A
    /// *foreground* only — the row's own colour lives in the background, and
    /// the two compose.
    pub colour: Option<Color>,
}

impl Piece {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            emphasis: false,
            colour: None,
        }
    }
}

/// What to do with a line too wide for its pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wrap {
    /// Continue on the next visual row.
    #[default]
    Wrap,
    /// Cut, marking the cut with an ellipsis.
    Truncate,
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// Available columns, or `None` for unlimited — the unified renderer lets
    /// the terminal do its own wrapping.
    pub width: Option<usize>,
    pub wrap: Wrap,
    pub tab_width: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: None,
            wrap: Wrap::default(),
            tab_width: 4,
        }
    }
}

/// The glyph marking a truncated line.
pub const ELLIPSIS: &str = "…";

/// Lay a line out into visual rows of pieces.
///
/// Always returns at least one row, so an empty line still occupies one.
///
/// `spans` are the emphasised regions and `colours` the syntax colours, both as
/// byte ranges into `text`. They are applied here rather than by a renderer
/// because tab expansion, wrapping and truncation all move the bytes they refer
/// to; resolving them anywhere else means resolving them against the wrong
/// offsets.
pub fn lay_out(
    text: &str,
    spans: &[Span],
    colours: &[(Span, Color)],
    layout: &Layout,
) -> Vec<Vec<Piece>> {
    let mut rows: Vec<Vec<Piece>> = Vec::new();
    let mut row: Vec<Piece> = Vec::new();
    let mut column = 0usize;

    // Room for the ellipsis itself, so a cut line is exactly as wide as the
    // pane rather than one column over it.
    let limit = layout.width;
    let cut_at = limit.map(|limit| limit.saturating_sub(width(ELLIPSIS)));

    for (offset, grapheme) in text.grapheme_indices(true) {
        let emphasis = spans
            .iter()
            .any(|span| offset >= span.start && offset < span.end);
        let colour = colours
            .iter()
            .find(|(span, _)| offset >= span.start && offset < span.end)
            .map(|(_, colour)| *colour);

        // Expanded here rather than earlier so the stop is measured in real
        // columns, after any wide characters before it on this row.
        let expanded = if grapheme == "\t" {
            let stop = layout.tab_width.max(1);
            " ".repeat(stop - (column % stop))
        } else {
            grapheme.to_owned()
        };
        let advance = width(&expanded).max(usize::from(!expanded.is_empty() && grapheme == "\t"));

        if let Some(limit) = limit
            && column + advance > limit
        {
            match layout.wrap {
                Wrap::Wrap => {
                    rows.push(std::mem::take(&mut row));
                    column = 0;
                }
                Wrap::Truncate => {
                    trim_to(&mut row, cut_at.unwrap_or(0), &mut column);
                    push(&mut row, ELLIPSIS.to_owned(), false, None);
                    rows.push(row);
                    return rows;
                }
            }
        }

        column += advance;
        push(&mut row, expanded, emphasis, colour);
    }

    rows.push(row);
    rows
}

/// Append, joining onto the previous piece when nothing about the style changed.
fn push(row: &mut Vec<Piece>, text: String, emphasis: bool, colour: Option<Color>) {
    match row.last_mut() {
        Some(last) if last.emphasis == emphasis && last.colour == colour => {
            last.text.push_str(&text)
        }
        _ => row.push(Piece {
            text,
            emphasis,
            colour,
        }),
    }
}

/// Drop trailing graphemes until the row fits in `target` columns.
fn trim_to(row: &mut Vec<Piece>, target: usize, column: &mut usize) {
    while *column > target {
        let Some(last) = row.last_mut() else { break };
        let Some((offset, grapheme)) = last.text.grapheme_indices(true).next_back() else {
            row.pop();
            continue;
        };
        *column = column.saturating_sub(width(grapheme));
        last.text.truncate(offset);
        if last.text.is_empty() {
            row.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str, layout: &Layout) -> Vec<String> {
        lay_out(text, &[], &[], layout)
            .into_iter()
            .map(|row| row.into_iter().map(|piece| piece.text).collect())
            .collect()
    }

    #[test]
    fn a_cjk_character_is_two_columns_wide() {
        assert_eq!(width("名前"), 4);
        assert_eq!(width("ab"), 2);
    }

    #[test]
    fn tabs_expand_to_the_next_tab_stop_not_a_fixed_width() {
        let layout = Layout {
            tab_width: 4,
            ..Default::default()
        };
        // Column 0 -> 4 spaces; after "ab" -> 2 spaces to reach column 4.
        assert_eq!(plain("\tx", &layout), ["    x"]);
        assert_eq!(plain("ab\tx", &layout), ["ab  x"]);
    }

    #[test]
    fn a_line_wraps_at_the_pane_width() {
        let layout = Layout {
            width: Some(4),
            wrap: Wrap::Wrap,
            tab_width: 4,
        };
        assert_eq!(plain("abcdefgh", &layout), ["abcd", "efgh"]);
    }

    #[test]
    fn truncation_leaves_room_for_the_ellipsis() {
        let layout = Layout {
            width: Some(5),
            wrap: Wrap::Truncate,
            tab_width: 4,
        };
        let rows = plain("abcdefgh", &layout);
        assert_eq!(rows, ["abcd…"]);
        assert_eq!(width(&rows[0]), 5);
    }

    #[test]
    fn a_wide_character_never_straddles_the_pane_edge() {
        // Three columns of room and a two-column character: it must move to the
        // next row whole, not be cut in half.
        let layout = Layout {
            width: Some(3),
            wrap: Wrap::Wrap,
            tab_width: 4,
        };
        let rows = plain("ab名", &layout);
        assert_eq!(rows, ["ab", "名"]);
        assert!(rows.iter().all(|row| width(row) <= 3));
    }

    #[test]
    fn an_empty_line_still_occupies_one_row() {
        assert_eq!(plain("", &Layout::default()), [""]);
    }

    #[test]
    fn emphasis_follows_the_text_through_tab_expansion() {
        let layout = Layout {
            tab_width: 4,
            ..Default::default()
        };
        // Emphasise "x", which sits after a tab.
        let rows = lay_out("\tx", &[Span::new(1, 2)], &[], &layout);
        assert_eq!(
            rows[0],
            [
                Piece::plain("    "),
                Piece {
                    text: "x".into(),
                    emphasis: true,
                    colour: None,
                },
            ]
        );
    }

    #[test]
    fn adjacent_graphemes_of_the_same_emphasis_become_one_piece() {
        let rows = lay_out("hello", &[Span::new(0, 5)], &[], &Layout::default());
        assert_eq!(rows[0].len(), 1);
        assert!(rows[0][0].emphasis);
    }

    #[test]
    fn a_colour_change_starts_a_new_piece() {
        let red = Color::Ansi(anstyle::AnsiColor::Red);
        let rows = lay_out("abcd", &[], &[(Span::new(0, 2), red)], &Layout::default());
        assert_eq!(rows[0].len(), 2, "{:?}", rows[0]);
        assert_eq!(rows[0][0].colour, Some(red));
        assert_eq!(rows[0][1].colour, None);
    }

    #[test]
    fn syntax_colour_survives_wrapping() {
        let red = Color::Ansi(anstyle::AnsiColor::Red);
        let layout = Layout {
            width: Some(2),
            wrap: Wrap::Wrap,
            tab_width: 4,
        };
        // The coloured region straddles the wrap point.
        let rows = lay_out("abcd", &[], &[(Span::new(1, 3), red)], &layout);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].last().unwrap().colour, Some(red));
        assert_eq!(rows[1].first().unwrap().colour, Some(red));
    }
}
