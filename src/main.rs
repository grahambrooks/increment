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
use gdiff::tui;
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
    // The review flow is interactive by definition — there is no non-terminal
    // rendering of "work through a branch". It is handled before everything
    // else because it does not read a source up front; it loads each commit as
    // the reader reaches it.
    if let Source::Review { rev, paths, limit } = args.source()? {
        return review(args, rev, paths, limit);
    }

    // Resolved before anything is read, so `--ui tui > file` fails immediately
    // rather than after the work.
    let surface = surface::resolve(args.ui.into(), std::io::stdout().is_terminal())
        .map_err(|error| error.to_string())?;

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

    let changed = if surface == surface::Surface::Tui {
        browse(args, &documents, &changes)?
    } else {
        let mut out = anstream::AutoStream::new(std::io::stdout().lock(), args.color_choice());
        write(args, &documents, &changes, &mut out)?
    };

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
        // Handled before any source is read — see `run`.
        Source::Review { .. } => unreachable!("the review flow does not read a source up front"),
        Source::Patch => {
            let stdin = std::io::stdin();
            source::patch::parse(stdin.lock(), diff_options)
                .map_err(|error| format!("reading the patch: {error}"))
        }
    }
}

/// Work through a branch, commit by commit.
fn review(
    args: &Args,
    rev: Option<&str>,
    paths: &[std::path::PathBuf],
    limit: usize,
) -> Result<i32, String> {
    if !std::io::stdout().is_terminal() {
        return Err(surface::NotATerminal.to_string());
    }

    // The working tree is the top row when no range was named — reviewing "this
    // branch" almost always means reviewing what is not committed yet as well.
    let mut items: Vec<tui::Item> = Vec::new();
    if rev.is_none() || rev.is_some_and(|rev| !rev.contains("..")) {
        items.push(tui::Item::Worktree);
    }

    // The history is walked on another thread and appended as it arrives, so
    // the first screen of a fifty-thousand-commit branch shows immediately
    // rather than after the walk.
    let (sender, incoming) = std::sync::mpsc::channel();
    let spec = rev.map(str::to_owned);
    std::thread::spawn(move || {
        let sent = source::git::log_each(spec.as_deref(), Some(limit), |batch| {
            let batch: Vec<tui::Item> = batch.into_iter().map(tui::Item::from).collect();
            // The reader quit: stop walking rather than filling a channel
            // nobody is reading.
            match sender.send(Ok(batch)) {
                Ok(()) => std::ops::ControlFlow::Continue(()),
                Err(_) => std::ops::ControlFlow::Break(()),
            }
        });
        if let Err(error) = sent {
            let _ = sender.send(Err(error.to_string()));
        }
    });

    let options = args.render_options(terminal_width());
    let diff_options = args.diff_options();
    let syntax = args.syntax_enabled(&options.theme);
    let paths = paths.to_vec();

    // Each row is diffed when the reader reaches it, not up front: a branch of
    // two hundred commits would otherwise mean two hundred diffs before the
    // first frame.
    let loader: tui::Loader<'_> = Box::new(move |item| {
        let changes = match item {
            tui::Item::Worktree => source::git::compare(None, &paths),
            tui::Item::Commit(commit) => source::git::commit(&commit.id, &paths),
        }
        .map_err(|error| error.to_string())?;

        Ok(changes
            .comparisons
            .iter()
            .map(|pair| {
                let document = diff::compare(&pair.old, &pair.new, &diff_options);
                let unfolded = diff::compare(
                    &pair.old,
                    &pair.new,
                    &diff::Options {
                        context: None,
                        ..diff_options
                    },
                );
                let (old, new) = (pair.old.clone(), pair.new.clone());
                tui::Entry::lazy(document, Some(unfolded), move || {
                    if syntax {
                        Highlighting::of(&old, &new)
                    } else {
                        Highlighting::none()
                    }
                })
            })
            .collect())
    });

    tui::review(items, Some(incoming), loader, options).map_err(|error| error.to_string())?;
    Ok(exit::SUCCESS)
}

/// Hand the documents to the interactive browser.
///
/// Binary changes are reported after it exits rather than inside it: the
/// browser has nothing to show for them, and swallowing them would be the same
/// lie by omission as dropping them from the stream.
fn browse(
    args: &Args,
    documents: &[(DiffDocument, Option<&Comparison>)],
    changes: &Changes,
) -> Result<bool, String> {
    let options = args.render_options(terminal_width());
    let entries: Vec<tui::Entry> = documents
        .iter()
        .filter(|(document, _)| document.has_changes())
        .map(|(document, pair)| {
            // Unfolding needs the sources. A patch never had them, so the
            // browser says so rather than offering a key that does nothing.
            let unfolded = pair.map(|pair| {
                diff::compare(
                    &pair.old,
                    &pair.new,
                    &diff::Options {
                        context: None,
                        ..args.diff_options()
                    },
                )
            });
            let syntax = args.syntax_enabled(&options.theme);
            let sources = pair.map(|pair| (pair.old.clone(), pair.new.clone()));
            tui::Entry::lazy(document.clone(), unfolded, move || match &sources {
                Some((old, new)) if syntax => Highlighting::of(old, new),
                _ => Highlighting::none(),
            })
        })
        .collect();

    let changed = !entries.is_empty() || !changes.binary.is_empty();
    tui::run(entries, options).map_err(|error| error.to_string())?;

    for name in &changes.binary {
        println!("Binary file {name} differs");
    }

    Ok(changed)
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
