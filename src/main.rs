//! The `gdiff` binary: a thin shell over the library.
//!
//! It parses arguments, resolves the output surface, and maps the outcome to an
//! exit code. Everything else lives in the library, where integration tests can
//! reach it.

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;
use gdiff::cli::{args::Args, surface};
use gdiff::exit;

fn main() -> ExitCode {
    let args = Args::parse();

    match run(&args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(message) => {
            eprintln!("gdiff: {message}");
            ExitCode::from(exit::TROUBLE as u8)
        }
    }
}

fn run(args: &Args) -> Result<i32, String> {
    // Enforced before anything is read, so `--ui tui > file` fails immediately
    // rather than after the work.
    let surface = surface::resolve(args.ui.into(), std::io::stdout().is_terminal())
        .map_err(|error| error.to_string())?;

    // Phase 0 is the scaffold: the gate is green, the layering is in place and
    // the surface rule is enforced, but nothing is diffed yet. Saying so and
    // exiting `TROUBLE` is the honest answer — printing an empty diff would
    // claim two files are identical without having looked at them.
    Err(format!(
        "diffing is not implemented yet (phase 1). \
         Would render {old} against {new} on the {surface:?} surface as {format:?}. \
         See design/002-architecture-and-plan.md.",
        old = args.old.display(),
        new = args.new.display(),
        format = args.format,
    ))
}
