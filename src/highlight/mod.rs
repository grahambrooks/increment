//! Syntax highlighting.
//!
//! `syntect` over bat's syntax and theme assets (`two-face`), taken with
//! `default-features = false` and the fancy regex engine so the `onig` C
//! library stays out of the build. Both crates default to onig, and cargo
//! unifies features across them — taking only *one* of them without defaults
//! is not enough, which is why `two-face` is pinned to `syntect-fancy` too.
//!
//! Two rules this module owes the renderers:
//!
//! - **Degrade, never fail.** An unrecognised language, an unreadable theme or
//!   a parse that gives up is not an error; it is a diff without colour, which
//!   is still a diff. Every entry point returns "no colours" rather than an
//!   `Err`.
//! - **Foreground only.** The diff's own colours live in the background, and
//!   the two must compose: syntax says what a token *is*, the row says what
//!   happened to it. A highlighter that set backgrounds would erase the diff.

use std::sync::OnceLock;

use anstyle::{Color, RgbColor};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme as SyntectTheme;
use syntect::parsing::SyntaxSet;

use crate::model::Span;

/// Beyond this many lines, a file is shown without syntax colour.
const MAX_LINES: usize = 20_000;

/// How long one file may spend being coloured before it is left as it is.
///
/// A line-count cap does not catch the case that actually hurts. This project's
/// own README — 176 lines of markdown with tables in it — takes **1.6 seconds**
/// to highlight in a debug build and 120ms in release, while a markdown file of
/// the same length beside it takes 98ms and 9ms. That is catastrophic
/// backtracking in `fancy-regex`, the pure-Rust engine chosen over `onig`, and
/// no bound on *size* will find it.
///
/// Whatever was coloured before the budget ran out is kept, so the top of the
/// file — which is what the parser reaches first, and usually what is on screen
/// — still has colour.
const BUDGET: std::time::Duration = std::time::Duration::from_millis(120);

// The clock is read on every line, not every sixteenth. Reading it costs
// nanoseconds against milliseconds of regex work, and striding overshoots
// badly in exactly the case the budget exists for: one pathological line can
// take 50ms on its own, so a stride of sixteen ran 900ms past a 120ms budget.
// The bound is now the budget plus one line, which is the best that can be
// done without preempting mid-line.

/// A line's colours, as byte ranges into that line.
pub type Colours = Vec<(Span, Color)>;

/// Both sides of a comparison, coloured.
///
/// Computed up front and handed to a renderer, rather than stored on the model
/// or looked up mid-draw. Two reasons, and they are the whole design of this
/// module's interface: the model must stay free of anything to do with colour,
/// and a syntax parser is stateful, so colours have to be produced from the
/// *whole file* in order — including the lines folding will hide.
#[derive(Debug, Clone, Default)]
pub struct Highlighting {
    old: Vec<Colours>,
    new: Vec<Colours>,
}

impl Highlighting {
    /// No highlighting. What `--syntax off`, an unknown language and a
    /// non-terminal surface all resolve to.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn of(old: &crate::model::SourceFile, new: &crate::model::SourceFile) -> Self {
        // Half the budget each, rather than a whole one each. A pair is one
        // file to the reader, and spending the budget twice makes the cap say
        // one thing and do another. Halving also stops the old side using it
        // all and leaving the new side — the one being read — with none.
        let each = BUDGET / 2;
        Self {
            old: colour_within(&old.name, &old.lines, each),
            new: colour_within(&new.name, &new.lines, each),
        }
    }

    /// Colours for a 1-based line number on the left-hand side.
    pub fn old_line(&self, number: usize) -> &[(Span, Color)] {
        Self::at(&self.old, number)
    }

    /// Colours for a 1-based line number on the right-hand side.
    pub fn new_line(&self, number: usize) -> &[(Span, Color)] {
        Self::at(&self.new, number)
    }

    fn at(lines: &[Colours], number: usize) -> &[(Span, Color)] {
        number
            .checked_sub(1)
            .and_then(|index| lines.get(index))
            .map_or(&[], Vec::as_slice)
    }
}

fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static SyntectTheme {
    static THEME: OnceLock<SyntectTheme> = OnceLock::new();
    THEME.get_or_init(|| {
        two_face::theme::extra()
            .get(two_face::theme::EmbeddedThemeName::Base16OceanDark)
            .clone()
    })
}

/// Colour every line of a file.
///
/// Returns one entry per line, empty where nothing is known. The whole file is
/// parsed rather than only the visible rows because a syntax parser is
/// stateful: line 400's colours depend on whether line 12 opened a string. A
/// renderer cannot skip ahead, so folding saves drawing, not parsing.
///
/// `name` selects the language by extension. An unknown extension yields empty
/// colours for every line.
pub fn colour_file(name: &str, lines: &[String]) -> Vec<Colours> {
    colour_within(name, lines, BUDGET)
}

/// [`colour_file`], with the time budget given rather than assumed.
///
/// Separate so a test can prove the budget does something, by comparing a
/// generous one against a mean one on the same input.
pub fn colour_within(name: &str, lines: &[String], budget: std::time::Duration) -> Vec<Colours> {
    let empty = || vec![Colours::new(); lines.len()];

    // Measured at roughly 60–80ms for a few hundred lines in release, and the
    // parser cannot skip ahead — line 400's colours depend on line 12. Past a
    // point the wait costs more than the colour is worth, and a diff without
    // syntax colour is still a diff.
    if lines.len() > MAX_LINES {
        return empty();
    }

    let Some(extension) = extension(name) else {
        return empty();
    };
    let syntaxes = syntaxes();
    let Some(syntax) = syntaxes.find_syntax_by_extension(extension) else {
        return empty();
    };

    let mut highlighter = HighlightLines::new(syntax, theme());
    let started = std::time::Instant::now();
    let mut out = Vec::with_capacity(lines.len());

    for line in lines {
        if started.elapsed() > budget {
            // Out of time. Everything from here on is plain, which is a diff
            // that draws now rather than a prettier one that draws in a second.
            out.resize(lines.len(), Colours::new());
            break;
        }

        // The syntax definitions expect newline-terminated input; ours are
        // stored without terminators.
        let terminated = format!("{line}\n");
        out.push(match highlighter.highlight_line(&terminated, syntaxes) {
            Ok(regions) => spans(line, &regions),
            // A parser that gives up mid-file leaves the rest uncoloured
            // rather than taking the diff down with it.
            Err(_) => Colours::new(),
        });
    }

    debug_assert_eq!(out.len(), lines.len());
    out
}

/// Convert syntect's `(style, text)` regions into byte ranges and colours.
fn spans(line: &str, regions: &[(syntect::highlighting::Style, &str)]) -> Colours {
    let mut colours = Colours::new();
    let mut offset = 0usize;

    for (style, text) in regions {
        // The trailing newline this module added is not part of the line.
        let text = text.strip_suffix('\n').unwrap_or(text);
        if text.is_empty() {
            continue;
        }
        let end = (offset + text.len()).min(line.len());
        if offset < end {
            let colour = style.foreground;
            colours.push((
                Span::new(offset, end),
                Color::Rgb(RgbColor(colour.r, colour.g, colour.b)),
            ));
        }
        offset = end;
    }

    colours
}

fn extension(name: &str) -> Option<&str> {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn a_known_language_gets_colours() {
        let source = lines("fn main() {\n    let x = 1;\n}");
        let coloured = colour_file("main.rs", &source);

        assert_eq!(coloured.len(), source.len());
        assert!(
            coloured.iter().any(|line| !line.is_empty()),
            "expected some colours"
        );
    }

    #[test]
    fn keywords_and_literals_are_coloured_differently() {
        let source = lines("let x = 1;");
        let coloured = colour_file("main.rs", &source);
        let distinct: std::collections::HashSet<_> =
            coloured[0].iter().map(|(_, colour)| *colour).collect();
        assert!(distinct.len() > 1, "everything came out one colour");
    }

    #[test]
    fn an_unknown_extension_degrades_to_no_colour() {
        // Not an error. A diff without syntax colour is still a diff.
        let source = lines("some text\nmore text");
        let coloured = colour_file("notes.unheardof", &source);
        assert_eq!(coloured.len(), 2);
        assert!(coloured.iter().all(Vec::is_empty));
    }

    /// Markdown that is slow but not absurd — the shape of the real case.
    fn slow_markdown() -> Vec<String> {
        (0..300)
            .map(|n| format!("| **a{n}** *b* `c` | [d](e) **_f_** | *g* *h* *i* |"))
            .collect()
    }

    #[test]
    fn the_budget_stops_a_slow_file_early_and_keeps_what_it_had() {
        // Not a size problem — this is 300 short lines. Nested emphasis and
        // tables are what make the markdown definition backtrack, and no bound
        // on *size* finds that. Only the clock does.
        let source = slow_markdown();

        let generous = Instant::now();
        let all = colour_within("slow.md", &source, Duration::from_secs(60));
        let generous = generous.elapsed();

        let mean = Instant::now();
        let some = colour_within("slow.md", &source, Duration::from_millis(1));
        let mean = mean.elapsed();

        // Every line still has an entry either way; only the colour is missing.
        assert_eq!(all.len(), source.len());
        assert_eq!(some.len(), source.len());

        assert!(
            some.iter().any(Vec::is_empty),
            "the budget never bit, so this proves nothing"
        );
        assert!(
            mean < generous,
            "a 1ms budget ({mean:?}) was no faster than an unbounded one ({generous:?})"
        );
    }

    #[test]
    fn what_the_budget_manages_to_colour_is_the_start_of_the_file() {
        // The parser runs forwards from line one, so a partial result is the
        // top of the file — which is the part most likely to be on screen.
        let source = slow_markdown();
        let some = colour_within("slow.md", &source, Duration::from_millis(30));

        let coloured = some.iter().take_while(|line| !line.is_empty()).count();
        let plain = some
            .iter()
            .skip(coloured)
            .filter(|line| line.is_empty())
            .count();
        assert!(coloured > 0, "nothing was coloured at all");
        assert_eq!(
            coloured + plain,
            source.len(),
            "the coloured lines should be a prefix, not scattered"
        );
    }

    #[test]
    fn a_very_large_file_is_shown_without_colour_rather_than_slowly() {
        let source: Vec<String> = (0..MAX_LINES + 1)
            .map(|n| format!("let x{n} = {n};"))
            .collect();
        let coloured = colour_file("big.rs", &source);
        assert_eq!(coloured.len(), source.len());
        assert!(coloured.iter().all(Vec::is_empty));
    }

    #[test]
    fn a_file_with_no_extension_degrades_to_no_colour() {
        let coloured = colour_file("Makefile", &lines("all:\n\techo hi"));
        assert!(coloured.iter().all(Vec::is_empty));
    }

    #[test]
    fn spans_stay_within_the_line_and_on_utf8_boundaries() {
        let source = lines("let 名前 = \"文字\";");
        let coloured = colour_file("main.rs", &source);
        for (span, _) in &coloured[0] {
            assert!(span.end <= source[0].len());
            // Panics if a span splits a multi-byte character.
            let _ = &source[0][span.start..span.end];
        }
    }

    #[test]
    fn every_line_gets_an_entry_even_when_blank() {
        let source = lines("fn a() {}\n\nfn b() {}");
        assert_eq!(colour_file("main.rs", &source).len(), 3);
    }

    #[test]
    fn no_highlighting_answers_for_every_line_without_panicking() {
        let none = Highlighting::none();
        assert!(none.old_line(1).is_empty());
        assert!(none.new_line(9_999).is_empty());
        // Line numbers are 1-based; 0 is not a line, and asking must not wrap.
        assert!(none.old_line(0).is_empty());
    }

    #[test]
    fn highlighting_is_looked_up_by_one_based_line_number() {
        use crate::model::SourceFile;
        let old = SourceFile::from_text("main.rs", "fn a() {}\nlet x = 1;\n");
        let new = SourceFile::from_text("main.rs", "fn a() {}\nlet x = 2;\n");
        let highlighting = Highlighting::of(&old, &new);

        assert!(!highlighting.old_line(1).is_empty());
        assert!(!highlighting.new_line(2).is_empty());
        // Past the end is empty, not a panic.
        assert!(highlighting.old_line(50).is_empty());
    }
}
