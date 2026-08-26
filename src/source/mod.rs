//! Where the two sides come from.
//!
//! - [`files`] — two paths.
//! - [`git`] — blobs, index and revisions via `gix`, in pure Rust. This is the
//!   path that reads both sides *in full*, which is what folding and an honest
//!   whole-file view need in order to exist at all.
//! - [`patch`] — a unified diff on stdin, for `core.pager` compatibility.
//!
//! Every source produces the same thing: a list of [`Comparison`]s, each a pair
//! of files to be diffed. One source, one shape, so the renderers never learn
//! where a diff came from.

pub mod files;
pub mod git;
pub mod patch;

use crate::model::{DiffDocument, SourceFile};

/// Two sides to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub old: SourceFile,
    pub new: SourceFile,
}

impl Comparison {
    pub fn new(old: SourceFile, new: SourceFile) -> Self {
        Self { old, new }
    }
}

/// What a source found.
#[derive(Debug, Default)]
pub struct Changes {
    pub comparisons: Vec<Comparison>,
    /// Paths that changed but cannot be shown as text.
    ///
    /// Named rather than silently dropped: "there is a change here I am not
    /// showing you" is information, and a diff tool that omits it is lying by
    /// omission about what changed.
    pub binary: Vec<String>,
    /// Documents that were parsed rather than computed — the patch source,
    /// which describes a diff instead of holding the files to make one.
    pub documents: Vec<DiffDocument>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.comparisons.is_empty() && self.binary.is_empty() && self.documents.is_empty()
    }
}

/// Whether a byte slice looks like something a text diff can show.
///
/// The same test git uses: a NUL byte in the first few kilobytes. Cheap, and
/// wrong only for files that are already pathological.
pub fn is_binary(bytes: &[u8]) -> bool {
    const SNIFF: usize = 8000;
    bytes[..bytes.len().min(SNIFF)].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_not_binary() {
        assert!(!is_binary(b"fn main() {}\n"));
        assert!(!is_binary("named \u{540d}\u{524d}".as_bytes()));
        assert!(!is_binary(b""));
    }

    #[test]
    fn a_nul_byte_makes_it_binary() {
        assert!(is_binary(b"\x89PNG\r\n\x1a\n\x00\x00"));
    }

    #[test]
    fn a_nul_beyond_the_sniff_window_is_not_looked_for() {
        // Matching git: the check is bounded, so a very large text file with
        // one stray NUL at the end still diffs as text.
        let mut bytes = vec![b'a'; 9000];
        bytes.push(0);
        assert!(!is_binary(&bytes));
    }
}
