//! Turning change blocks into aligned display rows.
//!
//! This is the module the project exists for. Everything else has an equivalent
//! in some other tool; the aligned two-pane view lives or dies here.
//!
//! Two jobs:
//!
//! 1. **Filler.** A row carries at most one line from each side, so a deletion
//!    leaves the right slot empty and an insertion leaves the left slot empty.
//!    Corresponding lines therefore sit level in both panes.
//! 2. **Pairing.** Inside a block where both sides have lines, decide which old
//!    line is an *edit of* which new line. A pair renders as one modified row
//!    with word-level emphasis; anything unpaired renders as a plain deletion
//!    or insertion. Without this step a replaced block is just a stack of
//!    removals above a stack of additions, which is what makes naive split
//!    views hard to read.
//!
//! The invariant, asserted by a property test: every line of both inputs
//! appears exactly once across the rows. Alignment silently dropping or
//! duplicating content is the one defect that would make the tool untrustworthy.

use crate::model::{Line, Row, SourceFile};

use super::engine::Block;
use super::inline::emphasis;
use super::tokens::similarity;

/// How alike two lines must be to read as one line that was edited rather than
/// as an unrelated removal and addition.
///
/// 0.5 — half their words in common. Lower and unrelated lines start pairing,
/// which produces confident, wrong inline highlights; higher and a genuine edit
/// that rewrote most of a line stops pairing, which loses the alignment.
const PAIR_THRESHOLD: f32 = 0.5;

/// How far ahead to look for a better anchor before giving up on a position.
///
/// Small deliberately. This rescues the common case — a line or two inserted in
/// the middle of an edited block — without turning pairing into a second,
/// slower diff that can disagree with the first one.
const LOOKAHEAD: usize = 3;

pub fn align(old: &SourceFile, new: &SourceFile, blocks: &[Block]) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut old_at, mut new_at) = (0usize, 0usize);

    for block in blocks {
        // Everything between the previous block and this one is equal.
        while old_at < block.old.start {
            rows.push(Row::equal(line(old, old_at), line(new, new_at)));
            old_at += 1;
            new_at += 1;
        }

        rows.extend(pair(old, new, block));
        old_at = block.old.end;
        new_at = block.new.end;
    }

    while old_at < old.len() {
        rows.push(Row::equal(line(old, old_at), line(new, new_at)));
        old_at += 1;
        new_at += 1;
    }

    rows
}

/// Pair the two sides of one block.
fn pair(old: &SourceFile, new: &SourceFile, block: &Block) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut i, mut j) = (block.old.start, block.new.start);

    while i < block.old.end && j < block.new.end {
        if similarity(&old.lines[i], &new.lines[j]) >= PAIR_THRESHOLD {
            rows.push(modified(old, new, i, j));
            i += 1;
            j += 1;
            continue;
        }

        match anchor(old, new, block, i, j) {
            // A better pairing exists a little further on. Whatever precedes it
            // on each side is a genuine insertion or deletion.
            Some((skip_old, skip_new)) => {
                unpaired(&mut rows, old, new, i..i + skip_old, j..j + skip_new);
                i += skip_old;
                j += skip_new;
            }
            // Nothing nearby matches: these two lines are unrelated, and saying
            // so is more useful than pairing them and inventing highlights.
            None => {
                unpaired(&mut rows, old, new, i..i + 1, j..j + 1);
                i += 1;
                j += 1;
            }
        }
    }

    for k in i..block.old.end {
        rows.push(Row::removed(line(old, k)));
    }
    for k in j..block.new.end {
        rows.push(Row::added(line(new, k)));
    }

    rows
}

/// Find the nearest pairing worth jumping to, as `(skip_old, skip_new)`.
///
/// Nearest by total distance so the smallest gap wins, and never `(0, 0)` —
/// that is the case the caller already rejected.
fn anchor(
    old: &SourceFile,
    new: &SourceFile,
    block: &Block,
    i: usize,
    j: usize,
) -> Option<(usize, usize)> {
    let mut best: Option<((usize, usize), f32)> = None;

    for skip_old in 0..=LOOKAHEAD.min(block.old.end - i) {
        for skip_new in 0..=LOOKAHEAD.min(block.new.end - j) {
            if skip_old == 0 && skip_new == 0 {
                continue;
            }
            let (Some(left), Some(right)) =
                (old.lines.get(i + skip_old), new.lines.get(j + skip_new))
            else {
                continue;
            };
            if i + skip_old >= block.old.end || j + skip_new >= block.new.end {
                continue;
            }

            let score = similarity(left, right);
            if score < PAIR_THRESHOLD {
                continue;
            }
            let closer = match best {
                None => true,
                Some(((best_old, best_new), best_score)) => {
                    (skip_old + skip_new, -score) < (best_old + best_new, -best_score)
                }
            };
            if closer {
                best = Some(((skip_old, skip_new), score));
            }
        }
    }

    best.map(|(skips, _)| skips)
}

/// Emit lines that could not be paired.
///
/// Side by side where both sides have one — a replaced block reads as a block
/// when the old and new sit level, and as twice the noise when they stack. The
/// rows are `Replaced` rather than `Modified` precisely because no
/// correspondence is being claimed: no inline emphasis, and each side keeps its
/// own colour. Whatever is left over when one side runs out is a plain deletion
/// or insertion.
fn unpaired(
    rows: &mut Vec<Row>,
    old: &SourceFile,
    new: &SourceFile,
    old_range: std::ops::Range<usize>,
    new_range: std::ops::Range<usize>,
) {
    let paired = old_range.len().min(new_range.len());
    for offset in 0..paired {
        rows.push(Row::replaced(
            line(old, old_range.start + offset),
            line(new, new_range.start + offset),
        ));
    }
    for k in old_range.start + paired..old_range.end {
        rows.push(Row::removed(line(old, k)));
    }
    for k in new_range.start + paired..new_range.end {
        rows.push(Row::added(line(new, k)));
    }
}

fn modified(old: &SourceFile, new: &SourceFile, i: usize, j: usize) -> Row {
    let (left_spans, right_spans) = emphasis(&old.lines[i], &new.lines[j]);
    Row::modified(
        line(old, i).with_emphasis(left_spans),
        line(new, j).with_emphasis(right_spans),
    )
}

fn line(file: &SourceFile, index: usize) -> Line {
    Line::new(index + 1, file.lines[index].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::engine::{Algorithm, blocks};
    use crate::model::RowKind;

    fn rows_for(old: &str, new: &str) -> Vec<Row> {
        let old = SourceFile::from_text("old", old);
        let new = SourceFile::from_text("new", new);
        let blocks = blocks(&old.lines, &new.lines, Algorithm::Histogram);
        align(&old, &new, &blocks)
    }

    fn kinds(rows: &[Row]) -> Vec<RowKind> {
        rows.iter().map(|row| row.kind.clone()).collect()
    }

    #[test]
    fn identical_files_are_all_equal_rows() {
        let rows = rows_for("a\nb\n", "a\nb\n");
        assert_eq!(kinds(&rows), [RowKind::Equal, RowKind::Equal]);
    }

    #[test]
    fn an_edited_line_becomes_one_modified_row_not_two() {
        let rows = rows_for("let x = 1;\n", "let x = 2;\n");
        assert_eq!(kinds(&rows), [RowKind::Modified]);

        let row = &rows[0];
        let left = row.left.as_ref().expect("left");
        let right = row.right.as_ref().expect("right");
        assert_eq!(
            &left.text[left.emphasis[0].start..left.emphasis[0].end],
            "1"
        );
        assert_eq!(
            &right.text[right.emphasis[0].start..right.emphasis[0].end],
            "2"
        );
    }

    #[test]
    fn an_unrelated_replacement_sits_side_by_side_without_claiming_a_pairing() {
        let rows = rows_for("alpha beta gamma\n", "impl Display for Widget {\n");
        assert_eq!(kinds(&rows), [RowKind::Replaced]);

        let row = &rows[0];
        assert_eq!(row.left.as_ref().unwrap().text, "alpha beta gamma");
        assert_eq!(
            row.right.as_ref().unwrap().text,
            "impl Display for Widget {"
        );
        // No emphasis: there is no correspondence to point at.
        assert!(row.left.as_ref().unwrap().emphasis.is_empty());
        assert!(row.right.as_ref().unwrap().emphasis.is_empty());
    }

    #[test]
    fn a_replaced_block_is_as_tall_as_its_longer_side_not_their_sum() {
        // Three unrelated lines replaced by three others: three rows, not six.
        let rows = rows_for(
            "one alpha\ntwo beta\nthree gamma\n",
            "impl A {\nfn b() {}\n}\n",
        );
        assert_eq!(
            kinds(&rows),
            [RowKind::Replaced, RowKind::Replaced, RowKind::Replaced]
        );
    }

    #[test]
    fn a_leftover_line_is_a_plain_deletion() {
        // Two old lines, one unrelated new line: one replaced row and one
        // removal, never a replaced row against nothing.
        let rows = rows_for("one alpha\ntwo beta\n", "impl A {\n");
        assert_eq!(kinds(&rows), [RowKind::Replaced, RowKind::Removed]);
    }

    #[test]
    fn an_insertion_leaves_the_left_slot_empty() {
        let rows = rows_for("a\nc\n", "a\nb\nc\n");
        assert_eq!(
            kinds(&rows),
            [RowKind::Equal, RowKind::Added, RowKind::Equal]
        );
        assert!(rows[1].left.is_none());
    }

    #[test]
    fn a_deletion_leaves_the_right_slot_empty() {
        let rows = rows_for("a\nb\nc\n", "a\nc\n");
        assert_eq!(
            kinds(&rows),
            [RowKind::Equal, RowKind::Removed, RowKind::Equal]
        );
        assert!(rows[1].right.is_none());
    }

    #[test]
    fn a_line_inserted_into_an_edited_block_does_not_derail_the_pairing() {
        // Without lookahead the insertion shifts every following line by one
        // and each subsequent pair fails the similarity test, turning a
        // two-line edit into a wall of removals and additions.
        let rows = rows_for(
            "let a = 1;\nlet b = 2;\n",
            "let a = 1;\nlet inserted = 0;\nlet b = 22;\n",
        );
        assert_eq!(
            kinds(&rows),
            [RowKind::Equal, RowKind::Added, RowKind::Modified]
        );
    }

    #[test]
    fn line_numbers_are_one_based_and_track_their_own_side() {
        let rows = rows_for("a\nc\n", "a\nb\nc\n");
        assert_eq!(rows[0].left.as_ref().unwrap().number, 1);
        assert_eq!(rows[1].right.as_ref().unwrap().number, 2);
        assert_eq!(rows[2].left.as_ref().unwrap().number, 2);
        assert_eq!(rows[2].right.as_ref().unwrap().number, 3);
    }

    /// The invariant. Every line of both inputs appears exactly once, in order.
    #[test]
    fn alignment_never_drops_or_duplicates_a_line() {
        let cases = [
            ("", ""),
            ("a\n", ""),
            ("", "a\n"),
            ("a\nb\nc\n", "a\nb\nc\n"),
            ("a\nb\nc\n", "c\nb\na\n"),
            ("one\ntwo\nthree\nfour\n", "one\nTWO\nthree\n"),
            ("x\n\n\ny\n", "x\ny\n"),
            (
                "let x = 1;\nlet y = 2;\n",
                "let x = 11;\nlet z = 3;\nlet y = 2;\n",
            ),
            ("a\na\na\na\n", "a\na\n"),
            ("fn a() {}\nfn b() {}\n", "fn b() {}\nfn a() {}\n"),
        ];

        for (old_text, new_text) in cases {
            let old = SourceFile::from_text("old", old_text);
            let new = SourceFile::from_text("new", new_text);
            let rows = align(
                &old,
                &new,
                &blocks(&old.lines, &new.lines, Algorithm::Histogram),
            );

            let seen = |slot: fn(&Row) -> Option<&Line>| -> Vec<(usize, String)> {
                rows.iter()
                    .filter_map(slot)
                    .map(|line| (line.number, line.text.clone()))
                    .collect()
            };

            let expected = |file: &SourceFile| -> Vec<(usize, String)> {
                file.lines
                    .iter()
                    .enumerate()
                    .map(|(index, text)| (index + 1, text.clone()))
                    .collect()
            };

            assert_eq!(
                seen(|row| row.left.as_ref()),
                expected(&old),
                "left side, {old_text:?} -> {new_text:?}"
            );
            assert_eq!(
                seen(|row| row.right.as_ref()),
                expected(&new),
                "right side, {old_text:?} -> {new_text:?}"
            );
        }
    }
}
