//! The `gdiff` binary: a thin shell over the library.
//!
//! It resolves what was asked for, asks a source for the files, asks the
//! library for a document per file, hands each to a renderer, and maps the
//! outcome to an exit code. Every decision it makes — which surface, which
//! view, which palette — is a library function it calls, not logic it owns.

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use gdiff::cli::args::{Args, Format, Source};
use gdiff::cli::surface;
use gdiff::highlight::Highlighting;
use gdiff::model::DiffDocument;
use gdiff::source::{self, Changes, Comparison};
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

    let diff_options = args.diff_options();
    let changes = read(args, &diff_options)?;

    // Sources that hold both files produce documents here; the patch source
    // parses documents directly, because it never had the files.
    let documents: Vec<(DiffDocument, Option<&Comparison>)> = changes
        .comparisons
        .iter()
        .map(|pair| {
            (
                diff::compare(&pair.old, &pair.new, &diff_options),
                Some(pair),
            )
        })
        .chain(changes.documents.iter().map(|d| (d.clone(), None)))
        .collect();

    let mut out = anstream::AutoStream::new(std::io::stdout().lock(), args.color_choice());
    let changed = write(args, &documents, &changes, &mut out)?;

    Ok(if changed {
        exit::DIFFERENCES
    } else {
        exit::SUCCESS
    })
}

fn read(args: &Args, diff_options: &diff::Options) -> Result<Changes, String> {
    match args.source()? {
        Source::Files { old, new } => {
            source::files::compare(old, new).map_err(|error| format!("{}: {error}", old.display()))
        }
        Source::Git { rev, paths } => {
            source::git::compare(rev, paths).map_err(|error| error.to_string())
        }
        Source::Patch => {
            let stdin = std::io::stdin();
            source::patch::parse(stdin.lock(), diff_options)
                .map_err(|error| format!("reading the patch: {error}"))
        }
    }
}

/// Draw every document, and report whether anything differed.
fn write(
    args: &Args,
    documents: &[(DiffDocument, Option<&Comparison>)],
    changes: &Changes,
    out: &mut impl Write,
) -> Result<bool, String> {
    let mut changed = !changes.binary.is_empty();

    let result = (|| -> std::io::Result<()> {
        for (index, (document, pair)) in documents.iter().enumerate() {
            if !document.has_changes() {
                continue;
            }
            changed = true;

            match args.format {
                Format::Json => render::json::render(document, out)?,
                Format::Text => {
                    // A blank line between files, but not before the first: a
                    // leading blank line in a pipe is noise.
                    if index > 0 {
                        writeln!(out)?;
                    }
                    let options = args.render_options(terminal_width());
                    // Highlighting needs the sources, which a document does not
                    // carry — and a patch never had them.
                    let highlighting = match pair {
                        Some(pair) if args.syntax_enabled(&options.theme) => {
                            Highlighting::of(&pair.old, &pair.new)
                        }
                        _ => Highlighting::none(),
                    };
                    render::render(document, &highlighting, args.view(), &options, out)?;
                }
            }
        }

        // Named, not silently dropped: "something changed here that I am not
        // showing you" is information the reader needs.
        for name in &changes.binary {
            writeln!(out, "Binary file {name} differs")?;
        }

        out.flush()
    })();

    match result {
        Ok(()) => Ok(changed),
        // Someone closed the pipe — `gdiff git | head`. The reader got what it
        // asked for, so this is a normal end, not a failure.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(changed),
        Err(error) => Err(format!("writing output: {error}")),
    }
}

/// The terminal's width, or `None` when output is not going to one.
///
/// `None` is meaningful rather than a missing value: it is what tells the
/// renderer not to split, because padding two panes to a guessed width would
/// fill a pipe with trailing spaces nobody asked for.
fn terminal_width() -> Option<usize> {
    terminal_size::terminal_size().map(|(terminal_size::Width(columns), _)| columns as usize)
}
