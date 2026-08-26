//! Argument definitions.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::diff;
use crate::render;
use crate::render::width::Wrap;
use crate::theme;

use super::surface;

#[derive(Debug, Parser)]
#[command(
    name = "gdiff",
    version,
    about = "The aligned side-by-side diff view, in the terminal.",
    long_about = None,
)]
pub struct Args {
    /// The left-hand side.
    pub old: PathBuf,

    /// The right-hand side.
    pub new: PathBuf,

    /// Output surface. `auto` never selects `tui` — see the design.
    #[arg(long, value_enum, default_value_t = Ui::Auto)]
    pub ui: Ui,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Which view to draw. `auto` splits where the terminal is wide enough.
    #[arg(long, value_enum, default_value_t = View::Auto)]
    pub view: View,

    /// Edit script to compute.
    #[arg(long, value_enum, default_value_t = Algorithm::Histogram)]
    pub algorithm: Algorithm,

    /// Unchanged lines to keep either side of a change.
    #[arg(short = 'U', long, default_value_t = 3, value_name = "LINES")]
    pub context: usize,

    /// Show every unchanged line instead of folding.
    #[arg(long, conflicts_with = "context")]
    pub full: bool,

    /// What to do with a line too wide for its pane.
    #[arg(long, value_enum, default_value_t = WrapMode::Wrap)]
    pub wrap: WrapMode,

    /// Columns a tab advances to.
    #[arg(long, default_value_t = 4, value_name = "COLUMNS")]
    pub tab_width: usize,

    /// Colour palette.
    #[arg(long, value_enum, default_value_t = Theme::Auto)]
    pub theme: Theme,

    /// When to emit colour.
    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,

    /// Override the detected terminal width.
    #[arg(long, value_name = "COLUMNS")]
    pub width: Option<usize>,

    /// Narrower than this, the split view gives way to the unified one.
    #[arg(long, default_value_t = 120, value_name = "COLUMNS")]
    pub min_split_width: usize,

    /// Hide the line numbers in the centre gutter.
    #[arg(long)]
    pub no_line_numbers: bool,

    /// Syntax highlighting. `auto` enables it only where the palette leaves
    /// the foreground free.
    #[arg(long, value_enum, default_value_t = Syntax::Auto)]
    pub syntax: Syntax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Ui {
    Auto,
    Plain,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum View {
    Auto,
    Split,
    Unified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Algorithm {
    Histogram,
    Myers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WrapMode {
    Wrap,
    Truncate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Theme {
    Auto,
    Dark,
    Ansi,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Syntax {
    Auto,
    On,
    Off,
}

impl Args {
    pub fn diff_options(&self) -> diff::Options {
        diff::Options {
            algorithm: match self.algorithm {
                Algorithm::Histogram => diff::Algorithm::Histogram,
                Algorithm::Myers => diff::Algorithm::Myers,
            },
            context: (!self.full).then_some(self.context),
        }
    }

    pub fn render_options(&self, terminal_width: Option<usize>) -> render::Options {
        render::Options {
            theme: theme::Theme::new(match self.theme {
                Theme::Auto => theme::Palette::Auto,
                Theme::Dark => theme::Palette::Dark,
                Theme::Ansi => theme::Palette::Ansi,
                Theme::None => theme::Palette::None,
            }),
            tab_width: self.tab_width,
            wrap: match self.wrap {
                WrapMode::Wrap => Wrap::Wrap,
                WrapMode::Truncate => Wrap::Truncate,
            },
            width: self.width.or(terminal_width),
            min_split_width: self.min_split_width,
            line_numbers: !self.no_line_numbers,
        }
    }

    pub fn view(&self) -> render::View {
        match self.view {
            View::Auto => render::View::Auto,
            View::Split => render::View::Split,
            View::Unified => render::View::Unified,
        }
    }

    /// Whether to syntax-highlight, given the palette that will draw.
    ///
    /// `auto` is not "on if we can parse it": a foreground palette has already
    /// spent the colour channel saying what changed, and layering token colours
    /// on top would make an addition indistinguishable from a removal. Asking
    /// for `on` with such a palette is allowed — it is an explicit choice — but
    /// it is not what `auto` does.
    pub fn syntax_enabled(&self, theme: &theme::Theme) -> bool {
        match self.syntax {
            Syntax::On => true,
            Syntax::Off => false,
            Syntax::Auto => theme.carries_change_in_background(),
        }
    }

    pub fn color_choice(&self) -> anstream::ColorChoice {
        match self.color {
            Color::Auto => anstream::ColorChoice::Auto,
            Color::Always => anstream::ColorChoice::Always,
            Color::Never => anstream::ColorChoice::Never,
        }
    }
}

impl From<Ui> for surface::Request {
    fn from(ui: Ui) -> Self {
        match ui {
            Ui::Auto => surface::Request::Auto,
            Ui::Plain => surface::Request::Plain,
            Ui::Tui => surface::Request::Tui,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from([&["gdiff"], args, &["a.rs", "b.rs"]].concat()).expect("parses")
    }

    #[test]
    fn the_cli_definition_is_valid() {
        // clap's own assertions catch conflicting flags and bad defaults, and
        // they only run here — a broken definition otherwise panics on first
        // use, in front of a user.
        Args::command().debug_assert();
    }

    #[test]
    fn ui_defaults_to_auto_which_is_never_the_tui() {
        let args = parse(&[]);
        assert_eq!(args.ui, Ui::Auto);
        assert_eq!(
            surface::resolve(args.ui.into(), true),
            Ok(surface::Surface::Plain)
        );
    }

    #[test]
    fn context_defaults_to_three_and_full_turns_folding_off() {
        assert_eq!(parse(&[]).diff_options().context, Some(3));
        assert_eq!(parse(&["-U", "7"]).diff_options().context, Some(7));
        assert_eq!(parse(&["--full"]).diff_options().context, None);
    }

    #[test]
    fn an_explicit_width_wins_over_the_detected_one() {
        let args = parse(&["--width", "200"]);
        assert_eq!(args.render_options(Some(80)).width, Some(200));
    }

    #[test]
    fn the_detected_width_is_used_when_none_is_given() {
        assert_eq!(parse(&[]).render_options(Some(80)).width, Some(80));
    }

    #[test]
    fn auto_syntax_follows_what_the_palette_leaves_free() {
        let args = parse(&[]);
        assert!(args.syntax_enabled(&theme::Theme::dark()));
        // A foreground palette has already spent the channel.
        assert!(!args.syntax_enabled(&theme::Theme::ansi()));
        assert!(!args.syntax_enabled(&theme::Theme::none()));
    }

    #[test]
    fn explicit_syntax_overrides_the_palette_in_both_directions() {
        assert!(parse(&["--syntax", "on"]).syntax_enabled(&theme::Theme::ansi()));
        assert!(!parse(&["--syntax", "off"]).syntax_enabled(&theme::Theme::dark()));
    }
}
