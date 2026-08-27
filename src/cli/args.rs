//! Argument definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The left-hand side.
    pub old: Option<PathBuf>,

    /// The right-hand side.
    pub new: Option<PathBuf>,

    /// Read a unified diff from standard input instead of comparing files.
    ///
    /// For use as a git pager. Lower fidelity than `gdiff git`: a patch only
    /// carries the context git chose to print.
    #[arg(long, short = 'p', conflicts_with_all = ["old", "new"])]
    pub patch: bool,

    /// Output surface. `auto` never selects `tui` — see the design.
    #[arg(long, value_enum, default_value_t = Ui::Auto, global = true)]
    pub ui: Ui,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text, global = true)]
    pub format: Format,

    /// Which view to draw. `auto` splits where the terminal is wide enough.
    #[arg(long, value_enum, default_value_t = View::Auto, global = true)]
    pub view: View,

    /// Edit script to compute.
    #[arg(long, value_enum, default_value_t = Algorithm::Histogram, global = true)]
    pub algorithm: Algorithm,

    /// Unchanged lines to keep either side of a change.
    #[arg(
        short = 'U',
        long,
        default_value_t = 3,
        value_name = "LINES",
        global = true
    )]
    pub context: usize,

    // Named after what it is for rather than after what it switches off:
    // `--full` describes the mechanism, and someone looking for "show me the
    // whole file" does not find it under a word about folding. The old spelling
    // stays as an alias — renaming a flag out from under a script is not worth
    // the tidiness.
    /// Show the whole file, not only the parts that changed.
    #[arg(
        long = "whole-file",
        visible_alias = "full",
        conflicts_with = "context",
        global = true
    )]
    pub whole_file: bool,

    /// What to do with a line too wide for its pane.
    #[arg(long, value_enum, default_value_t = WrapMode::Wrap, global = true)]
    pub wrap: WrapMode,

    /// Columns a tab advances to.
    #[arg(long, default_value_t = 4, value_name = "COLUMNS", global = true)]
    pub tab_width: usize,

    /// Colour palette.
    #[arg(long, value_enum, default_value_t = Theme::Auto, global = true)]
    pub theme: Theme,

    /// When to emit colour.
    #[arg(long, value_enum, default_value_t = Color::Auto, global = true)]
    pub color: Color,

    /// Override the detected terminal width.
    #[arg(long, value_name = "COLUMNS", global = true)]
    pub width: Option<usize>,

    /// Narrower than this, the split view gives way to the unified one.
    #[arg(long, default_value_t = 120, value_name = "COLUMNS", global = true)]
    pub min_split_width: usize,

    /// Hide the line numbers in the centre gutter.
    #[arg(long, global = true)]
    pub no_line_numbers: bool,

    /// Syntax highlighting. `auto` enables it only where the palette leaves
    /// the foreground free.
    #[arg(long, value_enum, default_value_t = Syntax::Auto, global = true)]
    pub syntax: Syntax,

    /// Ignore whitespace entirely when deciding what changed.
    #[arg(
        short = 'w',
        long,
        global = true,
        conflicts_with = "ignore_space_change"
    )]
    pub ignore_all_space: bool,

    /// Ignore changes in the amount of whitespace.
    #[arg(short = 'b', long, global = true)]
    pub ignore_space_change: bool,

    /// Report a block that moved as a deletion and an addition.
    #[arg(long, global = true)]
    pub no_moved: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Compare against git: a revision, a range, or the working tree.
    ///
    /// With no revision, compares `HEAD` against the working tree. With one,
    /// that revision against the working tree. With `a..b`, one revision
    /// against the other.
    Git {
        /// `HEAD`, `main`, `HEAD~2..HEAD`, …
        rev: Option<String>,

        /// Limit to these paths.
        #[arg(last = false)]
        paths: Vec<PathBuf>,
    },

    /// Work through a branch commit by commit.
    ///
    /// Opens a commit list; `Enter` shows what that commit did, and moving the
    /// selection with the split open follows it. Always interactive.
    Review {
        /// `HEAD`, `main..feature`, … Defaults to the current branch.
        rev: Option<String>,

        /// Limit to these paths.
        #[arg(last = false)]
        paths: Vec<PathBuf>,

        /// Most commits to list.
        #[arg(long, default_value_t = 200, value_name = "COUNT")]
        limit: usize,
    },
}

/// What the arguments add up to.
#[derive(Debug)]
pub enum Source<'a> {
    Files {
        old: &'a PathBuf,
        new: &'a PathBuf,
    },
    Git {
        rev: Option<&'a str>,
        paths: &'a [PathBuf],
    },
    Review {
        rev: Option<&'a str>,
        paths: &'a [PathBuf],
        limit: usize,
    },
    Patch,
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
    /// Work out what was asked for.
    ///
    /// `Err` carries the message for the one case clap cannot express: the bare
    /// form needs *both* paths, and clap will happily accept one.
    pub fn source(&self) -> Result<Source<'_>, &'static str> {
        match (&self.command, self.patch, &self.old, &self.new) {
            (Some(Command::Git { rev, paths }), ..) => Ok(Source::Git {
                rev: rev.as_deref(),
                paths,
            }),
            (Some(Command::Review { rev, paths, limit }), ..) => Ok(Source::Review {
                rev: rev.as_deref(),
                paths,
                limit: *limit,
            }),
            (None, true, ..) => Ok(Source::Patch),
            (None, false, Some(old), Some(new)) => Ok(Source::Files { old, new }),
            (None, false, Some(_), None) => Err("expected two files to compare, got one"),
            (None, false, None, _) => {
                Err("nothing to compare: give two files, `gdiff git`, or `--patch`")
            }
        }
    }

    pub fn diff_options(&self) -> diff::Options {
        diff::Options {
            algorithm: match self.algorithm {
                Algorithm::Histogram => diff::Algorithm::Histogram,
                Algorithm::Myers => diff::Algorithm::Myers,
            },
            context: (!self.whole_file).then_some(self.context),
            whitespace: if self.ignore_all_space {
                diff::Whitespace::IgnoreAll
            } else if self.ignore_space_change {
                diff::Whitespace::IgnoreChange
            } else {
                diff::Whitespace::Respect
            },
            detect_moves: !self.no_moved,
        }
    }

    pub fn render_options(&self, terminal_width: Option<usize>) -> render::Options {
        // `auto` is resolved here rather than carried, so that cycling themes at
        // runtime starts from something concrete.
        let palette = match self.theme {
            Theme::Auto => theme::Palette::resolve_auto(),
            Theme::Dark => theme::Palette::Dark,
            Theme::Ansi => theme::Palette::Ansi,
            Theme::None => theme::Palette::None,
        };

        let mut options = render::Options {
            syntax: self.syntax_enabled_for(palette),
            tab_width: self.tab_width,
            wrap: match self.wrap {
                WrapMode::Wrap => Wrap::Wrap,
                WrapMode::Truncate => Wrap::Truncate,
            },
            width: self.width.or(terminal_width),
            min_split_width: self.min_split_width,
            line_numbers: !self.no_line_numbers,
            ..render::Options::default()
        };
        options.set_palette(palette);
        options
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

    fn syntax_enabled_for(&self, palette: theme::Palette) -> bool {
        self.syntax_enabled(&theme::Theme::new(palette))
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

    fn parse_bare(args: &[&str]) -> Args {
        Args::try_parse_from([&["gdiff"], args].concat()).expect("parses")
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
    fn context_defaults_to_three_and_the_whole_file_turns_folding_off() {
        assert_eq!(parse(&[]).diff_options().context, Some(3));
        assert_eq!(parse(&["-U", "7"]).diff_options().context, Some(7));
        assert_eq!(parse(&["--whole-file"]).diff_options().context, None);
    }

    #[test]
    fn full_still_works_as_a_name_for_the_whole_file() {
        // Renaming a flag someone has already put in a script is not worth the
        // tidiness; the old spelling stays, and both appear in `--help`.
        assert_eq!(parse(&["--full"]).diff_options().context, None);
    }

    #[test]
    fn the_whitespace_flags_map_to_the_three_modes() {
        assert_eq!(
            parse(&[]).diff_options().whitespace,
            diff::Whitespace::Respect
        );
        assert_eq!(
            parse(&["-w"]).diff_options().whitespace,
            diff::Whitespace::IgnoreAll
        );
        assert_eq!(
            parse(&["-b"]).diff_options().whitespace,
            diff::Whitespace::IgnoreChange
        );
        // Asking for both at once is a contradiction, not a precedence puzzle.
        assert!(Args::try_parse_from(["gdiff", "-w", "-b", "a", "b"]).is_err());
    }

    #[test]
    fn move_detection_is_on_unless_turned_off() {
        assert!(parse(&[]).diff_options().detect_moves);
        assert!(!parse(&["--no-moved"]).diff_options().detect_moves);
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
    fn two_paths_are_a_file_comparison() {
        let args = parse_bare(&["old.rs", "new.rs"]);
        assert!(matches!(args.source(), Ok(Source::Files { .. })));
    }

    #[test]
    fn one_path_is_an_error_rather_than_a_diff_against_nothing() {
        let args = parse_bare(&["only.rs"]);
        assert!(args.source().is_err(), "{:?}", args.source());
    }

    #[test]
    fn no_arguments_at_all_says_what_the_options_are() {
        let args = parse_bare(&[]);
        let message = args.source().expect_err("should not resolve");
        assert!(message.contains("git"), "{message}");
        assert!(message.contains("--patch"), "{message}");
    }

    #[test]
    fn the_git_subcommand_carries_its_revision_and_paths() {
        let args = parse_bare(&["git", "HEAD~1..HEAD", "src", "tests"]);
        match args.source().expect("resolves") {
            Source::Git { rev, paths } => {
                assert_eq!(rev, Some("HEAD~1..HEAD"));
                assert_eq!(paths.len(), 2);
            }
            other => panic!("expected a git source, got {other:?}"),
        }
    }

    #[test]
    fn options_may_come_before_the_subcommand() {
        // `gdiff --ui tui git` must not be read as the two-file form with `git`
        // as the first path.
        for args in [
            vec!["git", "--view", "unified"],
            vec!["--view", "unified", "git"],
            vec!["--ui", "tui", "git", "HEAD~1..HEAD"],
        ] {
            let parsed = Args::try_parse_from([vec!["gdiff"], args.clone()].concat())
                .unwrap_or_else(|error| panic!("{args:?} should parse: {error}"));
            assert!(
                matches!(parsed.source(), Ok(Source::Git { .. })),
                "{args:?} resolved to {:?}",
                parsed.source()
            );
        }
    }

    #[test]
    fn review_carries_its_revision_paths_and_limit() {
        let args = parse_bare(&["review", "main..feature", "src", "--limit", "50"]);
        match args.source().expect("resolves") {
            Source::Review { rev, paths, limit } => {
                assert_eq!(rev, Some("main..feature"));
                assert_eq!(paths.len(), 1);
                assert_eq!(limit, 50);
            }
            other => panic!("expected a review source, got {other:?}"),
        }
    }

    #[test]
    fn git_with_no_revision_is_still_a_git_source() {
        let args = parse_bare(&["git"]);
        assert!(matches!(args.source(), Ok(Source::Git { rev: None, .. })));
    }

    #[test]
    fn patch_reads_from_stdin_and_takes_no_paths() {
        assert!(matches!(
            parse_bare(&["--patch"]).source(),
            Ok(Source::Patch)
        ));
        // Giving it files as well is a contradiction, and clap should say so.
        assert!(Args::try_parse_from(["gdiff", "--patch", "a.rs", "b.rs"]).is_err());
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
