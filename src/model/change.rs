//! Lines and the emphasised regions within them.

use serde::{Deserialize, Serialize};

/// A byte range within a line's text, marking the part that actually changed.
///
/// Byte offsets rather than character indices because that is what slicing a
/// `&str` needs, and every span produced here falls on a UTF-8 boundary — the
/// word tokenizer only ever splits at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// One line of one side, as it appears in a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// 1-based line number within its own file.
    pub number: usize,
    pub text: String,
    /// The parts of `text` that differ from the line it is paired with.
    /// Always empty unless the row is `Modified`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emphasis: Vec<Span>,
}

impl Line {
    pub fn new(number: usize, text: impl Into<String>) -> Self {
        Self {
            number,
            text: text.into(),
            emphasis: Vec::new(),
        }
    }

    pub fn with_emphasis(mut self, emphasis: Vec<Span>) -> Self {
        self.emphasis = emphasis;
        self
    }
}
