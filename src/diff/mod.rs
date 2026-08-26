//! The engine layer: text in, aligned rows out.
//!
//! The pipeline, in order:
//!
//! 1. [`engine`] runs the histogram algorithm over interned lines and returns
//!    the blocks where the two sides disagree.
//! 2. [`align`] turns those blocks into display rows — filler where one side
//!    has no counterpart, and pairing of similar lines inside a block so an
//!    edit reads as one modified row rather than a removal beside an addition.
//! 3. [`inline`] computes the word-level emphasis within each pair.
//! 4. [`moves`] recognises a block deleted here and added there as one move
//!    rather than two changes, before alignment gets the chance to pair its
//!    lines with whatever happens to sit opposite them.
//! 5. [`fold`] collapses runs of unchanged rows beyond the context window.
//!
//! Nothing here knows about terminals, colour or width.

pub mod align;
pub mod engine;
pub mod fold;
pub mod inline;
pub mod moves;
pub mod tokens;

pub use engine::{Algorithm, Whitespace};

use crate::model::{DiffDocument, SourceFile};

/// How to compute a diff.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub algorithm: Algorithm,
    /// Unchanged lines to keep either side of a change. `None` shows the whole
    /// file.
    pub context: Option<usize>,
    /// How much whitespace matters when deciding whether two lines differ.
    pub whitespace: Whitespace,
    /// Recognise blocks that moved rather than reporting them as a deletion
    /// and an addition somewhere else.
    pub detect_moves: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::default(),
            context: Some(3),
            whitespace: Whitespace::default(),
            detect_moves: true,
        }
    }
}

/// Compare two files.
pub fn compare(old: &SourceFile, new: &SourceFile, options: &Options) -> DiffDocument {
    let blocks = engine::blocks(
        &old.lines,
        &new.lines,
        options.algorithm,
        options.whitespace,
    );
    let found = if options.detect_moves {
        moves::detect(old, new, &blocks)
    } else {
        moves::Moves::default()
    };
    let rows = fold::fold(align::align(old, new, &blocks, &found), options.context);
    DiffDocument::new(old, new, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_files_produce_no_changes() {
        let file = SourceFile::from_text("x", "a\nb\nc\n");
        let document = compare(&file, &file, &Options::default());
        assert!(!document.has_changes());
        assert!(document.rows.is_empty());
    }

    #[test]
    fn a_moved_block_is_reported_as_a_move_not_as_a_deletion_and_an_addition() {
        // The phase-5 criterion, end to end: a commit that moves a function
        // shows it as a move, not as a wall of deletions and an equal wall of
        // additions somewhere else.
        let helper =
            "fn helper(value: u32) -> u32 {\n    let doubled = value * 2;\n    doubled + 1\n}\n";
        let caller = "fn main() {\n    let answer = helper(1);\n    println!(\"{answer}\");\n}\n";

        let old = SourceFile::from_text("old", &format!("{helper}{caller}"));
        let new = SourceFile::from_text("new", &format!("{caller}{helper}"));

        let document = compare(
            &old,
            &new,
            &Options {
                context: None,
                ..Options::default()
            },
        );

        assert!(
            document.stats.moved > 0,
            "expected a move: {:?}",
            document.stats
        );
        assert_eq!(
            (document.stats.added, document.stats.removed),
            (0, 0),
            "the move was also counted as a deletion and an addition: {:?}",
            document.stats
        );
    }

    #[test]
    fn move_detection_can_be_turned_off() {
        let function =
            "fn helper(value: u32) -> u32 {\n    let doubled = value * 2;\n    doubled + 1\n}\n";
        let old = SourceFile::from_text("old", &format!("{function}\nfn main() {{}}\n"));
        let new = SourceFile::from_text("new", &format!("fn main() {{}}\n\n{function}"));

        let document = compare(
            &old,
            &new,
            &Options {
                detect_moves: false,
                ..Options::default()
            },
        );
        assert_eq!(document.stats.moved, 0);
        assert!(document.stats.added > 0 || document.stats.removed > 0);
    }

    #[test]
    fn ignoring_whitespace_hides_a_reindentation() {
        let old = SourceFile::from_text("old", "fn main() {\nlet x = 1;\n}\n");
        let new = SourceFile::from_text("new", "fn main() {\n    let x = 1;\n}\n");

        assert!(
            compare(&old, &new, &Options::default()).has_changes(),
            "by default, indentation is a change"
        );
        assert!(
            !compare(
                &old,
                &new,
                &Options {
                    whitespace: Whitespace::IgnoreAll,
                    ..Options::default()
                }
            )
            .has_changes(),
            "with whitespace ignored, it is not"
        );
    }

    #[test]
    fn ignoring_space_change_still_notices_a_real_edit() {
        // The point of the flag is to hide reformatting, not to hide changes
        // that happen to sit next to it.
        let old = SourceFile::from_text("old", "let x  =  1;\n");
        let new = SourceFile::from_text("new", "let x = 2;\n");
        assert!(
            compare(
                &old,
                &new,
                &Options {
                    whitespace: Whitespace::IgnoreChange,
                    ..Options::default()
                }
            )
            .has_changes()
        );
    }

    #[test]
    fn stats_count_rows_by_kind() {
        let old = SourceFile::from_text("old", "keep\ndrop\nedit me\n");
        let new = SourceFile::from_text("new", "keep\nedit me now\nadded\n");
        let document = compare(&old, &new, &Options::default());

        assert!(document.has_changes());
        assert_eq!(document.stats.removed, 1);
        assert_eq!(document.stats.modified, 1);
        assert_eq!(document.stats.added, 1);
    }
}
