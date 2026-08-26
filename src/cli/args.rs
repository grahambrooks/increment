//! Argument definitions.
//!
//! Only what phase 0 can honestly honour: the two inputs, and the choices
//! (surface, format, algorithm) that are already decided in the design even
//! though the renderers behind them arrive in phases 1 and 2. Flags are not
//! added ahead of the code that implements them — these three exist because the
//! surface rule in [`super::surface`] is enforced from day one.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

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

    #[test]
    fn the_cli_definition_is_valid() {
        // clap's own assertions catch conflicting flags and bad defaults, and
        // they only run here — a broken definition otherwise panics on first
        // use, in front of a user.
        Args::command().debug_assert();
    }

    #[test]
    fn ui_defaults_to_auto_which_is_never_the_tui() {
        let args = Args::try_parse_from(["gdiff", "a.rs", "b.rs"]).expect("parses");
        assert_eq!(args.ui, Ui::Auto);
        assert_eq!(
            surface::resolve(args.ui.into(), true),
            Ok(surface::Surface::Plain)
        );
    }
}
