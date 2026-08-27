//! Snapshot coverage for every renderer, from one fixture.
//!
//! The shared fixture is the point: the unified view, the split view at three
//! widths and the JSON all describe the *same* document. Give each renderer its
//! own example and they drift — one grows a case the others never see, and the
//! JSON quietly stops matching what the terminal shows.
//!
//! Snapshots are rendered with the `none` palette. Colour is asserted by the
//! unit tests in `theme`; putting escape codes in a snapshot would make every
//! palette tweak look like a rendering regression.
//!
//! These snapshots pin *content and shape*. They do not pin row widths — insta
//! strips trailing whitespace from the end of a snapshot, so the final row
//! always appears short. The width invariant is asserted directly, against the
//! renderer, in `render::split`'s unit tests.

use increment::diff::{self, Options as DiffOptions};
use increment::highlight::Highlighting;
use increment::model::{DiffDocument, SourceFile};
use increment::render::{self, Options as RenderOptions, View, width::Wrap};
use increment::theme::Theme;

const OLD: &str = include_str!("fixtures/catalog.old.rs");
const NEW: &str = include_str!("fixtures/catalog.new.rs");

fn fixture(context: Option<usize>) -> DiffDocument {
    let old = SourceFile::from_text("a/catalog.rs", OLD);
    let new = SourceFile::from_text("b/catalog.rs", NEW);
    diff::compare(
        &old,
        &new,
        &DiffOptions {
            context,
            ..DiffOptions::default()
        },
    )
}

fn draw(document: &DiffDocument, view: View, width: Option<usize>, wrap: Wrap) -> String {
    let options = RenderOptions {
        theme: Theme::none(),
        width,
        wrap,
        ..RenderOptions::default()
    };
    let mut buffer = Vec::new();
    render::render(document, &Highlighting::none(), view, &options, &mut buffer).expect("renders");
    String::from_utf8(buffer).expect("utf-8")
}

#[test]
fn unified_view() {
    insta::assert_snapshot!(draw(&fixture(Some(3)), View::Unified, None, Wrap::Wrap));
}

#[test]
fn split_view_at_80_columns() {
    insta::assert_snapshot!(draw(&fixture(Some(3)), View::Split, Some(80), Wrap::Wrap));
}

#[test]
fn split_view_at_120_columns() {
    insta::assert_snapshot!(draw(&fixture(Some(3)), View::Split, Some(120), Wrap::Wrap));
}

#[test]
fn split_view_at_200_columns() {
    insta::assert_snapshot!(draw(&fixture(Some(3)), View::Split, Some(200), Wrap::Wrap));
}

#[test]
fn split_view_truncating_at_80_columns() {
    insta::assert_snapshot!(draw(
        &fixture(Some(3)),
        View::Split,
        Some(80),
        Wrap::Truncate
    ));
}

#[test]
fn split_view_without_folding() {
    insta::assert_snapshot!(draw(&fixture(None), View::Split, Some(120), Wrap::Wrap));
}

#[test]
fn json_view() {
    let mut buffer = Vec::new();
    render::json::render(&fixture(Some(3)), &mut buffer).expect("renders");
    insta::assert_snapshot!(String::from_utf8(buffer).expect("utf-8"));
}

/// `auto` picks the split view only where it fits, and the fallback is a real
/// unified diff rather than a squeezed split.
#[test]
fn auto_falls_back_to_unified_on_a_narrow_terminal() {
    let document = fixture(Some(3));
    let narrow = draw(&document, View::Auto, Some(80), Wrap::Wrap);
    let unified = draw(&document, View::Unified, Some(80), Wrap::Wrap);
    assert_eq!(narrow, unified);

    let wide = draw(&document, View::Auto, Some(200), Wrap::Wrap);
    let split = draw(&document, View::Split, Some(200), Wrap::Wrap);
    assert_eq!(wide, split);
}

/// The browser and the pager draw the same rows.
///
/// The phase-4 criterion asked for both surfaces to be driven from one fixture
/// document. They are — but the guarantee that actually holds is stronger than
/// a shared fixture: both call `render::split::compose`, so there is one
/// implementation of what a diff looks like and two ways of putting it on a
/// screen. This test pins that, and would fail the moment either surface grew
/// its own layout.
#[test]
fn the_browser_and_the_pager_lay_out_identically() {
    use increment::render::split;
    use increment::tui::state::{App, Entry};

    let document = fixture(Some(3));
    let options = RenderOptions {
        theme: Theme::none(),
        width: Some(120),
        ..RenderOptions::default()
    };

    let composed = split::compose(&document, &Highlighting::none(), &options);

    let mut app = App::new(
        vec![Entry::new(document.clone(), None, Highlighting::none())],
        options,
    );
    app.set_viewport(120, composed.len().max(1));

    let text = |rows: &[split::VisualRow]| -> Vec<String> {
        rows.iter()
            .map(|row| row.cells.iter().map(|cell| cell.text.as_str()).collect())
            .collect()
    };

    assert_eq!(text(app.layout()), text(&composed));
}

/// What `--whole-file` promises: the whole file, not a wider window onto it.
///
/// The alignment property test guarantees this of `align`; this pins it end to
/// end, through folding and the split renderer, which is where the flag
/// actually has to hold.
#[test]
fn the_whole_file_view_shows_every_line_of_both_sides() {
    let old = SourceFile::from_text("a/catalog.rs", OLD);
    let new = SourceFile::from_text("b/catalog.rs", NEW);

    let whole = draw(
        &diff::compare(
            &old,
            &new,
            &DiffOptions {
                context: None,
                ..DiffOptions::default()
            },
        ),
        View::Split,
        Some(200),
        Wrap::Wrap,
    );

    for (index, line) in old.lines.iter().enumerate() {
        assert!(
            whole.contains(line.trim()),
            "old line {} is missing: {line:?}",
            index + 1
        );
    }
    for (index, line) in new.lines.iter().enumerate() {
        assert!(
            whole.contains(line.trim()),
            "new line {} is missing: {line:?}",
            index + 1
        );
    }

    // And nothing is folded away, because there is nothing left to fold.
    assert!(
        !whole.contains("unchanged line"),
        "the whole-file view still folded something:\n{whole}"
    );
}

/// The contrast: folding is still the default, and still hides things.
#[test]
fn the_default_view_folds_and_says_how_much_it_hid() {
    let folded = draw(&fixture(Some(3)), View::Split, Some(200), Wrap::Wrap);
    assert!(folded.contains("unchanged lines"), "{folded}");

    let old = SourceFile::from_text("a/catalog.rs", OLD);
    let hidden = old
        .lines
        .iter()
        .filter(|line| !folded.contains(line.trim()) && !line.trim().is_empty())
        .count();
    assert!(
        hidden > 0,
        "folding hid nothing, so the fixture proves nothing"
    );
}
