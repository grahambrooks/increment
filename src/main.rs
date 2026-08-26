//! The `gdiff` binary: a thin shell over the library.
//!
//! It reads the two sides, asks the library for a document, hands it to a
//! renderer and maps the outcome to an exit code. Every decision it makes —
//! which surface, which view, which palette — is a library function it calls,
//! not logic it owns.

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use gdiff::cli::{args::Args, surface};
use gdiff::highlight::Highlighting;
use gdiff::model::SourceFile;
use gdiff::{diff, exit, render};

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
    // Resolved before anything is read, so `--ui tui > file` fails immediately
    // rather than after the work.
    let surface = surface::resolve(args.ui.into(), std::io::stdout().is_terminal())
        .map_err(|error| error.to_string())?;

    if surface == surface::Surface::Tui {
        return Err("the interactive browser is not implemented yet (phase 4). \
             Drop `--ui tui` for the side-by-side view."
            .to_owned());
    }

    let old = read(&args.old)?;
    let new = read(&args.new)?;
    let document = diff::compare(&old, &new, &args.diff_options());

    let mut out = anstream::AutoStream::new(std::io::stdout().lock(), args.color_choice());

    match match args.format {
        gdiff::cli::args::Format::Json => render::json::render(&document, &mut out),
        gdiff::cli::args::Format::Text => {
            let options = args.render_options(terminal_width());
            // Computed from the sources, which the document deliberately does
            // not carry — a syntax parser needs the lines folding hides.
            let highlighting = if args.syntax_enabled(&options.theme) {
                Highlighting::of(&old, &new)
            } else {
                Highlighting::none()
            };
            render::render(&document, &highlighting, args.view(), &options, &mut out)
        }
    }
    .and_then(|()| out.flush())
    {
        Ok(()) => {}
        // Someone closed the pipe — `gdiff a b | head`. The reader got what it
        // asked for, so this is a normal end, not a failure. Reporting it would
        // print an error after every `| head`.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(error) => return Err(format!("writing output: {error}")),
    }

    Ok(if document.has_changes() {
        exit::DIFFERENCES
    } else {
        exit::SUCCESS
    })
}

fn read(path: &std::path::Path) -> Result<SourceFile, String> {
    SourceFile::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The terminal's width, or `None` when output is not going to one.
///
/// `None` is meaningful rather than a missing value: it is what tells the
/// renderer not to split, because padding two panes to a guessed width would
/// fill a pipe with trailing spaces nobody asked for.
fn terminal_width() -> Option<usize> {
    terminal_size::terminal_size().map(|(terminal_size::Width(columns), _)| columns as usize)
}
