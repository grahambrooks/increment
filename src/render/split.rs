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
//! # Layout is computed, not written
//!
//! [`compose`] turns a document into styled [`VisualRow`]s and touches no I/O.
//! [`render`] serialises those rows to a writer, and the interactive browser
//! draws the very same rows into a terminal buffer. That split is what stops
//! the two surfaces from drifting: there is one implementation of "what does
//! this diff look like", and two ways of putting it on a screen.

use std::io::Write;

use anstyle::{Color, Style};

use crate::highlight::Highlighting;
use crate::model::{DiffDocument, Line, RowKind};
use crate::theme::marker;

use super::Options;
use super::unified::{with_syntax, write_styled};
use super::width::{Piece, lay_out, width};

/// Between a pane and the gutter.
pub const SEPARATOR: char = '│';

/// A run of text and the style it draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub style: Style,
}

impl Cell {
    fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

/// One drawn line of the split view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualRow {
    pub cells: Vec<Cell>,
    /// Which row of the document this was drawn from.
    ///
    /// Several visual rows can share one document row when a line wraps, which
    /// is why the interactive browser needs this to map a screen position back
    /// to a change.
    pub source: usize,
    /// Whether this is the first visual row of that document row — the one
    /// carrying the line numbers.
    pub first: bool,
}

/// The summary line: what was compared, and how much changed.
pub fn header(document: &DiffDocument, options: &Options) -> Vec<Cell> {
    let theme = &options.theme;
    let stats = document.stats;
    let mut cells = vec![
        Cell::new(
            format!("{} → {}", document.old.name, document.new.name),
            theme.header,
        ),
        Cell::new("  ", Style::new()),
        Cell::new(format!("+{}", stats.added), theme.added),
        Cell::new(" ", Style::new()),
        Cell::new(format!("-{}", stats.removed), theme.removed),
        Cell::new(" ", Style::new()),
        Cell::new(format!("~{}", stats.modified), theme.modified),
    ];

    // A diff that is entirely a move would otherwise read `+0 -0 ~0`, which
    // says nothing happened. Shown only when there is one, so the common case
    // keeps its three counts.
    if stats.moved > 0 {
        cells.push(Cell::new(" ", Style::new()));
        cells.push(Cell::new(format!("⇄{}", stats.moved), theme.moved));
    }

    cells
}

/// Lay a document out into styled visual rows. No I/O, no terminal.
pub fn compose(
    document: &DiffDocument,
    highlighting: &Highlighting,
    options: &Options,
) -> Vec<VisualRow> {
    let theme = &options.theme;
    let total = options.width.unwrap_or(options.min_split_width);
    let geometry = Geometry::new(document, options, total);
    let mut out = Vec::new();

    for (index, row) in document.rows.iter().enumerate() {
        if let RowKind::Fold { hidden } = row.kind {
            out.push(VisualRow {
                cells: vec![Cell::new(fold_text(hidden, total), theme.fold)],
                source: index,
                first: true,
            });
            continue;
        }

        let mut styles = RowStyles::of(&row.kind, theme);
        if matches!(row.kind, RowKind::Moved { .. }) && row.left.is_some() {
            styles.glyph = marker::MOVED_FROM;
        }
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

        for visual_index in 0..height {
            // Only the first visual row carries numbers; the marker repeats, so
            // a wrapped removal still reads as a removal all the way down.
            let numbers = if visual_index == 0 {
                (
                    row.left.as_ref().map(|line| line.number),
                    row.right.as_ref().map(|line| line.number),
                )
            } else {
                (None, None)
            };

            let mut cells = Vec::new();
            pane(
                &mut cells,
                &PaneCell {
                    pieces: left.get(visual_index),
                    present: row.left.is_some(),
                    style: styles.left,
                    emphasis: styles.left_emphasis,
                    filler: theme.filler,
                    width: geometry.left,
                },
            );
            gutter(&mut cells, options, &geometry, numbers, &styles);
            pane(
                &mut cells,
                &PaneCell {
                    pieces: right.get(visual_index),
                    present: row.right.is_some(),
                    style: styles.right,
                    emphasis: styles.right_emphasis,
                    filler: theme.filler,
                    width: geometry.right,
                },
            );

            out.push(VisualRow {
                cells,
                source: index,
                first: visual_index == 0,
            });
        }
    }

    out
}

/// Draw a document to a writer.
pub fn render(
    document: &DiffDocument,
    highlighting: &Highlighting,
    options: &Options,
    out: &mut impl Write,
) -> std::io::Result<()> {
    if !document.has_changes() {
        return Ok(());
    }

    for cell in header(document, options) {
        write_styled(out, cell.style, &cell.text)?;
    }
    writeln!(out)?;

    for row in compose(document, highlighting, options) {
        for cell in &row.cells {
            write_styled(out, cell.style, &cell.text)?;
        }
        writeln!(out)?;
    }

    Ok(())
}

fn fold_text(hidden: usize, total: usize) -> String {
    let text = format!(
        " {} {hidden} unchanged line{} ",
        marker::FOLD,
        if hidden == 1 { "" } else { "s" }
    );
    // Split the remainder rather than halving it twice: an odd number of
    // columns left over would otherwise leave the fold row one column short of
    // every other row.
    let remaining = total.saturating_sub(width(&text));
    let (before, after) = (remaining / 2, remaining - remaining / 2);
    format!("{}{text}{}", "─".repeat(before), "─".repeat(after))
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
            // One tint for the whole move, on whichever side it appears, with
            // the marker saying which direction it went. Alternating by group
            // keeps two adjacent moves apart.
            RowKind::Moved { group } => {
                let tint = if group % 2 == 0 {
                    theme.moved
                } else {
                    theme.moved_alt
                };
                uniform(tint, tint, marker::MOVED_TO)
            }
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
struct PaneCell<'a> {
    pieces: Option<&'a Vec<Piece>>,
    /// Whether this side has a line at all, as opposed to having run out of
    /// visual rows because the other side wrapped further.
    present: bool,
    style: Style,
    emphasis: Style,
    filler: Style,
    width: usize,
}

/// Append one pane cell, padded to its width.
fn pane(out: &mut Vec<Cell>, cell: &PaneCell<'_>) {
    // No line on this side at all: paint the void so it reads as absence rather
    // than as the end of the file.
    let Some(pieces) = cell.pieces else {
        let style = if cell.present {
            cell.style
        } else {
            cell.filler
        };
        out.push(Cell::new(" ".repeat(cell.width), style));
        return;
    };

    let mut used = 0usize;
    for piece in pieces {
        let base = if piece.emphasis {
            cell.emphasis
        } else {
            cell.style
        };
        out.push(Cell::new(piece.text.clone(), with_syntax(base, piece)));
        used += width(&piece.text);
    }

    out.push(Cell::new(
        " ".repeat(cell.width.saturating_sub(used)),
        cell.style,
    ));
}

fn gutter(
    out: &mut Vec<Cell>,
    options: &Options,
    geometry: &Geometry,
    numbers: (Option<usize>, Option<usize>),
    styles: &RowStyles,
) {
    let theme = &options.theme;
    out.push(Cell::new(SEPARATOR.to_string(), theme.separator));

    match geometry.numbers {
        Some(number_width) => {
            let render = |number: Option<usize>| match number {
                Some(number) => format!("{number:>number_width$}"),
                None => " ".repeat(number_width),
            };
            out.push(Cell::new(format!(" {} ", render(numbers.0)), theme.gutter));
            out.push(Cell::new(styles.glyph.to_string(), styles.marker));
            out.push(Cell::new(format!(" {} ", render(numbers.1)), theme.gutter));
        }
        None => {
            out.push(Cell::new(format!(" {} ", styles.glyph), styles.marker));
        }
    }

    out.push(Cell::new(SEPARATOR.to_string(), theme.separator));
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
    fn a_diff_that_is_only_a_move_does_not_report_itself_as_empty() {
        // `+0 -0 ~0` on a header reads as "nothing happened here", which is
        // exactly wrong for a refactor that moved a function and changed
        // nothing else.
        let helper =
            "fn helper(value: u32) -> u32 {\n    let doubled = value * 2;\n    doubled + 1\n}\n";
        let caller = "fn main() {\n    let answer = helper(1);\n    println!(\"hi\");\n}\n";
        let output = plain(
            &format!("{helper}{caller}"),
            &format!("{caller}{helper}"),
            100,
        );

        let header = output.lines().next().expect("a header");
        assert!(header.contains('⇄'), "the move count is missing: {header}");
    }

    #[test]
    fn a_move_is_marked_in_both_directions() {
        let helper =
            "fn helper(value: u32) -> u32 {\n    let doubled = value * 2;\n    doubled + 1\n}\n";
        let caller = "fn main() {\n    let answer = helper(1);\n    println!(\"hi\");\n}\n";
        let output = plain(
            &format!("{helper}{caller}"),
            &format!("{caller}{helper}"),
            100,
        );

        let (_, gone, _) = row_marked(&output, marker::MOVED_FROM);
        assert!(gone.contains('<'), "{gone:?}");
        let (_, arrived, _) = row_marked(&output, marker::MOVED_TO);
        assert!(arrived.contains('>'), "{arrived:?}");
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
