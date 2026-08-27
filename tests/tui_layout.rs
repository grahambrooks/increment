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
    Entry::new(
        compare(&old, &new, &DiffOptions::default()),
        Some(compare(
            &old,
            &new,
            &DiffOptions {
                context: None,
                ..DiffOptions::default()
            },
        )),
        Highlighting::none(),
    )
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

// ---------------------------------------------------------------------------
// The review flow: a commit list, and the selected commit's diff below it.
// ---------------------------------------------------------------------------

use gdiff::source::git::Commit;
use gdiff::tui::review::{Event, Item, Loader, Review};

fn commit(n: usize, summary: &str) -> Commit {
    Commit {
        short_id: format!("{n:07x}"),
        id: format!("{n:040x}"),
        summary: summary.to_owned(),
        author: "Graham Brooks".to_owned(),
        date: "2026-08-26".to_owned(),
    }
}

fn review() -> Review<'static> {
    let commits: Vec<Item> = vec![
        commit(0x3f2a1c9, "Phase 5: move detection and whitespace modes"),
        commit(0x10f4df2, "Phase 4: the interactive browser"),
        commit(0x5ab2f81, "Phase 3: git integration"),
        commit(0x47763dd, "Phases 1 and 2: the aligned split view"),
        commit(0x039c984, "Phase 0: scaffold"),
    ]
    .into_iter()
    .map(Item::from)
    .collect();
    let loader: Loader<'static> = Box::new(|_| Ok(vec![long("src/diff/moves.rs")]));
    Review::new(
        commits,
        loader,
        RenderOptions {
            theme: Theme::none(),
            ..RenderOptions::default()
        },
    )
}

fn draw_review(review: &mut Review<'_>, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| gdiff::tui::draw::review(frame, review))
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
fn the_commit_list_before_anything_is_opened() {
    insta::assert_snapshot!(draw_review(&mut review(), 90, 12));
}

#[test]
fn opening_a_commit_splits_the_view() {
    let mut review = review();
    let _ = draw_review(&mut review, 100, 24);
    review.apply(Event::Down);
    review.apply(Event::Open);
    insta::assert_snapshot!(draw_review(&mut review, 100, 24));
}

#[test]
fn the_selected_commit_is_marked_in_the_list() {
    let mut review = review();
    let _ = draw_review(&mut review, 90, 12);
    review.apply(Event::Down);
    review.apply(Event::Down);

    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("terminal");
    terminal
        .draw(|frame| gdiff::tui::draw::review(frame, &mut review))
        .expect("draws");
    let buffer = terminal.backend().buffer().clone();

    // Exactly one row of the list is highlighted, and it is the third.
    let highlighted: Vec<u16> = (0..buffer.area.height)
        .filter(|&y| {
            buffer[(2, y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
        .collect();
    assert_eq!(highlighted.len(), 1, "rows highlighted: {highlighted:?}");
}

#[test]
fn a_narrow_review_still_draws() {
    for (width, height) in [(40, 8), (24, 6), (14, 4)] {
        let mut review = review();
        let drawn = draw_review(&mut review, width, height);
        assert!(!drawn.is_empty(), "{width}x{height} drew nothing");
    }
}

#[test]
fn the_working_tree_sits_at_the_top_of_the_list() {
    let mut items = vec![Item::Worktree];
    items.push(Item::from(commit(0x3f2a1c9, "Phase 5: move detection")));
    let loader: Loader<'static> = Box::new(|_| Ok(vec![long("f.rs")]));
    let mut review = Review::new(
        items,
        loader,
        RenderOptions {
            theme: Theme::none(),
            ..RenderOptions::default()
        },
    );
    insta::assert_snapshot!(draw_review(&mut review, 90, 8));
}

#[test]
fn a_loading_history_says_so_in_the_title() {
    // The count moves while a large history is walked; a number that quietly
    // changes under the reader is worse than one that says it is still coming.
    let mut review = review();
    review.set_loading(true);
    let drawn = draw_review(&mut review, 90, 8);
    assert!(drawn.contains("loading"), "{drawn}");
}

#[test]
fn an_empty_history_says_so() {
    let loader: Loader<'static> = Box::new(|_| Ok(Vec::new()));
    let mut empty = Review::new(
        Vec::new(),
        loader,
        RenderOptions {
            theme: Theme::none(),
            ..RenderOptions::default()
        },
    );
    let drawn = draw_review(&mut empty, 60, 8);
    assert!(drawn.contains("no commits"), "{drawn}");
}

#[test]
fn the_focused_file_list_says_what_its_keys_do() {
    // The status line belongs to whichever pane is being driven. Showing the
    // diff's keys over a focused file list would advertise keys that choose
    // nothing there.
    let mut app = app(vec![
        long("src/main.rs"),
        entry("src/lib.rs", "pub fn a() {}\n", "pub fn b() {}\n"),
        entry("README.md", "# one\n", "# two\n"),
    ]);
    let _ = draw(&mut app, 110, 16);
    app.apply(Action::ToggleFocus);
    app.apply(Action::Down);

    let drawn = draw(&mut app, 110, 16);
    assert!(drawn.contains("file 2/3"), "{drawn}");
    assert!(drawn.contains("j/k choose"), "{drawn}");
    assert!(
        !drawn.contains("n/N change"),
        "the diff's keys are shown: {drawn}"
    );
}

#[test]
fn choosing_a_file_shows_that_file() {
    let mut app = app(vec![
        long("src/main.rs"),
        entry("src/lib.rs", "pub fn a() {}\n", "pub fn b() {}\n"),
    ]);
    let _ = draw(&mut app, 110, 16);

    let first = draw(&mut app, 110, 16);
    assert!(first.contains("b/src/main.rs"), "{first}");

    app.apply(Action::ToggleFocus);
    app.apply(Action::Down);
    let second = draw(&mut app, 110, 16);
    assert!(second.contains("b/src/lib.rs"), "{second}");
    assert!(second.contains("pub fn b()"), "{second}");
}

#[test]
fn the_file_list_in_a_review_can_be_focused() {
    let loader: Loader<'static> = Box::new(|_| {
        Ok(vec![
            long("src/main.rs"),
            long("src/lib.rs"),
            long("README.md"),
        ])
    });
    let mut review = Review::new(
        vec![Item::from(commit(0x3f2a1c9, "Phase 5: move detection"))],
        loader,
        RenderOptions {
            theme: Theme::none(),
            ..RenderOptions::default()
        },
    );
    let _ = draw_review(&mut review, 110, 24);
    review.apply(Event::Open);
    // Log -> file list.
    review.apply(Event::ToggleFocus);
    review.apply(Event::ToggleFocus);

    insta::assert_snapshot!(draw_review(&mut review, 110, 24));
}

#[test]
fn the_help_overlay_lists_the_keys_and_what_the_settings_are() {
    let mut app = app(vec![long("src/main.rs"), long("src/lib.rs")]);
    let _ = draw(&mut app, 110, 30);
    app.apply(Action::ToggleHelp);
    insta::assert_snapshot!(draw(&mut app, 110, 30));
}

#[test]
fn the_help_overlay_follows_the_settings_it_reports() {
    let mut app = app(vec![long("src/main.rs")]);
    let _ = draw(&mut app, 110, 30);
    app.apply(Action::ToggleHelp);

    let before = draw(&mut app, 110, 30);
    assert!(before.contains("wrapped"), "{before}");
    assert!(before.contains("changed parts"), "{before}");

    app.apply(Action::ToggleWrap);
    app.apply(Action::ToggleFold);
    let after = draw(&mut app, 110, 30);
    assert!(after.contains("truncated"), "{after}");
    assert!(after.contains("whole file"), "{after}");
}

#[test]
fn settings_changed_in_one_commit_apply_to_the_next() {
    // A setting belongs to the reader, not to the commit they happened to be
    // looking at when they changed it.
    let loader: Loader<'static> = Box::new(|_| Ok(vec![long("f.rs"), long("g.rs")]));
    let mut review = Review::new(
        vec![Item::from(commit(1, "one")), Item::from(commit(2, "two"))],
        loader,
        RenderOptions {
            width: Some(110),
            ..RenderOptions::default()
        },
    );
    let _ = draw_review(&mut review, 110, 24);

    review.apply(Event::Open);
    review.apply(Event::Diff(Action::ToggleWrap));
    let wrap = review.diff().expect("a diff").render_options().wrap;

    // Move to the next commit and let it load.
    review.apply(Event::ToggleFocus);
    review.apply(Event::ToggleFocus);
    review.apply(Event::ToggleFocus);
    review.apply(Event::Down);
    review.settle();

    assert_eq!(
        review.diff().expect("a diff").render_options().wrap,
        wrap,
        "the setting did not survive the next commit"
    );
}

#[test]
fn a_diff_setting_changed_in_a_review_reloads_the_next_commit_with_it() {
    let loader: Loader<'static> = Box::new(|_| Ok(vec![long("f.rs")]));
    let mut review = Review::new(
        vec![Item::from(commit(1, "one")), Item::from(commit(2, "two"))],
        loader,
        RenderOptions {
            width: Some(110),
            ..RenderOptions::default()
        },
    );
    let _ = draw_review(&mut review, 110, 24);

    review.apply(Event::Open);
    review.apply(Event::Diff(Action::CycleWhitespace));
    assert_eq!(review.diff_options().whitespace_name(), "ignore-change");

    review.apply(Event::ToggleFocus);
    review.apply(Event::ToggleFocus);
    review.apply(Event::ToggleFocus);
    review.apply(Event::Down);
    review.settle();

    assert_eq!(
        review
            .diff()
            .expect("a diff")
            .diff_options()
            .whitespace_name(),
        "ignore-change",
        "the next commit was diffed with the old settings"
    );
}
