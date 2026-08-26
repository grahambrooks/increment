//! The browser's layout, drawn into a buffer instead of a terminal.
//!
//! `TestBackend` gives a real render with no terminal involved, so these run in
//! CI like any other test. They pin what the reader sees; the navigation that
//! decides *what* to show is unit-tested in `tui::state`, without even a buffer.
//!
//! Snapshots are text only. Colour is asserted separately, on the few cells
//! where it carries meaning — putting styles in a snapshot would make every
//! palette tweak look like a layout regression.

use gdiff::diff::{Options as DiffOptions, compare};
use gdiff::highlight::Highlighting;
use gdiff::model::SourceFile;
use gdiff::render::Options as RenderOptions;
use gdiff::theme::Theme;
use gdiff::tui::state::{Action, App, Entry};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn entry(name: &str, old: &str, new: &str) -> Entry {
    let old = SourceFile::from_text(format!("a/{name}"), old);
    let new = SourceFile::from_text(format!("b/{name}"), new);
    Entry {
        document: compare(&old, &new, &DiffOptions::default()),
        unfolded: Some(compare(
            &old,
            &new,
            &DiffOptions {
                context: None,
                ..DiffOptions::default()
            },
        )),
        highlighting: Highlighting::none(),
    }
}

/// Forty lines with edits at 5 and 35 — long enough to fold, scroll and map.
fn long(name: &str) -> Entry {
    let old: String = (1..=40).map(|n| format!("let line{n} = {n};\n")).collect();
    let new: String = (1..=40)
        .map(|n| {
            if n == 5 || n == 35 {
                format!("let line{n} = {n}00;\n")
            } else {
                format!("let line{n} = {n};\n")
            }
        })
        .collect();
    entry(name, &old, &new)
}

fn app(entries: Vec<Entry>) -> App {
    App::new(
        entries,
        RenderOptions {
            theme: Theme::none(),
            ..RenderOptions::default()
        },
    )
}

/// Draw, and return the buffer as plain text.
fn draw(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| gdiff::tui::draw::draw(frame, app))
        .expect("draws");

    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn one_file_hides_the_file_list() {
    // A sidebar with a single row in it is a waste of thirty columns.
    let mut app = app(vec![long("only.rs")]);
    insta::assert_snapshot!(draw(&mut app, 100, 20));
}

#[test]
fn several_files_show_the_list() {
    let mut app = app(vec![
        long("src/main.rs"),
        entry("src/lib.rs", "pub fn a() {}\n", "pub fn b() {}\n"),
    ]);
    insta::assert_snapshot!(draw(&mut app, 110, 20));
}

#[test]
fn scrolled_to_a_change() {
    let mut app = app(vec![long("only.rs")]);
    app.apply(Action::NextChange);
    insta::assert_snapshot!(draw(&mut app, 100, 16));
}

#[test]
fn typing_a_search_shows_the_query_in_the_status_line() {
    let mut app = app(vec![long("only.rs")]);
    // Draw once so the state machine knows the viewport.
    let _ = draw(&mut app, 100, 16);
    app.apply(Action::SearchStart);
    for c in "line35".chars() {
        app.apply(Action::SearchType(c));
    }
    insta::assert_snapshot!(draw(&mut app, 100, 16));
}

#[test]
fn a_committed_search_reports_which_match_it_is_on() {
    let mut app = app(vec![long("only.rs")]);
    let _ = draw(&mut app, 100, 16);
    app.apply(Action::SearchStart);
    for c in "let line".chars() {
        app.apply(Action::SearchType(c));
    }
    app.apply(Action::SearchCommit);

    let drawn = draw(&mut app, 100, 16);
    assert!(drawn.contains("match 1/"), "{drawn}");
}

#[test]
fn unfolding_reveals_the_hidden_lines() {
    let mut app = app(vec![long("only.rs")]);
    let _ = draw(&mut app, 100, 16);
    let folded = draw(&mut app, 100, 16);
    assert!(folded.contains("unchanged lines"), "{folded}");

    app.apply(Action::ToggleFold);
    let unfolded = draw(&mut app, 100, 16);
    assert!(
        !unfolded.contains("unchanged lines"),
        "the fold should be gone:\n{unfolded}"
    );
}

#[test]
fn a_narrow_terminal_still_draws_without_panicking() {
    // Ratatui panics if a layout asks for more room than exists, and a reader
    // with a small window is not an error case.
    for (width, height) in [(40, 6), (20, 4), (12, 3)] {
        let mut app = app(vec![long("only.rs"), long("other.rs")]);
        let drawn = draw(&mut app, width, height);
        assert!(!drawn.is_empty(), "{width}x{height} drew nothing");
    }
}

#[test]
fn a_file_whose_changes_are_all_edits_is_not_listed_as_unchanged() {
    // `+0 -0` next to a filename reads as "nothing happened here". A file with
    // only modified lines is exactly the case that gets it wrong.
    let mut app = app(vec![
        entry("edited.rs", "let x = 1;\n", "let x = 2;\n"),
        entry("other.rs", "a\n", "a\nb\n"),
    ]);
    let drawn = draw(&mut app, 110, 10);
    assert!(drawn.contains("~1"), "the edit count is missing:\n{drawn}");
}

#[test]
fn an_empty_browser_says_there_are_no_changes() {
    let mut app = app(Vec::new());
    let drawn = draw(&mut app, 60, 8);
    assert!(drawn.contains("no changes"), "{drawn}");
}

#[test]
fn the_change_map_marks_where_the_reader_is() {
    // The map is the whole-file overview the stdout renderer deliberately does
    // not have. It is only worth its column if it shows both what changed and
    // where the viewport sits.
    let mut app = app(vec![long("only.rs")]);
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
    terminal
        .draw(|frame| gdiff::tui::draw::draw(frame, &mut app))
        .expect("draws");

    let buffer = terminal.backend().buffer().clone();
    let map_x = buffer.area.width - 1;
    let column: Vec<String> = (0..buffer.area.height - 1)
        .map(|y| buffer[(map_x, y)].symbol().to_owned())
        .collect();

    assert!(
        column.iter().any(|glyph| glyph == "▐"),
        "expected change marks in the map, got {column:?}"
    );

    // The file is longer than the screen here, so the indicator must appear.
    let reversed = (0..buffer.area.height - 1)
        .filter(|&y| {
            buffer[(map_x, y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
        .count();
    assert!(reversed > 0, "the viewport indicator is missing");
}
