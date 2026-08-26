//! The unit every renderer draws: one display row.
//!
//! A row holds at most one line from each side. That is the whole trick behind
//! the aligned view — a deletion pads the right, an insertion pads the left,
//! and corresponding lines therefore sit level with each other in both panes.
//! The unified renderer reads the same rows and simply prints the two slots one
//! after the other.

use serde::{Deserialize, Serialize};

use super::change::Line;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RowKind {
    /// Both sides present and identical.
    Equal,
    /// Right side only.
    Added,
    /// Left side only.
    Removed,
    /// Both sides present, paired because they are similar enough to read as
    /// one line that was edited. Carries the word-level emphasis.
    Modified,
    /// Both sides present in the same position, but too different to claim one
    /// is an edit of the other: a deletion and an insertion that happen to line
    /// up. Shown side by side because that is what makes a replaced block
    /// readable — and *without* inline emphasis, because there is no
    /// correspondence to point at.
    Replaced,
    /// A line that was not deleted or added but *moved*: it appears on the
    /// other side somewhere else. The left slot means it left here, the right
    /// slot means it arrived here.
    ///
    /// `group` identifies the block it moved with, so adjacent moves can be
    /// tinted differently — otherwise two blocks that swapped places look like
    /// one block.
    Moved { group: usize },
    /// A run of equal rows that folding replaced. Neither side is present.
    Fold { hidden: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    #[serde(flatten)]
    pub kind: RowKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Line>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Line>,
}

impl Row {
    pub fn equal(left: Line, right: Line) -> Self {
        Self {
            kind: RowKind::Equal,
            left: Some(left),
            right: Some(right),
        }
    }

    pub fn added(right: Line) -> Self {
        Self {
            kind: RowKind::Added,
            left: None,
            right: Some(right),
        }
    }

    pub fn removed(left: Line) -> Self {
        Self {
            kind: RowKind::Removed,
            left: Some(left),
            right: None,
        }
    }

    pub fn modified(left: Line, right: Line) -> Self {
        Self {
            kind: RowKind::Modified,
            left: Some(left),
            right: Some(right),
        }
    }

    pub fn replaced(left: Line, right: Line) -> Self {
        Self {
            kind: RowKind::Replaced,
            left: Some(left),
            right: Some(right),
        }
    }

    pub fn moved_from(left: Line, group: usize) -> Self {
        Self {
            kind: RowKind::Moved { group },
            left: Some(left),
            right: None,
        }
    }

    pub fn moved_to(right: Line, group: usize) -> Self {
        Self {
            kind: RowKind::Moved { group },
            left: None,
            right: Some(right),
        }
    }

    pub fn fold(hidden: usize) -> Self {
        Self {
            kind: RowKind::Fold { hidden },
            left: None,
            right: None,
        }
    }

    pub fn is_change(&self) -> bool {
        matches!(
            self.kind,
            RowKind::Added
                | RowKind::Removed
                | RowKind::Modified
                | RowKind::Replaced
                | RowKind::Moved { .. }
        )
    }
}
