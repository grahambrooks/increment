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
//! 4. [`fold`] collapses runs of unchanged rows beyond the context window.
//!
//! Nothing here knows about terminals, colour or width.

pub mod align;
pub mod engine;
pub mod fold;
pub mod inline;
pub mod tokens;

pub use engine::Algorithm;

use crate::model::{DiffDocument, SourceFile};

/// How to compute a diff.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub algorithm: Algorithm,
    /// Unchanged lines to keep either side of a change. `None` shows the whole
    /// file.
    pub context: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::default(),
            context: Some(3),
        }
    }
}

/// Compare two files.
pub fn compare(old: &SourceFile, new: &SourceFile, options: &Options) -> DiffDocument {
    let blocks = engine::blocks(&old.lines, &new.lines, options.algorithm);
    let rows = fold::fold(align::align(old, new, &blocks), options.context);
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
