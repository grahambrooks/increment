//! The unified view: one column, `diff -u` shape, styled.
//!
//! Deliberately patch-shaped. With colour off, the output is a valid unified
//! diff — the hunk headers count the lines that follow them, and applying it to
//! the old file reconstructs the new one. That is a property worth keeping: it
//! is what lets this renderer be the safe fallback for a narrow terminal, a
//! pipe, or a reader that wants to feed the result to something else.

use std::io::Write;

use anstyle::Style;

use crate::highlight::Highlighting;
use crate::model::{DiffDocument, Line, Row, RowKind};
use crate::theme::marker;

use super::Options;
use super::width::{Piece, lay_out};

pub fn render(
    document: &DiffDocument,
    highlighting: &Highlighting,
    options: &Options,
    out: &mut impl Write,
) -> std::io::Result<()> {
    let theme = &options.theme;

    if !document.has_changes() {
        return Ok(());
    }

    write_styled(out, theme.header, &format!("--- {}", document.old.name))?;
    writeln!(out)?;
    write_styled(out, theme.header, &format!("+++ {}", document.new.name))?;
    writeln!(out)?;

    for hunk in hunks(&document.rows) {
        write_styled(out, theme.fold, &hunk.header())?;
        writeln!(out)?;

        for row in hunk.rows {
            match &row.kind {
                RowKind::Equal => {
                    line(
                        out,
                        options,
                        marker::EQUAL,
                        Style::new(),
                        Style::new(),
                        left(row),
                        highlighting.old_line(left(row).number),
                    )?;
                }
                RowKind::Removed => {
                    line(
                        out,
                        options,
                        marker::REMOVED,
                        theme.removed,
                        theme.removed_emphasis,
                        left(row),
                        highlighting.old_line(left(row).number),
                    )?;
                }
                RowKind::Added => {
                    line(
                        out,
                        options,
                        marker::ADDED,
                        theme.added,
                        theme.added_emphasis,
                        right(row),
                        highlighting.new_line(right(row).number),
                    )?;
                }
                // Both are a removal followed by an addition in this view.
                // Keeping the output patch-shaped matters more here than
                // showing the pairing, which is what the split view is for.
                RowKind::Modified | RowKind::Replaced => {
                    line(
                        out,
                        options,
                        marker::REMOVED,
                        theme.removed,
                        theme.removed_emphasis,
                        left(row),
                        highlighting.old_line(left(row).number),
                    )?;
                    line(
                        out,
                        options,
                        marker::ADDED,
                        theme.added,
                        theme.added_emphasis,
                        right(row),
                        highlighting.new_line(right(row).number),
                    )?;
                }
                RowKind::Fold { .. } => unreachable!("folds delimit hunks"),
            }
        }
    }

    if document.old.missing_final_newline || document.new.missing_final_newline {
        write_styled(out, theme.fold, "\\ No newline at end of file")?;
        writeln!(out)?;
    }

    Ok(())
}

fn left(row: &Row) -> &Line {
    row.left.as_ref().expect("row has a left line")
}

fn right(row: &Row) -> &Line {
    row.right.as_ref().expect("row has a right line")
}

#[allow(clippy::too_many_arguments)]
fn line(
    out: &mut impl Write,
    options: &Options,
    marker: char,
    style: Style,
    emphasis: Style,
    line: &Line,
    colours: &[(crate::model::Span, anstyle::Color)],
) -> std::io::Result<()> {
    write_styled(out, style, &marker.to_string())?;

    // No width limit: one column of full-width text, wrapped by the terminal
    // the way `diff -u` output always has been. Cutting it here would break the
    // patch-shaped property for no benefit.
    let layout = options.layout(None);
    for (index, visual) in lay_out(&line.text, &line.emphasis, colours, &layout)
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            writeln!(out)?;
            write_styled(out, style, &marker.to_string())?;
        }
        write_pieces(out, &visual, style, emphasis)?;
    }
    writeln!(out)
}

fn write_pieces(
    out: &mut impl Write,
    pieces: &[Piece],
    style: Style,
    emphasis: Style,
) -> std::io::Result<()> {
    for piece in pieces {
        let base = if piece.emphasis { emphasis } else { style };
        write_styled(out, with_syntax(base, piece), &piece.text)?;
    }
    Ok(())
}

/// Layer a syntax colour onto a row style.
///
/// Only where the row style has not already claimed the foreground: a palette
/// that says "removed" in red must keep saying it, and a token colour painted
/// over it would leave the reader unable to tell a removal from an addition.
pub(crate) fn with_syntax(style: Style, piece: &Piece) -> Style {
    match piece.colour {
        Some(colour) if style.get_fg_color().is_none() => style.fg_color(Some(colour)),
        _ => style,
    }
}

pub(crate) fn write_styled(out: &mut impl Write, style: Style, text: &str) -> std::io::Result<()> {
    if style == Style::new() {
        return write!(out, "{text}");
    }
    write!(out, "{style}{text}{style:#}")
}

/// A run of rows between folds, with the line counts a hunk header needs.
struct Hunk<'a> {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    rows: &'a [Row],
}

impl Hunk<'_> {
    fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
    }
}

/// Split rows into hunks at the folds, counting each side.
///
/// A hunk that contains no line for one side still needs a start for it — a
/// pure insertion is `-n,0`, where `n` is the last old line before it. That is
/// why the last seen numbers are tracked across the whole walk rather than
/// derived from the hunk alone.
fn hunks(rows: &[Row]) -> Vec<Hunk<'_>> {
    let mut hunks = Vec::new();
    let (mut last_old, mut last_new) = (0usize, 0usize);
    let mut start = 0usize;

    let flush = |range: std::ops::Range<usize>, last_old: &mut usize, last_new: &mut usize| {
        let rows = &rows[range];
        if rows.is_empty() {
            return None;
        }

        let first_old = rows
            .iter()
            .find_map(|row| row.left.as_ref())
            .map(|l| l.number);
        let first_new = rows
            .iter()
            .find_map(|row| row.right.as_ref())
            .map(|l| l.number);
        let old_count = rows.iter().filter(|row| row.left.is_some()).count();
        let new_count = rows.iter().filter(|row| row.right.is_some()).count();

        if let Some(last) = rows.iter().filter_map(|row| row.left.as_ref()).next_back() {
            *last_old = last.number;
        }
        if let Some(last) = rows.iter().filter_map(|row| row.right.as_ref()).next_back() {
            *last_new = last.number;
        }

        Some(Hunk {
            old_start: first_old.unwrap_or(*last_old),
            old_count,
            new_start: first_new.unwrap_or(*last_new),
            new_count,
            rows,
        })
    };

    for (index, row) in rows.iter().enumerate() {
        if matches!(row.kind, RowKind::Fold { .. }) {
            if let Some(hunk) = flush(start..index, &mut last_old, &mut last_new) {
                hunks.push(hunk);
            }
            start = index + 1;
        }
    }
    if let Some(hunk) = flush(start..rows.len(), &mut last_old, &mut last_new) {
        hunks.push(hunk);
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Options as DiffOptions, compare};
    use crate::model::SourceFile;
    use crate::theme::Theme;

    fn plain(old: &str, new: &str) -> String {
        let old = SourceFile::from_text("a/file", old);
        let new = SourceFile::from_text("b/file", new);
        let document = compare(&old, &new, &DiffOptions::default());

        let options = Options {
            theme: Theme::none(),
            ..Default::default()
        };
        let mut buffer = Vec::new();
        render(&document, &Highlighting::none(), &options, &mut buffer).expect("renders");
        String::from_utf8(buffer).expect("utf-8")
    }

    /// Apply a unified diff to `old` and return the result.
    ///
    /// Deliberately strict: it verifies every context and removal line against
    /// the source, so a hunk header with the wrong start or a body that does
    /// not match makes it fail rather than silently produce something plausible.
    fn apply(old: &str, patch: &str) -> Result<String, String> {
        let source: Vec<&str> = old.lines().collect();
        let mut out: Vec<String> = Vec::new();
        let mut cursor = 0usize;

        for line in patch.lines() {
            if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with('\\') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("@@ -") {
                let start: usize = rest
                    .split(',')
                    .next()
                    .ok_or("malformed hunk header")?
                    .parse()
                    .map_err(|_| format!("malformed hunk header: {line}"))?;
                // Hunks are 1-based; everything skipped is unchanged context.
                let target = start.saturating_sub(1);
                if target < cursor {
                    return Err(format!("hunk goes backwards: {line}"));
                }
                out.extend(source[cursor..target].iter().map(|s| s.to_string()));
                cursor = target;
                continue;
            }

            let (marker, text) = line.split_at(1);
            match marker {
                " " | "-" => {
                    let expected = source
                        .get(cursor)
                        .ok_or_else(|| format!("patch runs past the end of the file: {line}"))?;
                    if *expected != text {
                        return Err(format!("expected {expected:?}, patch says {text:?}"));
                    }
                    if marker == " " {
                        out.push(text.to_string());
                    }
                    cursor += 1;
                }
                "+" => out.push(text.to_string()),
                _ => return Err(format!("unexpected line: {line}")),
            }
        }

        out.extend(source[cursor..].iter().map(|s| s.to_string()));
        let mut text = out.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        Ok(text)
    }

    #[test]
    fn identical_files_render_nothing() {
        assert_eq!(plain("a\nb\n", "a\nb\n"), "");
    }

    #[test]
    fn a_modified_line_renders_as_a_removal_and_an_addition() {
        let output = plain("let x = 1;\n", "let x = 2;\n");
        assert!(output.contains("-let x = 1;"), "{output}");
        assert!(output.contains("+let x = 2;"), "{output}");
    }

    #[test]
    fn the_header_names_both_sides() {
        let output = plain("a\n", "b\n");
        assert!(output.starts_with("--- a/file\n+++ b/file\n"), "{output}");
    }

    /// The property that makes this renderer the safe fallback: its output is a
    /// real patch.
    #[test]
    fn the_output_applies_back_to_the_new_file() {
        let cases = [
            ("a\nb\nc\n", "a\nB\nc\n"),
            ("a\nc\n", "a\nb\nc\n"),
            ("a\nb\nc\n", "a\nc\n"),
            ("", "a\nb\n"),
            ("a\nb\n", ""),
            (
                "one\ntwo\nthree\nfour\nfive\n",
                "one\ntwo\nTHREE\nfour\nfive\n",
            ),
            (
                "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n",
                "1\n2\n3\n4\n5\n6\n7\n8\nNINE\n10\n11\n12\n13\n14\n15\n",
            ),
            (
                &(1..=40).map(|n| format!("line {n}\n")).collect::<String>(),
                &(1..=40)
                    .map(|n| {
                        if n == 5 || n == 35 {
                            format!("LINE {n}\n")
                        } else {
                            format!("line {n}\n")
                        }
                    })
                    .collect::<String>(),
            ),
        ];

        for (old, new) in cases {
            let patch = plain(old, new);
            match apply(old, &patch) {
                Ok(result) => assert_eq!(result, new, "patch:\n{patch}"),
                Err(error) => panic!("{error}\npatch:\n{patch}"),
            }
        }
    }

    #[test]
    fn a_hunk_header_counts_the_lines_that_follow_it() {
        // Two changes far apart, so folding produces two hunks. A header whose
        // counts disagree with its body is a patch that applies to the wrong
        // place — the failure mode this renderer exists to avoid.
        let old: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=40)
            .map(|n| {
                if n == 5 || n == 35 {
                    format!("LINE {n}\n")
                } else {
                    format!("line {n}\n")
                }
            })
            .collect();

        let patch = plain(&old, &new);
        let hunks = parse_hunks(&patch);
        assert_eq!(hunks.len(), 2, "{patch}");

        for hunk in hunks {
            let old_lines = hunk
                .body
                .iter()
                .filter(|line| line.starts_with(' ') || line.starts_with('-'))
                .count();
            let new_lines = hunk
                .body
                .iter()
                .filter(|line| line.starts_with(' ') || line.starts_with('+'))
                .count();
            assert_eq!(old_lines, hunk.old_count, "old count in {}", hunk.header);
            assert_eq!(new_lines, hunk.new_count, "new count in {}", hunk.header);
        }
    }

    struct ParsedHunk<'a> {
        header: &'a str,
        old_count: usize,
        new_count: usize,
        body: Vec<&'a str>,
    }

    /// Parse `@@ -a,b +c,d @@` and the lines beneath it.
    fn parse_hunks(patch: &str) -> Vec<ParsedHunk<'_>> {
        let count = |field: &str| -> usize {
            field
                .split_once(',')
                .expect("a hunk header names both a start and a count")
                .1
                .parse()
                .expect("a numeric count")
        };

        let mut hunks: Vec<ParsedHunk<'_>> = Vec::new();
        for line in patch.lines() {
            if let Some(rest) = line.strip_prefix("@@ ") {
                let mut fields = rest.trim_end_matches(" @@").split(' ');
                let old = fields.next().expect("an old range").trim_start_matches('-');
                let new = fields.next().expect("a new range").trim_start_matches('+');
                hunks.push(ParsedHunk {
                    header: line,
                    old_count: count(old),
                    new_count: count(new),
                    body: Vec::new(),
                });
            } else if let Some(hunk) = hunks.last_mut()
                && !line.starts_with("---")
                && !line.starts_with("+++")
                && !line.starts_with('\\')
            {
                hunk.body.push(line);
            }
        }
        hunks
    }
}
