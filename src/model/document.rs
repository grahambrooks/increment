//! The whole comparison: what was compared, what changed, and the rows.

use serde::{Deserialize, Serialize};

use super::file::SourceFile;
use super::row::{Row, RowKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub name: String,
    pub lines: usize,
    /// Present only when true, because it is the unusual case.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing_final_newline: bool,
}

impl From<&SourceFile> for FileMeta {
    fn from(file: &SourceFile) -> Self {
        Self {
            name: file.name.clone(),
            lines: file.len(),
            missing_final_newline: file.missing_final_newline,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

impl Stats {
    pub fn any(&self) -> bool {
        self.added > 0 || self.removed > 0 || self.modified > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffDocument {
    pub old: FileMeta,
    pub new: FileMeta,
    pub stats: Stats,
    pub rows: Vec<Row>,
}

impl FileMeta {
    pub fn new(name: impl Into<String>, lines: usize) -> Self {
        Self {
            name: name.into(),
            lines,
            missing_final_newline: false,
        }
    }
}

impl DiffDocument {
    pub fn new(old: &SourceFile, new: &SourceFile, rows: Vec<Row>) -> Self {
        Self::from_meta(old.into(), new.into(), rows)
    }

    /// Build from metadata rather than from the files themselves.
    ///
    /// For sources that never hold a whole file: a patch on stdin describes
    /// only the slices git chose to emit, so there is no `SourceFile` to
    /// summarise.
    pub fn from_meta(old: FileMeta, new: FileMeta, rows: Vec<Row>) -> Self {
        let stats = rows.iter().fold(Stats::default(), |mut stats, row| {
            match row.kind {
                RowKind::Added => stats.added += 1,
                RowKind::Removed => stats.removed += 1,
                RowKind::Modified => stats.modified += 1,
                // A replacement really is both, and counting it as one would
                // under-report whichever half was dropped.
                RowKind::Replaced => {
                    stats.removed += 1;
                    stats.added += 1;
                }
                RowKind::Equal | RowKind::Fold { .. } => {}
            }
            stats
        });

        Self {
            old,
            new,
            stats,
            rows,
        }
    }

    /// Whether the two sides differ — the source of the process exit code.
    pub fn has_changes(&self) -> bool {
        self.stats.any()
    }
}
