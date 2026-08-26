//! The diff document: files, changes, and the aligned rows a renderer draws.
//!
//! Pure data. No I/O, no terminal, no colour — serde-serializable end to end,
//! because `--format json` is this module's `Display`.

pub mod change;
pub mod document;
pub mod file;
pub mod row;

pub use change::{Line, Span};
pub use document::{DiffDocument, FileMeta, Stats};
pub use file::{Eol, SourceFile};
pub use row::{Row, RowKind};
