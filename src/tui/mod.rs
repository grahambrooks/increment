//! The interactive browser.
//!
//! Always an explicit request — `--ui tui`, never `auto`. An alternate screen
//! cannot be piped, redirected or read by CI, and `auto` is what CI hits.
//!
//! Two halves, deliberately: [`state`] is navigation with no terminal in it,
//! and [`draw`] turns that state into cells and does nothing else. The rows it
//! draws come from the same `render::split::compose` the stdout renderer uses,
//! so the two surfaces cannot disagree about what a diff looks like.

pub mod draw;
pub mod keys;
pub mod review;
pub mod state;

use std::io::IsTerminal;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event};

use crate::render::Options;

pub use review::{Item, Loader, Review};
pub use state::{Action, App, Entry};

/// Run the browser until the reader quits.
pub fn run(entries: Vec<Entry>, options: Options) -> std::io::Result<()> {
    // Belt and braces: the CLI has already refused a redirected TUI, but this
    // is the function that would write escape codes into a file, so it checks
    // for itself rather than trusting its caller.
    if !std::io::stdout().is_terminal() {
        return Err(std::io::Error::other(
            "the interactive browser needs a terminal",
        ));
    }

    let mut app = App::new(entries, options);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

/// Rows still arriving from a background walk of the history.
///
/// `Err` ends the stream with a message; `Ok` is a batch to append.
pub type Incoming = Receiver<Result<Vec<Item>, String>>;

/// Run the review flow — a commit list, with each commit's diff below it.
///
/// `incoming` carries rows found after the first frame, so a long history shows
/// its first screen immediately instead of after the walk finishes. Pass `None`
/// when everything is already in `items`.
pub fn review(
    items: Vec<Item>,
    incoming: Option<Incoming>,
    loader: Loader<'_>,
    options: Options,
) -> std::io::Result<()> {
    if !std::io::stdout().is_terminal() {
        return Err(std::io::Error::other(
            "the interactive browser needs a terminal",
        ));
    }

    let mut review = Review::new(items, loader, options);
    review.set_loading(incoming.is_some());

    let mut terminal = ratatui::init();
    let result = review_loop(&mut terminal, &mut review, incoming.as_ref());
    ratatui::restore();
    result
}

/// How long to wait for a key before looking at the world again.
///
/// Only used while there is something to look at — rows arriving, or a deferred
/// diff to load. Otherwise the loop blocks, so an idle browser costs nothing.
const TICK: Duration = Duration::from_millis(30);

fn review_loop(
    terminal: &mut ratatui::DefaultTerminal,
    review: &mut Review<'_>,
    incoming: Option<&Incoming>,
) -> std::io::Result<()> {
    while !review.should_quit() {
        terminal.draw(|frame| draw::review(frame, review))?;

        // Deferred work, done only when the reader has stopped moving: the
        // diff of the commit they landed on, and then its colours. Holding `j`
        // down the log stays instant, and a commit draws before it is coloured.
        if !event::poll(Duration::ZERO)? {
            if review.needs_settle() {
                review.settle();
                continue;
            }
            if review.diff().is_some_and(App::needs_colour) {
                if let Some(diff) = review.diff_mut() {
                    diff.colour_now();
                }
                continue;
            }
        }

        let busy = review.loading()
            || review.needs_settle()
            || review.diff().is_some_and(App::needs_colour);
        if busy && !event::poll(TICK)? {
            drain(review, incoming);
            continue;
        }

        if let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
        {
            let search = review
                .diff()
                .map_or(state::Search::Off, |diff| diff.search().clone());
            if let Some(event) = keys::review_action(key, review.focus(), &search) {
                review.apply(event);
            }
        }
        drain(review, incoming);
    }
    Ok(())
}

/// Take whatever the background walk has produced since the last look.
fn drain(review: &mut Review<'_>, incoming: Option<&Incoming>) {
    let Some(incoming) = incoming else {
        return;
    };
    loop {
        match incoming.try_recv() {
            Ok(Ok(batch)) => review.extend(batch),
            Ok(Err(message)) => {
                review.report(message);
                review.set_loading(false);
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            // The walk finished and dropped its end.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                review.set_loading(false);
                return;
            }
        }
    }
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| draw::draw(frame, app))?;

        // Colour arrives on the frame after the diff does. Highlighting one
        // file can cost more than everything else about opening it, and a
        // plain frame now beats a coloured one in a second.
        if app.needs_colour() && !event::poll(Duration::ZERO)? {
            app.colour_now();
            continue;
        }

        // Only key *presses*: a terminal that reports releases and repeats
        // would otherwise move three rows for one keystroke.
        if let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
            && let Some(action) = keys::action(key, app.search())
        {
            app.apply(action);
        }
    }
    Ok(())
}
