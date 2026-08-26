//! The surfaces. Each one consumes already-computed rows; none of them diffs.
//!
//! - [`unified`] — styled structured stdout, one column. The default, and the
//!   fallback whenever a terminal is too narrow to split.
//! - [`split`] — the product: two panes, a shared centre gutter carrying both
//!   line numbers, block colour by change kind, inline emphasis on modified
//!   pairs, and folded unchanged regions.
//! - [`json`] — the single serializer every surface shares.
//! - [`width`] — unicode column arithmetic, tab expansion and wrapping.

pub mod json;
pub mod split;
pub mod unified;
pub mod width;

use std::io::Write;

use crate::highlight::Highlighting;
use crate::model::DiffDocument;
use crate::theme::Theme;

use width::Wrap;

/// How to draw a document.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub theme: Theme,
    pub tab_width: usize,
    pub wrap: Wrap,
    /// Columns available, or `None` when the width is unknown — output is not
    /// going to a terminal, so nothing should be padded or cut to fit one.
    pub width: Option<usize>,
    /// Below this, the split view stops being readable and the unified
    /// renderer takes over. Two panes of twenty columns each are worse than
    /// one pane of fifty.
    pub min_split_width: usize,
    /// Show line numbers in the gutter.
    pub line_numbers: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            tab_width: 4,
            wrap: Wrap::default(),
            width: None,
            min_split_width: 120,
            line_numbers: true,
        }
    }
}

impl Options {
    /// Whether a split view fits. An unknown width is not a terminal, and the
    /// side-by-side layout is meaningless outside one.
    pub fn split_fits(&self) -> bool {
        self.width
            .is_some_and(|width| width >= self.min_split_width)
    }

    fn layout(&self, width: Option<usize>) -> width::Layout {
        width::Layout {
            width,
            wrap: self.wrap,
            tab_width: self.tab_width,
        }
    }
}

/// Which view to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// Split where it fits, unified where it does not.
    #[default]
    Auto,
    Split,
    Unified,
}

/// Draw a document.
///
/// `highlighting` is passed in rather than computed here because it needs the
/// full source text, which a document deliberately does not carry — see
/// [`crate::highlight::Highlighting`]. Pass [`Highlighting::none`] for no
/// syntax colour.
pub fn render(
    document: &DiffDocument,
    highlighting: &Highlighting,
    view: View,
    options: &Options,
    out: &mut impl Write,
) -> std::io::Result<()> {
    let split = match view {
        View::Split => true,
        View::Unified => false,
        View::Auto => options.split_fits(),
    };

    if split {
        split::render(document, highlighting, options, out)
    } else {
        unified::render(document, highlighting, options, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_terminal_falls_back_to_unified() {
        let options = Options {
            width: Some(80),
            min_split_width: 120,
            ..Default::default()
        };
        assert!(!options.split_fits());
    }

    #[test]
    fn an_unknown_width_is_not_a_terminal_and_does_not_split() {
        // Piped output. Padding both panes to a guessed width would produce a
        // file full of trailing spaces that no one asked for.
        let options = Options {
            width: None,
            ..Default::default()
        };
        assert!(!options.split_fits());
    }

    #[test]
    fn a_wide_terminal_splits() {
        let options = Options {
            width: Some(200),
            ..Default::default()
        };
        assert!(options.split_fits());
    }
}
