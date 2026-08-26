//! The split view: the reason this project exists.
//!
//! Two panes with a shared centre gutter carrying both line numbers, laid out
//! so corresponding lines sit level with each other — the JetBrains
//! arrangement, which is what the reference screenshot shows. The alignment
//! itself is already done by the time rows arrive here; this module's job is to
//! turn each row into columns without shearing them.
//!
//! Three things it has to get right, none of which a naive `format!` does:
//!
//! - **Filler.** Where a row has no line on one side, that pane is painted
//!   rather than left blank, so the gap reads as "nothing here" instead of as
//!   the end of the file.
//! - **Wrapping across panes.** A long line on one side wraps to several visual
//!   rows; the other side must be padded to match, or every row below it
//!   drifts out of alignment.
//! - **Column arithmetic.** Widths come from [`super::width`], never from
//!   `str::len`.
//!
//! The change map from the reference is deliberately *not* here — see the note
//! in `design/002-architecture-and-plan.md` §4. In a scrolling pager it would
//! restate the marker column; it earns its place in the interactive browser.

use std::io::Write;

use anstyle::{Color, Style};

use crate::highlight::Highlighting;
use crate::model::{DiffDocument, Line, RowKind};
use crate::theme::marker;

use super::Options;
use super::unified::{with_syntax, write_styled};
use super::width::{Piece, lay_out, width};

/// Between a pane and the gutter.
const SEPARATOR: char = '│';

pub fn render(
    document: &DiffDocument,
    highlighting: &Highlighting,
    options: &Options,
    out: &mut impl Write,
) -> std::io::Result<()> {
    if !document.has_changes() {
        return Ok(());
    }

    let theme = &options.theme;
    let total = options.width.unwrap_or(options.min_split_width);
    let geometry = Geometry::new(document, options, total);

    header(document, options, out)?;

    for row in &document.rows {
        if let RowKind::Fold { hidden } = row.kind {
            let text = format!(
                " {} {hidden} unchanged line{} ",
                marker::FOLD,
                if hidden == 1 { "" } else { "s" }
            );
            // Split the remainder rather than halving it twice: an odd
            // number of columns left over would otherwise leave the fold row
            // one column short of every other row.
            let remaining = total.saturating_sub(width(&text));
            let (before, after) = (remaining / 2, remaining - remaining / 2);
            write_styled(
                out,
                theme.fold,
                &format!("{}{text}{}", "─".repeat(before), "─".repeat(after)),
            )?;
            writeln!(out)?;
            continue;
        }

        let styles = RowStyles::of(&row.kind, theme);

        let left = visual(
            row.left.as_ref(),
            |number| highlighting.old_line(number),
            options,
            geometry.left,
        );
        let right = visual(
            row.right.as_ref(),
            |number| highlighting.new_line(number),
            options,
            geometry.right,
        );
        let height = left.len().max(right.len()).max(1);

        for index in 0..height {
            // Only the first visual row carries numbers; the marker repeats, so
            // a wrapped removal still reads as a removal all the way down.
            let numbers = if index == 0 {
                (
                    row.left.as_ref().map(|line| line.number),
                    row.right.as_ref().map(|line| line.number),
                )
            } else {
                (None, None)
            };

            let left_cell = Cell {
                pieces: left.get(index),
                present: row.left.is_some(),
                style: styles.left,
                emphasis: styles.left_emphasis,
                filler: theme.filler,
                width: geometry.left,
            };
            let right_cell = Cell {
                pieces: right.get(index),
                present: row.right.is_some(),
                style: styles.right,
                emphasis: styles.right_emphasis,
                filler: theme.filler,
                width: geometry.right,
            };

            pane(out, &left_cell)?;
            gutter(
                out,
                options,
                &geometry,
                numbers,
                styles.glyph,
                styles.marker,
            )?;
            pane(out, &right_cell)?;
            writeln!(out)?;
        }
    }

    Ok(())
}

/// The styles one row draws with.
///
/// Held per side rather than per row, because the two most interesting kinds
/// are asymmetric: a modified row emphasises what the left lost and what the
/// right gained, and a replaced row is a deletion beside an insertion with no
/// shared colour at all.
#[derive(Debug, Clone, Copy)]
struct RowStyles {
    left: Style,
    left_emphasis: Style,
    right: Style,
    right_emphasis: Style,
    glyph: char,
    marker: Style,
}

impl RowStyles {
    fn of(kind: &RowKind, theme: &crate::theme::Theme) -> Self {
        let uniform = |style: Style, emphasis: Style, glyph: char| Self {
            left: style,
            left_emphasis: emphasis,
            right: style,
            right_emphasis: emphasis,
            glyph,
            marker: style,
        };

        match kind {
            RowKind::Equal => uniform(Style::new(), Style::new(), marker::EQUAL),
            RowKind::Added => uniform(theme.added, theme.added_emphasis, marker::ADDED),
            RowKind::Removed => uniform(theme.removed, theme.removed_emphasis, marker::REMOVED),
            // One row, one colour, but the emphasis points in each direction:
            // what this line lost on the left, what it gained on the right.
            RowKind::Modified => Self {
                left: theme.modified,
                left_emphasis: theme.removed_emphasis,
                right: theme.modified,
                right_emphasis: theme.added_emphasis,
                glyph: marker::MODIFIED,
                marker: theme.modified,
            },
            // Not an edit of each other, so each side keeps its own colour and
            // neither carries emphasis.
            RowKind::Replaced => Self {
                left: theme.removed,
                left_emphasis: theme.removed,
                right: theme.added,
                right_emphasis: theme.added,
                glyph: marker::REPLACED,
                marker: theme.modified,
            },
            RowKind::Fold { .. } => unreachable!("folds are drawn before this point"),
        }
    }
}

/// Column budget for one rendering.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    left: usize,
    right: usize,
    numbers: Option<usize>,
}

impl Geometry {
    fn new(document: &DiffDocument, options: &Options, total: usize) -> Self {
        let numbers = options
            .line_numbers
            .then(|| digits(document.old.lines.max(document.new.lines)));

        // ` 123 ~ 456 ` plus a separator either side.
        let gutter = gutter_width(numbers) + 2;
        let panes = total.saturating_sub(gutter).max(2);

        Self {
            left: panes / 2,
            // The odd column goes to the right pane rather than being dropped.
            right: panes - panes / 2,
            numbers,
        }
    }
}

/// ` 123 ~ 456 `, or ` ~ ` when line numbers are off.
fn gutter_width(numbers: Option<usize>) -> usize {
    numbers.map_or(3, |width| 2 * width + 5)
}

fn digits(n: usize) -> usize {
    n.max(1).to_string().len()
}

/// Lay one side of a row out into visual rows of pieces.
fn visual<'a>(
    line: Option<&Line>,
    colours: impl Fn(usize) -> &'a [(crate::model::Span, Color)],
    options: &Options,
    pane_width: usize,
) -> Vec<Vec<Piece>> {
    let Some(line) = line else {
        return Vec::new();
    };
    lay_out(
        &line.text,
        &line.emphasis,
        colours(line.number),
        &options.layout(Some(pane_width)),
    )
}

/// One pane's worth of one visual row.
struct Cell<'a> {
    pieces: Option<&'a Vec<Piece>>,
    /// Whether this side has a line at all, as opposed to having run out of
    /// visual rows because the other side wrapped further.
    present: bool,
    style: Style,
    emphasis: Style,
    filler: Style,
    width: usize,
}

/// Write one pane cell, padded to its width.
fn pane(out: &mut impl Write, cell: &Cell<'_>) -> std::io::Result<()> {
    // No line on this side at all: paint the void so it reads as absence rather
    // than as the end of the file.
    let Some(pieces) = cell.pieces else {
        let style = if cell.present {
            cell.style
        } else {
            cell.filler
        };
        return write_styled(out, style, &" ".repeat(cell.width));
    };

    let mut used = 0usize;
    for piece in pieces {
        let base = if piece.emphasis {
            cell.emphasis
        } else {
            cell.style
        };
        write_styled(out, with_syntax(base, piece), &piece.text)?;
        used += width(&piece.text);
    }

    write_styled(
        out,
        cell.style,
        &" ".repeat(cell.width.saturating_sub(used)),
    )
}

fn gutter(
    out: &mut impl Write,
    options: &Options,
    geometry: &Geometry,
    numbers: (Option<usize>, Option<usize>),
    glyph: char,
    row_style: Style,
) -> std::io::Result<()> {
    let theme = &options.theme;
    write_styled(out, theme.separator, &SEPARATOR.to_string())?;

    match geometry.numbers {
        Some(number_width) => {
            let render = |number: Option<usize>| match number {
                Some(number) => format!("{number:>number_width$}"),
                None => " ".repeat(number_width),
            };
            write_styled(out, theme.gutter, &format!(" {} ", render(numbers.0)))?;
            write_styled(out, row_style, &glyph.to_string())?;
            write_styled(out, theme.gutter, &format!(" {} ", render(numbers.1)))?;
        }
        None => {
            write_styled(out, row_style, &format!(" {glyph} "))?;
        }
    }

    write_styled(out, theme.separator, &SEPARATOR.to_string())
}

fn header(document: &DiffDocument, options: &Options, out: &mut impl Write) -> std::io::Result<()> {
    let theme = &options.theme;
    let stats = document.stats;
    write_styled(
        out,
        theme.header,
        &format!("{} → {}", document.old.name, document.new.name),
    )?;
    write!(out, "  ")?;
    write_styled(out, theme.added, &format!("+{}", stats.added))?;
    write!(out, " ")?;
    write_styled(out, theme.removed, &format!("-{}", stats.removed))?;
    write!(out, " ")?;
    write_styled(out, theme.modified, &format!("~{}", stats.modified))?;
    writeln!(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Options as DiffOptions, compare};
    use crate::model::SourceFile;
    use crate::render::width::Wrap;
    use crate::theme::Theme;

    fn plain(old: &str, new: &str, terminal_width: usize) -> String {
        render_with(old, new, terminal_width, Wrap::Wrap, DiffOptions::default())
    }

    fn render_with(
        old: &str,
        new: &str,
        terminal_width: usize,
        wrap: Wrap,
        diff_options: DiffOptions,
    ) -> String {
        let old = SourceFile::from_text("old", old);
        let new = SourceFile::from_text("new", new);
        let document = compare(&old, &new, &diff_options);

        let options = Options {
            theme: Theme::none(),
            width: Some(terminal_width),
            wrap,
            ..Default::default()
        };
        let mut buffer = Vec::new();
        render(&document, &Highlighting::none(), &options, &mut buffer).expect("renders");
        String::from_utf8(buffer).expect("utf-8")
    }

    /// Every rendered row is exactly as wide as the terminal. This is the one
    /// that catches shearing, and it is why the width tests exist at all.
    fn assert_every_row_is_exactly(output: &str, columns: usize) {
        for line in body(output) {
            assert_eq!(
                width(line),
                columns,
                "row {line:?} is {} columns, not {columns}",
                width(line)
            );
        }
    }

    /// The rows, without the header line.
    fn body(output: &str) -> impl Iterator<Item = &str> {
        output.lines().skip(1)
    }

    /// Split a rendered row into its three columns.
    fn columns(row: &str) -> Option<(&str, &str, &str)> {
        let mut parts = row.split(SEPARATOR);
        Some((parts.next()?, parts.next()?, parts.next()?))
    }

    /// The first row whose gutter carries `glyph`.
    fn row_marked(output: &str, glyph: char) -> (&str, &str, &str) {
        body(output)
            .filter_map(columns)
            .find(|(_, gutter, _)| gutter.contains(glyph))
            .unwrap_or_else(|| panic!("no row marked {glyph:?} in:\n{output}"))
    }

    #[test]
    fn both_line_numbers_appear_in_the_centre_gutter() {
        // Line 2 on each side, either side of the marker — the arrangement the
        // reference screenshot uses, and the reason the gutter is central.
        let output = plain("a\nlet b = 1;\nc\n", "a\nlet b = 2;\nc\n", 60);
        let (left, gutter, right) = row_marked(&output, marker::MODIFIED);

        assert_eq!(gutter.trim(), "2 ~ 2", "{gutter:?}");
        assert!(left.contains("let b = 1;"), "{left:?}");
        assert!(right.contains("let b = 2;"), "{right:?}");
    }

    #[test]
    fn line_numbers_can_be_turned_off() {
        let old = SourceFile::from_text("old", "a\nlet b = 1;\n");
        let new = SourceFile::from_text("new", "a\nlet b = 2;\n");
        let document = compare(&old, &new, &DiffOptions::default());
        let options = Options {
            theme: Theme::none(),
            width: Some(60),
            line_numbers: false,
            ..Default::default()
        };
        let mut buffer = Vec::new();
        render(&document, &Highlighting::none(), &options, &mut buffer).expect("renders");
        let output = String::from_utf8(buffer).expect("utf-8");

        let (_, gutter, _) = row_marked(&output, marker::MODIFIED);
        assert_eq!(gutter, " ~ ");
        assert_every_row_is_exactly(&output, 60);
    }

    #[test]
    fn a_deletion_leaves_a_painted_gap_on_the_right() {
        let output = plain("a\nb\nc\n", "a\nc\n", 60);
        let (left, gutter, right) = row_marked(&output, marker::REMOVED);

        assert!(left.contains('b'), "{left:?}");
        // Present but empty: the gap is painted, not collapsed.
        assert_eq!(right.trim(), "", "{right:?}");
        assert!(
            !right.is_empty(),
            "the right pane should still occupy its columns"
        );
        // Only the old number is known on a removed row.
        assert_eq!(gutter.trim(), "2 -", "{gutter:?}");
    }

    #[test]
    fn every_row_is_exactly_the_terminal_width() {
        for columns in [80, 120, 200] {
            let output = plain(
                "alpha\nbeta\ngamma\n",
                "alpha\nBETA changed\ngamma\ndelta\n",
                columns,
            );
            assert_every_row_is_exactly(&output, columns);
        }
    }

    #[test]
    fn a_fold_row_is_as_wide_as_every_other_row() {
        // Halving the leftover columns and using it twice leaves an odd width
        // one column short, which is invisible until it sits next to a real
        // row. Every listed width here is one the snapshots also cover.
        let old: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=40)
            .map(|n| {
                if n == 20 {
                    "CHANGED entirely\n".to_owned()
                } else {
                    format!("line {n}\n")
                }
            })
            .collect();

        for columns in [79, 80, 81, 120, 121, 200] {
            let output = plain(&old, &new, columns);
            assert!(output.contains('⋯'), "expected a fold at {columns}");
            assert_every_row_is_exactly(&output, columns);
        }
    }

    #[test]
    fn wide_characters_do_not_shear_the_panes() {
        let output = plain("名前 = 1\nb\n", "名前 = 2\nb\n", 80);
        assert_every_row_is_exactly(&output, 80);
    }

    #[test]
    fn a_wrapped_line_keeps_both_sides_aligned() {
        // Many words, so the two long lines pair as one modified row and both
        // sides wrap. If the shorter side were not padded to match, every row
        // below would drift by the difference.
        let long: String = (1..=60).map(|n| format!("word{n} ")).collect();
        let output = plain(
            &format!("{long}\ntail\n"),
            &format!("{long}extra\ntail\n"),
            80,
        );
        assert_every_row_is_exactly(&output, 80);

        let tail_rows = body(&output)
            .filter_map(columns)
            .filter(|(left, _, _)| left.trim() == "tail")
            .count();
        assert_eq!(tail_rows, 1, "{output}");

        // And the two panes produced the same number of visual rows.
        let wrapped = body(&output)
            .filter_map(columns)
            .filter(|(left, _, _)| left.contains("word"))
            .count();
        assert!(wrapped > 1, "the line should have wrapped:\n{output}");
    }

    #[test]
    fn truncation_keeps_the_row_width() {
        let long = "x".repeat(300);
        let output = render_with(
            &format!("{long}\n"),
            &format!("{long}y\n"),
            80,
            Wrap::Truncate,
            DiffOptions::default(),
        );
        assert_every_row_is_exactly(&output, 80);
    }

    #[test]
    fn a_fold_says_how_many_lines_it_hides() {
        let old: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=40)
            .map(|n| {
                if n == 20 {
                    "CHANGED\n".to_owned()
                } else {
                    format!("line {n}\n")
                }
            })
            .collect();

        let output = plain(&old, &new, 80);
        assert!(output.contains("unchanged lines"), "{output}");
        assert!(output.contains('⋯'), "{output}");
    }

    #[test]
    fn identical_files_render_nothing() {
        assert_eq!(plain("a\n", "a\n", 80), "");
    }

    #[test]
    fn the_header_carries_the_names_and_the_counts() {
        let output = plain("a\n", "b\n", 80);
        let header = output.lines().next().expect("a header");
        assert!(header.contains("old → new"), "{header}");
        assert!(header.contains("+1"), "{header}");
        assert!(header.contains("-1"), "{header}");
    }
}
