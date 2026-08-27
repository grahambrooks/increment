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
use crate::theme::{Palette, Theme};

use width::Wrap;

/// How to draw a document.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub theme: Theme,
    /// Which palette `theme` came from, so cycling knows where it is.
    ///
    /// Set this through [`Options::set_palette`] rather than on its own: the
    /// two have to agree, and nothing else notices if they stop.
    pub palette: Palette,
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
    /// Colour tokens by syntax, where the palette leaves room for it.
    ///
    /// Here rather than decided once at startup, so it can be turned off while
    /// reading — which is the difference between a setting and a flag.
    pub syntax: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            palette: Palette::default(),
            tab_width: 4,
            wrap: Wrap::default(),
            width: None,
            min_split_width: 120,
            line_numbers: true,
            syntax: true,
        }
    }
}

/// The settings a reader can change without restarting.
///
/// Toggling lives on the options rather than in a view, because two views need
/// it — the browser owns its settings, and the review owns settings that
/// outlive the commit currently on screen.
impl Options {
    /// Choose a palette, and the theme that goes with it.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.theme = Theme::new(palette);
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap = match self.wrap {
            Wrap::Wrap => Wrap::Truncate,
            Wrap::Truncate => Wrap::Wrap,
        };
    }

    pub fn toggle_syntax(&mut self) {
        self.syntax = !self.syntax;
    }

    pub fn toggle_line_numbers(&mut self) {
        self.line_numbers = !self.line_numbers;
    }

    /// Dark, then the sixteen colours, then none, then round again.
    pub fn cycle_theme(&mut self) {
        self.set_palette(match self.palette {
            Palette::Dark => Palette::Ansi,
            Palette::Ansi => Palette::None,
            // `Auto` has already resolved to one of the others by the time
            // anyone can press a key, so it is a starting point, not a stop.
            Palette::None | Palette::Auto => Palette::Dark,
        });
    }

    /// What the status line should call the current palette.
    pub fn palette_name(&self) -> &'static str {
        match self.palette {
            Palette::Auto => "auto",
            Palette::Dark => "dark",
            Palette::Ansi => "ansi",
            Palette::None => "none",
        }
    }

    /// Whether syntax colour can be drawn at all with this palette.
    ///
    /// A palette that says "added" in the foreground has already spent the
    /// channel; see [`Theme::carries_change_in_background`].
    pub fn syntax_visible(&self) -> bool {
        self.syntax && self.theme.carries_change_in_background()
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
    fn setting_a_palette_moves_the_theme_with_it() {
        // The two are separate fields and have to agree; nothing else notices
        // if they stop, which is why they are set together.
        let mut options = Options::default();
        options.set_palette(Palette::Dark);
        assert_eq!(options.palette_name(), "dark");
        assert!(options.theme.carries_change_in_background());

        options.set_palette(Palette::Ansi);
        assert_eq!(options.palette_name(), "ansi");
        assert!(!options.theme.carries_change_in_background());
    }

    #[test]
    fn cycling_the_theme_visits_each_palette_and_returns() {
        let mut options = Options::default();
        options.set_palette(Palette::Dark);
        options.cycle_theme();
        assert_eq!(options.palette_name(), "ansi");
        options.cycle_theme();
        assert_eq!(options.palette_name(), "none");
        options.cycle_theme();
        assert_eq!(options.palette_name(), "dark");
        // …and the theme follows the palette, rather than being left behind.
        assert!(options.theme.carries_change_in_background());
    }

    #[test]
    fn syntax_colour_is_only_visible_where_the_palette_leaves_room() {
        let mut options = Options::default();
        options.set_palette(Palette::Dark);
        assert!(options.syntax_visible());

        // A foreground palette has already spent the channel.
        options.cycle_theme();
        assert!(!options.syntax_visible());

        // …and asking for it off means off, whatever the palette.
        options.set_palette(Palette::Dark);
        options.toggle_syntax();
        assert!(!options.syntax_visible());
    }

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
