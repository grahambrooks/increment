//! The line-level edit script.
//!
//! `imara-diff`'s histogram algorithm, behind a small interface so the choice
//! stays replaceable. Histogram is the default rather than Myers because git's
//! own documentation is right about it: anchoring on rare lines produces more
//! readable output for large diffs, function moves and refactors, and the
//! *minimal* edit script is frequently the least readable one.

use std::ops::Range;

use imara_diff::Diff;
use imara_diff::InternedInput;

/// How much whitespace matters when deciding whether two lines differ.
///
/// Applied to the text the diff *compares*, never to the text it *shows*: a
/// line still renders exactly as it is on disk. Normalising for display would
/// turn "your reformatting is hidden" into "gdiff lied about the file".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Whitespace {
    /// Every byte counts.
    #[default]
    Respect,
    /// Runs of whitespace are one space, and the ends do not count.
    IgnoreChange,
    /// Whitespace does not count at all.
    IgnoreAll,
}

impl Whitespace {
    /// The form of a line used for comparison.
    fn normalise(self, line: &str) -> String {
        match self {
            Self::Respect => line.to_owned(),
            Self::IgnoreAll => line.chars().filter(|c| !c.is_whitespace()).collect(),
            Self::IgnoreChange => line.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }
}

/// Which edit script to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Algorithm {
    #[default]
    Histogram,
    Myers,
}

impl From<Algorithm> for imara_diff::Algorithm {
    fn from(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Histogram => imara_diff::Algorithm::Histogram,
            Algorithm::Myers => imara_diff::Algorithm::Myers,
        }
    }
}

/// A region where the two sides disagree, as half-open line ranges.
///
/// Either range may be empty: an empty `old` is a pure insertion, an empty
/// `new` a pure deletion. Blocks arrive in increasing order and never overlap,
/// so everything between two consecutive blocks is equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// Compute the change blocks between two sequences of lines.
pub fn blocks(
    old: &[String],
    new: &[String],
    algorithm: Algorithm,
    whitespace: Whitespace,
) -> Vec<Block> {
    // Interned on the normalised form, so two lines that differ only in
    // whitespace intern to the same token and the diff never sees a change.
    // The originals are untouched and are what gets drawn.
    let old: Vec<String> = old.iter().map(|line| whitespace.normalise(line)).collect();
    let new: Vec<String> = new.iter().map(|line| whitespace.normalise(line)).collect();

    let mut input = InternedInput::default();
    input.update_before(old.iter().map(String::as_str));
    input.update_after(new.iter().map(String::as_str));

    let mut diff = Diff::compute(algorithm.into(), &input);
    // Slides a hunk to the most readable of its equivalent positions — the
    // reason a diff lands on a function boundary instead of one line above it.
    diff.postprocess_lines(&input);

    diff.hunks()
        .map(|hunk| Block {
            old: hunk.before.start as usize..hunk.before.end as usize,
            new: hunk.after.start as usize..hunk.after.end as usize,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn identical_input_has_no_blocks() {
        let old = lines("a\nb\nc");
        assert!(blocks(&old, &old, Algorithm::Histogram, Whitespace::Respect).is_empty());
    }

    #[test]
    fn whitespace_modes_decide_what_counts_as_a_change() {
        let old = lines("let x  =  1;");
        let new = lines("let x = 1;");

        assert!(!blocks(&old, &new, Algorithm::Histogram, Whitespace::Respect).is_empty());
        assert!(blocks(&old, &new, Algorithm::Histogram, Whitespace::IgnoreChange).is_empty());
        assert!(blocks(&old, &new, Algorithm::Histogram, Whitespace::IgnoreAll).is_empty());
    }

    #[test]
    fn ignore_change_is_not_ignore_all() {
        // Indentation is a change in space *count* at the start of the line,
        // which `IgnoreChange` trims but `git diff -b` also ignores. The
        // distinction that matters: joining two words is not the same as
        // respacing them.
        let old = lines("ab cd");
        let new = lines("abcd");
        assert!(!blocks(&old, &new, Algorithm::Histogram, Whitespace::IgnoreChange).is_empty());
        assert!(blocks(&old, &new, Algorithm::Histogram, Whitespace::IgnoreAll).is_empty());
    }

    #[test]
    fn an_insertion_has_an_empty_old_range() {
        let blocks = blocks(
            &lines("a\nc"),
            &lines("a\nb\nc"),
            Algorithm::Histogram,
            Whitespace::Respect,
        );
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].old.is_empty());
        assert_eq!(blocks[0].new, 1..2);
    }

    #[test]
    fn a_deletion_has_an_empty_new_range() {
        let blocks = blocks(
            &lines("a\nb\nc"),
            &lines("a\nc"),
            Algorithm::Histogram,
            Whitespace::Respect,
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].old, 1..2);
        assert!(blocks[0].new.is_empty());
    }

    #[test]
    fn a_replacement_has_both_ranges() {
        let blocks = blocks(
            &lines("a\nb\nc"),
            &lines("a\nB\nc"),
            Algorithm::Histogram,
            Whitespace::Respect,
        );
        assert_eq!(
            blocks,
            [Block {
                old: 1..2,
                new: 1..2
            }]
        );
    }

    #[test]
    fn blocks_are_ordered_and_disjoint() {
        let old = lines("a\nb\nc\nd\ne\nf");
        let new = lines("a\nB\nc\nd\nE\nf");
        let blocks = blocks(&old, &new, Algorithm::Histogram, Whitespace::Respect);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].old.end <= blocks[1].old.start);
        assert!(blocks[0].new.end <= blocks[1].new.start);
    }

    #[test]
    fn both_algorithms_agree_that_something_changed() {
        // They may choose different edit scripts; neither may claim equality.
        for algorithm in [Algorithm::Histogram, Algorithm::Myers] {
            let blocks = blocks(
                &lines("a\nb"),
                &lines("a\nc"),
                algorithm,
                Whitespace::Respect,
            );
            assert!(!blocks.is_empty(), "{algorithm:?} found no change");
        }
    }
}
