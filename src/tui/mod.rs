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
pub mod state;

use std::io::IsTerminal;

use ratatui::crossterm::event::{self, Event};

use crate::render::Options;

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

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| draw::draw(frame, app))?;

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
