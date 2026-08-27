//! Navigation, as a state machine that has never heard of a terminal.
//!
//! Everything about *where the reader is* lives here: which file, how far down
//! it, what is folded, what is being searched for. [`super::draw`] turns that
//! into cells and nothing else.
//!
//! The separation is not tidiness. A browser's bugs are almost all navigation
//! bugs — scrolling past the end, a jump that lands one row off, a search that
//! finds a match nobody can see — and none of them need a screen to reproduce.
//! Every one of them is a unit test in this file.

use std::cell::OnceCell;
use std::rc::Rc;

use crate::diff::Options as DiffOptions;
use crate::highlight::Highlighting;
use crate::model::{DiffDocument, RowKind, SourceFile};
use crate::render::Options;
use crate::render::split::{VisualRow, compose};

/// A value computed the first time it is wanted, if ever.
struct Lazy<T> {
    value: OnceCell<T>,
    source: Option<Box<dyn Fn() -> T>>,
}

impl<T> Lazy<T> {
    fn ready(value: T) -> Self {
        let cell = OnceCell::new();
        let _ = cell.set(value);
        Self {
            value: cell,
            source: None,
        }
    }

    fn deferred(source: impl Fn() -> T + 'static) -> Self {
        Self {
            value: OnceCell::new(),
            source: Some(Box::new(source)),
        }
    }

    fn is_ready(&self) -> bool {
        self.value.get().is_some()
    }

    /// The value, computing it now if it has not been computed.
    fn force(&self) -> &T
    where
        T: Default,
    {
        self.value.get_or_init(|| match &self.source {
            Some(source) => source(),
            None => T::default(),
        })
    }

    /// The value if it is already known, without computing it.
    fn peek(&self) -> Option<&T> {
        self.value.get()
    }
}

/// One file in the browser.
pub struct Entry {
    pub document: DiffDocument,
    /// The same comparison with nothing folded.
    ///
    /// Deferred: it is only wanted if the reader presses `f`, and computing it
    /// on load doubled the diffing done for every file in a commit to serve a
    /// key most of them never get.
    ///
    /// Resolves to `None` for a patch, which never carried the hidden lines —
    /// so there is nothing to unfold, and saying so beats a key that does
    /// nothing.
    unfolded: Lazy<Option<DiffDocument>>,
    /// Deferred for a different reason: it is slow. Highlighting one file can
    /// cost more than everything else about loading a commit put together, so
    /// the first frame is drawn without it and it arrives on the next.
    highlighting: Lazy<Highlighting>,
    /// The two files this was made from, and the settings that made it.
    ///
    /// Kept so the reader can change what a diff *is* — whether whitespace
    /// counts, whether moves are detected — without going back to the shell.
    /// A patch has no sources, so those settings are fixed for it, and the
    /// browser says so rather than pretending.
    sources: Option<Rc<(SourceFile, SourceFile)>>,
}

impl Entry {
    /// An entry whose parts are already known, and which cannot be re-diffed.
    pub fn new(
        document: DiffDocument,
        unfolded: Option<DiffDocument>,
        highlighting: Highlighting,
    ) -> Self {
        Self {
            document,
            unfolded: Lazy::ready(unfolded),
            highlighting: Lazy::ready(highlighting),
            sources: None,
        }
    }

    /// An entry that keeps its sources, and computes the expensive parts only
    /// when something asks for them.
    pub fn from_sources(old: SourceFile, new: SourceFile, options: DiffOptions) -> Self {
        Self::rebuilt(Rc::new((old, new)), options)
    }

    fn rebuilt(sources: Rc<(SourceFile, SourceFile)>, options: DiffOptions) -> Self {
        let document = crate::diff::compare(&sources.0, &sources.1, &options);

        let whole = Rc::clone(&sources);
        let colours = Rc::clone(&sources);
        Self {
            document,
            unfolded: Lazy::deferred(move || {
                Some(crate::diff::compare(
                    &whole.0,
                    &whole.1,
                    &DiffOptions {
                        context: None,
                        ..options
                    },
                ))
            }),
            highlighting: Lazy::deferred(move || Highlighting::of(&colours.0, &colours.1)),
            sources: Some(sources),
        }
    }

    /// The same two files, diffed differently. `None` without sources.
    pub fn rediff(&self, options: DiffOptions) -> Option<Self> {
        self.sources
            .as_ref()
            .map(|sources| Self::rebuilt(Rc::clone(sources), options))
    }

    /// Whether this entry can be diffed again with other settings.
    pub fn can_rediff(&self) -> bool {
        self.sources.is_some()
    }

    /// The unfolded document, computing it if this is the first ask.
    pub fn unfolded(&self) -> Option<&DiffDocument> {
        self.unfolded.force().as_ref()
    }

    /// Colours if they have been computed; otherwise none, without computing.
    pub fn colours(&self) -> &Highlighting {
        static NONE: std::sync::OnceLock<Highlighting> = std::sync::OnceLock::new();
        self.highlighting
            .peek()
            .unwrap_or_else(|| NONE.get_or_init(Highlighting::none))
    }

    /// Whether the colours are still owed.
    pub fn needs_colour(&self) -> bool {
        !self.highlighting.is_ready()
    }

    /// Compute the colours now.
    pub fn colour_now(&self) {
        let _ = self.highlighting.force();
    }

    pub fn name(&self) -> &str {
        &self.document.new.name
    }
}

/// Which pane the keys are talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Files,
    Diff,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Search {
    #[default]
    Off,
    /// Being typed. The diff does not move until it is committed.
    Typing(String),
    Active {
        query: String,
        /// Visual row of every match, in order.
        matches: Vec<usize>,
        at: usize,
    },
}

/// What a keypress means. Deliberately separate from the key that produced it,
/// so the bindings can be tested without inventing terminal events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    NextChange,
    PreviousChange,
    NextFile,
    PreviousFile,
    /// Leave the file list for the diff of the file just chosen.
    OpenFile,
    ToggleFold,
    ToggleFocus,
    ToggleWrap,
    ToggleSyntax,
    ToggleLineNumbers,
    CycleTheme,
    /// Re-diffs: these change what the diff is, not how it is drawn.
    CycleWhitespace,
    ToggleMoves,
    ToggleHelp,
    SearchStart,
    SearchType(char),
    SearchBackspace,
    SearchCommit,
    SearchCancel,
}

pub struct App {
    entries: Vec<Entry>,
    options: Options,
    selected: usize,
    scroll: usize,
    /// The row the reader was last sent to, which is not always the row at the
    /// top of the screen.
    ///
    /// Near the end of a file the scroll clamps — there is nothing below to
    /// show — so jumping to the last change leaves `scroll` short of it.
    /// Searching from `scroll` would then find that same change again and
    /// again, and `n` would look broken. Jumps count from here instead.
    anchor: usize,
    /// Rows the diff pane can show. Set by the drawer, because only it knows.
    height: usize,
    focus: Focus,
    unfolded: bool,
    search: Search,
    quit: bool,
    /// The composed layout of the selected entry, cached until something that
    /// changes it changes.
    layout: Vec<VisualRow>,
    /// A one-shot note for the status line.
    notice: Option<String>,
    /// The settings the entries were diffed with, so changing them can diff
    /// again rather than reaching back into the shell.
    diff_options: DiffOptions,
    help: bool,
    /// Whether this view sits inside the review flow, where `q` goes back to
    /// the commit list rather than quitting. The status line has to say the
    /// right thing: a hint that names a key which does something else is worse
    /// than no hint.
    nested: bool,
}

impl App {
    pub fn new(entries: Vec<Entry>, options: Options) -> Self {
        let mut app = Self {
            entries,
            options,
            selected: 0,
            scroll: 0,
            anchor: 0,
            height: 1,
            focus: Focus::Diff,
            unfolded: false,
            search: Search::Off,
            quit: false,
            layout: Vec::new(),
            notice: None,
            nested: false,
            diff_options: DiffOptions::default(),
            help: false,
        };
        app.relayout();
        app
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn render_options(&self) -> Options {
        self.options
    }

    pub fn diff_options(&self) -> DiffOptions {
        self.diff_options
    }

    pub fn set_render_options(&mut self, options: Options) {
        self.options = options;
        self.relayout();
    }

    /// Diff every file again with new settings.
    ///
    /// Entries without sources — a patch — keep what they have, and the reader
    /// is told, because a setting that silently applies to some files and not
    /// others is worse than one that says where it stops.
    pub fn set_diff_options(&mut self, options: DiffOptions) {
        self.diff_options = options;

        let mut fixed = 0usize;
        for entry in &mut self.entries {
            match entry.rediff(options) {
                Some(rebuilt) => *entry = rebuilt,
                None => fixed += 1,
            }
        }
        if fixed > 0 {
            self.notice = Some(format!(
                "{fixed} file{} came from a patch and cannot be diffed again",
                if fixed == 1 { "" } else { "s" }
            ));
        }
        self.relayout();
    }

    pub fn showing_help(&self) -> bool {
        self.help
    }

    /// Whether the whole file is on show rather than only the changed parts.
    pub fn is_unfolded(&self) -> bool {
        self.unfolded
    }

    /// Mark this view as living inside the review flow.
    pub fn nest(&mut self) {
        self.nested = true;
    }

    pub fn is_nested(&self) -> bool {
        self.nested
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn set_focus(&mut self, focus: Focus) {
        // Focusing a list with nothing to choose from is a dead end: the keys
        // would do nothing and the only way out would be another Tab.
        self.focus = if focus == Focus::Files && !self.can_choose_file() {
            Focus::Diff
        } else {
            focus
        };
    }

    /// Whether there is more than one file to move between.
    pub fn can_choose_file(&self) -> bool {
        self.entries.len() > 1
    }

    /// Whether the file on show is still waiting for its colours.
    ///
    /// The event loop asks when no keypress is queued, so a commit draws
    /// immediately and gains colour a frame later instead of stalling on it.
    pub fn needs_colour(&self) -> bool {
        self.options.syntax_visible() && self.entry().is_some_and(Entry::needs_colour)
    }

    /// Colour the file on show, and lay it out again with the result.
    pub fn colour_now(&mut self) {
        if let Some(entry) = self.entry() {
            entry.colour_now();
        }
        self.relayout();
    }

    pub fn search(&self) -> &Search {
        &self.search
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn layout(&self) -> &[VisualRow] {
        &self.layout
    }

    pub fn entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// The document currently on show — folded or not.
    pub fn document(&self) -> Option<&DiffDocument> {
        let entry = self.entry()?;
        Some(if self.unfolded {
            entry.unfolded().unwrap_or(&entry.document)
        } else {
            &entry.document
        })
    }

    /// Tell the app how much room the diff pane has.
    ///
    /// Called by the drawer every frame: the terminal can be resized between
    /// any two of them, and a scroll position that was valid before may not be.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        let changed = self.options.width != Some(width);
        self.height = height.max(1);
        if changed {
            self.options.width = Some(width);
            self.relayout();
        }
        self.clamp();
    }

    pub fn apply(&mut self, action: Action) {
        self.notice = None;

        // While a query is being typed the diff stays put, so the reader can
        // still see where they are.
        if let Search::Typing(query) = &mut self.search {
            match action {
                Action::SearchType(c) => {
                    query.push(c);
                    return;
                }
                Action::SearchBackspace => {
                    query.pop();
                    return;
                }
                Action::SearchCommit => {
                    let query = std::mem::take(query);
                    self.commit_search(query);
                    return;
                }
                Action::SearchCancel | Action::Quit => {
                    self.search = Search::Off;
                    return;
                }
                _ => {}
            }
        }

        // Moving by hand puts the anchor back under the reader's control.
        let manual = matches!(
            action,
            Action::Up
                | Action::Down
                | Action::PageUp
                | Action::PageDown
                | Action::Top
                | Action::Bottom
        );

        match action {
            Action::Quit => self.quit = true,
            // With the file list focused these choose a file; with the diff
            // focused they scroll it. Before this the focus only recoloured a
            // border — the keys scrolled the diff either way, which made the
            // file list something you could look at but not use.
            Action::Up
            | Action::Down
            | Action::PageUp
            | Action::PageDown
            | Action::Top
            | Action::Bottom
                if self.focus == Focus::Files =>
            {
                let last = self.entries.len().saturating_sub(1);
                let step = self.height.max(1);
                self.select(match action {
                    Action::Up => self.selected.saturating_sub(1),
                    Action::Down => (self.selected + 1).min(last),
                    Action::PageUp => self.selected.saturating_sub(step),
                    Action::PageDown => (self.selected + step).min(last),
                    Action::Top => 0,
                    _ => last,
                });
            }
            Action::Up => self.scroll = self.scroll.saturating_sub(1),
            Action::Down => self.scroll += 1,
            Action::PageUp => self.scroll = self.scroll.saturating_sub(self.height),
            Action::PageDown => self.scroll += self.height,
            Action::Top => self.scroll = 0,
            Action::Bottom => self.scroll = usize::MAX,
            Action::NextChange => self.jump(true),
            Action::PreviousChange => self.jump(false),
            Action::NextFile => self.select(self.selected.saturating_add(1)),
            Action::PreviousFile => self.select(self.selected.saturating_sub(1)),
            Action::ToggleFold => self.toggle_fold(),
            Action::ToggleWrap => {
                self.options.toggle_wrap();
                self.relayout();
            }
            Action::ToggleSyntax => {
                self.options.toggle_syntax();
                self.relayout();
            }
            Action::ToggleLineNumbers => {
                self.options.toggle_line_numbers();
                self.relayout();
            }
            Action::CycleTheme => {
                self.options.cycle_theme();
                self.relayout();
            }
            Action::CycleWhitespace => {
                let mut options = self.diff_options;
                options.cycle_whitespace();
                self.set_diff_options(options);
            }
            Action::ToggleMoves => {
                let mut options = self.diff_options;
                options.toggle_moves();
                self.set_diff_options(options);
            }
            Action::ToggleHelp => self.help = !self.help,
            // Choosing a file and then reading it are two steps, and this is
            // the second: the file list is behind you now.
            Action::OpenFile => self.focus = Focus::Diff,
            Action::ToggleFocus => self.set_focus(match self.focus {
                Focus::Files => Focus::Diff,
                Focus::Diff => Focus::Files,
            }),
            Action::SearchStart => self.search = Search::Typing(String::new()),
            Action::SearchCancel => self.search = Search::Off,
            Action::SearchType(_) | Action::SearchBackspace | Action::SearchCommit => {}
        }

        self.clamp();
        if manual {
            self.anchor = self.scroll;
        }
    }

    /// Send the reader to a visual row, keeping the scroll in range.
    fn go_to(&mut self, row: usize) {
        self.anchor = row;
        self.scroll = row;
        self.clamp();
    }

    /// The rows currently on screen.
    pub fn visible(&self) -> std::ops::Range<usize> {
        self.scroll..(self.scroll + self.height).min(self.layout.len().max(1))
    }

    fn select(&mut self, index: usize) {
        let index = index.min(self.entries.len().saturating_sub(1));
        if index == self.selected {
            return;
        }
        self.selected = index;
        self.scroll = 0;
        self.anchor = 0;
        // A search is about a file; carrying its matches to the next one would
        // point at rows that have nothing to do with the query.
        if matches!(self.search, Search::Active { .. }) {
            self.search = Search::Off;
        }
        self.relayout();
    }

    fn toggle_fold(&mut self) {
        let available = self.entry().is_some_and(|entry| entry.unfolded().is_some());
        if !available {
            self.notice =
                Some("this diff came from a patch; the hidden lines were never in it".to_owned());
            return;
        }

        // Keep the reader where they were: the row under the top of the screen
        // is the same line before and after, even though its position moves.
        let anchor = self.layout.get(self.scroll).map(|row| row.source);
        let anchor_line = anchor.and_then(|index| {
            let rows = &self.document()?.rows;
            let row = rows.get(index)?;
            row.left
                .as_ref()
                .or(row.right.as_ref())
                .map(|line| (line.number, row.left.is_some()))
        });

        self.unfolded = !self.unfolded;
        self.relayout();

        if let Some((number, from_left)) = anchor_line {
            self.scroll = self
                .visual_of_line(number, from_left)
                .unwrap_or(self.scroll);
        }
    }

    /// The visual row showing a given line number.
    fn visual_of_line(&self, number: usize, from_left: bool) -> Option<usize> {
        let document = self.document()?;
        self.layout.iter().position(|visual| {
            let row = &document.rows[visual.source];
            let line = if from_left {
                row.left.as_ref()
            } else {
                row.right.as_ref()
            };
            visual.first && line.is_some_and(|line| line.number == number)
        })
    }

    fn relayout(&mut self) {
        self.layout = match (self.document(), self.entry()) {
            // Colour is asked for only when the palette leaves room for it and
            // the reader has not turned it off — so turning it off also stops
            // it being computed, not just drawn.
            (Some(document), Some(entry)) if self.options.syntax_visible() => {
                compose(document, entry.colours(), &self.options)
            }
            (Some(document), _) => compose(document, &Highlighting::none(), &self.options),
            _ => Vec::new(),
        };
        // Any recorded match positions refer to the old layout.
        if let Search::Active { query, .. } = &self.search {
            let query = query.clone();
            self.commit_search(query);
        }
    }

    fn commit_search(&mut self, query: String) {
        if query.is_empty() {
            self.search = Search::Off;
            return;
        }

        let needle = query.to_lowercase();
        let matches: Vec<usize> = match self.document() {
            None => Vec::new(),
            Some(document) => self
                .layout
                .iter()
                .enumerate()
                .filter(|(_, visual)| {
                    let row = &document.rows[visual.source];
                    visual.first
                        && [row.left.as_ref(), row.right.as_ref()]
                            .into_iter()
                            .flatten()
                            .any(|line| line.text.to_lowercase().contains(&needle))
                })
                .map(|(index, _)| index)
                .collect(),
        };

        if matches.is_empty() {
            self.notice = Some(format!("no match for {query:?}"));
            self.search = Search::Off;
            return;
        }

        // Land on the first match at or after where the reader already is,
        // rather than yanking them back to the top of the file.
        let at = matches
            .iter()
            .position(|&row| row >= self.scroll)
            .unwrap_or(0);
        self.go_to(matches[at]);
        self.search = Search::Active { query, matches, at };
    }

    /// Move to the next or previous target: a search match if one is active,
    /// otherwise the start of the next change.
    fn jump(&mut self, forward: bool) {
        if let Search::Active { matches, at, .. } = &mut self.search {
            if matches.is_empty() {
                return;
            }
            *at = if forward {
                (*at + 1) % matches.len()
            } else {
                (*at + matches.len() - 1) % matches.len()
            };
            let row = matches[*at];
            self.go_to(row);
            return;
        }

        let starts = self.change_starts();
        let target = if forward {
            starts.iter().find(|&&row| row > self.anchor).copied()
        } else {
            starts.iter().rev().find(|&&row| row < self.anchor).copied()
        };
        if let Some(target) = target {
            self.go_to(target);
        } else {
            self.notice = Some(
                if forward {
                    "no further changes"
                } else {
                    "no earlier changes"
                }
                .to_owned(),
            );
        }
    }

    /// The visual row starting each run of changed rows.
    ///
    /// Runs, not rows: a twenty-line replacement is one thing the reader wants
    /// to look at, and "next change" that steps through it line by line is a
    /// slower way of pressing the down arrow.
    pub fn change_starts(&self) -> Vec<usize> {
        let Some(document) = self.document() else {
            return Vec::new();
        };
        self.layout
            .iter()
            .enumerate()
            .filter(|(_, visual)| {
                if !visual.first || !document.rows[visual.source].is_change() {
                    return false;
                }
                visual.source == 0 || !document.rows[visual.source - 1].is_change()
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// The change kind of each visual row, for the change map.
    pub fn row_kinds(&self) -> Vec<RowKind> {
        let Some(document) = self.document() else {
            return Vec::new();
        };
        self.layout
            .iter()
            .map(|visual| document.rows[visual.source].kind.clone())
            .collect()
    }

    pub fn max_scroll(&self) -> usize {
        self.layout.len().saturating_sub(self.height)
    }

    fn clamp(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Options as DiffOptions, compare};
    use crate::model::SourceFile;

    fn entry(old: &str, new: &str, context: Option<usize>) -> Entry {
        Entry::from_sources(
            SourceFile::from_text("a/f.rs", old),
            SourceFile::from_text("b/f.rs", new),
            DiffOptions {
                context,
                ..DiffOptions::default()
            },
        )
    }

    fn options() -> Options {
        let mut options = Options {
            width: Some(100),
            ..Options::default()
        };
        options.set_palette(crate::theme::Palette::None);
        options
    }

    /// Forty lines, with edits at 5 and 35.
    fn long() -> Entry {
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
        entry(&old, &new, Some(3))
    }

    fn app(entries: Vec<Entry>, height: usize) -> App {
        let mut app = App::new(entries, options());
        app.set_viewport(100, height);
        app
    }

    #[test]
    fn scrolling_stops_at_the_top() {
        let mut app = app(vec![long()], 5);
        app.apply(Action::Up);
        app.apply(Action::Up);
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn scrolling_stops_before_the_end_leaving_a_full_screen() {
        let mut app = app(vec![long()], 5);
        for _ in 0..500 {
            app.apply(Action::Down);
        }
        assert_eq!(app.scroll(), app.max_scroll());
        assert_eq!(app.scroll() + 5, app.layout().len());
    }

    #[test]
    fn bottom_and_top_go_all_the_way() {
        let mut app = app(vec![long()], 5);
        app.apply(Action::Bottom);
        assert_eq!(app.scroll(), app.max_scroll());
        app.apply(Action::Top);
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn a_short_diff_does_not_scroll_at_all() {
        // Fewer rows than the screen: there is nowhere to go, and allowing the
        // scroll to move would drag content off the top for no reason.
        let mut app = app(vec![entry("a\n", "b\n", Some(3))], 40);
        app.apply(Action::Bottom);
        assert_eq!(app.scroll(), 0);
        assert_eq!(app.max_scroll(), 0);
    }

    #[test]
    fn next_change_moves_to_the_following_change_not_the_following_row() {
        let mut app = app(vec![long()], 10);
        let starts = app.change_starts();
        assert!(starts.len() >= 2, "expected two changes, got {starts:?}");

        app.apply(Action::NextChange);
        assert!(
            app.visible().contains(&starts[1]),
            "the second change at {} is not on screen: {:?}",
            starts[1],
            app.visible()
        );
    }

    #[test]
    fn next_change_keeps_advancing_near_the_end_of_a_file() {
        // The scroll clamps before the last change can reach the top of the
        // screen. Counting the next jump from the scroll would find the same
        // change over and over, and `n` would look broken.
        let mut app = app(vec![long()], 10);
        let starts = app.change_starts();

        // One press per change, then one more to run off the end.
        for _ in 0..starts.len() + 1 {
            app.apply(Action::NextChange);
        }
        assert!(
            app.notice().is_some_and(|note| note.contains("no further")),
            "should have run out of changes, notice was {:?}",
            app.notice()
        );
    }

    #[test]
    fn a_run_of_changed_rows_counts_as_one_change() {
        // Five consecutive replaced lines are one thing to look at.
        let old = "keep\nalpha one\nbeta two\ngamma three\nkeep2\n";
        let new = "keep\nimpl A {\nfn b() {}\n}\nkeep2\n";
        let app = app(vec![entry(old, new, None)], 10);
        assert_eq!(app.change_starts().len(), 1, "{:?}", app.change_starts());
    }

    #[test]
    fn previous_change_goes_back_and_says_when_there_is_nowhere_to_go() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::NextChange);
        app.apply(Action::PreviousChange);
        assert!(app.visible().contains(&app.change_starts()[0]));

        app.apply(Action::PreviousChange);
        assert!(app.notice().is_some(), "should say there is nothing before");
    }

    #[test]
    fn moving_between_files_resets_the_scroll() {
        let mut app = app(vec![long(), long()], 5);
        app.apply(Action::Bottom);
        assert!(app.scroll() > 0);

        app.apply(Action::NextFile);
        assert_eq!(app.selected(), 1);
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn file_selection_stops_at_both_ends() {
        let mut app = app(vec![long(), long()], 5);
        app.apply(Action::PreviousFile);
        assert_eq!(app.selected(), 0);
        app.apply(Action::NextFile);
        app.apply(Action::NextFile);
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn unfolding_shows_more_rows_and_folding_puts_them_back() {
        let mut app = app(vec![long()], 10);
        let folded = app.layout().len();

        app.apply(Action::ToggleFold);
        assert!(
            app.layout().len() > folded,
            "unfolding should reveal rows: {} then {}",
            folded,
            app.layout().len()
        );

        app.apply(Action::ToggleFold);
        assert_eq!(app.layout().len(), folded);
    }

    #[test]
    fn unfolding_keeps_the_reader_on_the_same_line() {
        // The row at the top of the screen must still be at the top afterwards.
        // Without this the reader is thrown to a different part of the file by
        // a key whose whole purpose is to show more of where they already are.
        let mut app = app(vec![long()], 10);
        app.apply(Action::NextChange);

        let before = app.layout()[app.scroll()].source;
        let line = {
            let document = app.document().unwrap();
            document.rows[before].left.as_ref().unwrap().number
        };

        app.apply(Action::ToggleFold);

        let after = app.layout()[app.scroll()].source;
        let line_after = {
            let document = app.document().unwrap();
            document.rows[after].left.as_ref().unwrap().number
        };
        assert_eq!(line_after, line);
    }

    #[test]
    fn a_patch_diff_says_why_it_cannot_unfold() {
        // A patch-shaped entry: no sources, so nothing to unfold.
        let entry = Entry::new(
            compare(
                &SourceFile::from_text("a/f.rs", "a\nb\n"),
                &SourceFile::from_text("b/f.rs", "a\nc\n"),
                &DiffOptions::default(),
            ),
            None,
            Highlighting::none(),
        );

        let mut app = app(vec![entry], 10);
        app.apply(Action::ToggleFold);
        assert!(app.notice().is_some_and(|note| note.contains("patch")));
    }

    #[test]
    fn typing_a_search_does_not_move_the_diff_until_it_is_committed() {
        let mut app = app(vec![long()], 10);
        let before = app.scroll();

        app.apply(Action::SearchStart);
        for c in "line35".chars() {
            app.apply(Action::SearchType(c));
        }
        assert_eq!(app.scroll(), before, "the diff moved while typing");

        app.apply(Action::SearchCommit);
        assert_ne!(app.scroll(), before, "committing should jump to the match");
    }

    #[test]
    fn a_committed_search_lands_on_a_row_containing_the_query() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::SearchStart);
        for c in "line35".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchCommit);

        let document = app.document().unwrap();
        let found = app.visible().any(|row| {
            let source = app.layout()[row].source;
            let row = &document.rows[source];
            [row.left.as_ref(), row.right.as_ref()]
                .into_iter()
                .flatten()
                .any(|line| line.text.contains("line35"))
        });
        assert!(found, "the match is not on screen: {:?}", app.visible());
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut app = app(vec![entry("alpha\n", "BETA gamma\n", None)], 10);
        app.apply(Action::SearchStart);
        for c in "beta".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchCommit);
        assert!(matches!(app.search(), Search::Active { .. }));
    }

    #[test]
    fn a_search_with_no_match_says_so_and_leaves_the_reader_put() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::Bottom);
        let before = app.scroll();

        app.apply(Action::SearchStart);
        for c in "nothinglikethis".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchCommit);

        assert_eq!(app.scroll(), before, "a failed search should not move");
        assert!(app.notice().is_some_and(|note| note.contains("no match")));
        assert_eq!(*app.search(), Search::Off);
    }

    #[test]
    fn backspace_edits_the_query() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::SearchStart);
        for c in "linex".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchBackspace);
        assert_eq!(*app.search(), Search::Typing("line".to_owned()));
    }

    #[test]
    fn cancelling_a_search_leaves_no_trace() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::SearchStart);
        app.apply(Action::SearchType('x'));
        app.apply(Action::SearchCancel);
        assert_eq!(*app.search(), Search::Off);
    }

    #[test]
    fn with_a_search_active_next_steps_through_matches_and_wraps() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::SearchStart);
        // Matches many lines.
        for c in "let line".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchCommit);

        let Search::Active { matches, .. } = app.search().clone() else {
            panic!("expected an active search, got {:?}", app.search());
        };
        assert!(matches.len() > 2, "{matches:?}");

        let first = app.scroll();
        app.apply(Action::NextChange);
        let Search::Active { at, .. } = app.search().clone() else {
            unreachable!()
        };
        assert_eq!(at, 1, "should have stepped to the second match");

        // All the way round.
        for _ in 1..matches.len() {
            app.apply(Action::NextChange);
        }
        assert_eq!(app.scroll(), first, "should have wrapped to the start");
    }

    #[test]
    fn changing_file_drops_the_search() {
        // Its match positions belong to the file it was run against.
        let mut app = app(vec![long(), long()], 10);
        app.apply(Action::SearchStart);
        for c in "line".chars() {
            app.apply(Action::SearchType(c));
        }
        app.apply(Action::SearchCommit);
        assert!(matches!(app.search(), Search::Active { .. }));

        app.apply(Action::NextFile);
        assert_eq!(*app.search(), Search::Off);
    }

    #[test]
    fn quit_while_typing_cancels_the_search_rather_than_the_program() {
        // `q` is a letter when a query is being typed.
        let mut app = app(vec![long()], 10);
        app.apply(Action::SearchStart);
        app.apply(Action::Quit);
        assert!(!app.should_quit());
        assert_eq!(*app.search(), Search::Off);
    }

    #[test]
    fn wrapping_can_be_changed_while_reading() {
        // A long line, in a narrow pane: wrapped it takes several visual rows,
        // truncated it takes one.
        let long_line = "word ".repeat(60);
        let mut app = app(
            vec![entry(
                &format!("{long_line}\n"),
                &format!("{long_line}x\n"),
                None,
            )],
            10,
        );
        let wrapped = app.layout().len();

        app.apply(Action::ToggleWrap);
        let truncated = app.layout().len();
        assert!(
            truncated < wrapped,
            "truncating should need fewer rows than wrapping: {truncated} vs {wrapped}"
        );

        app.apply(Action::ToggleWrap);
        assert_eq!(app.layout().len(), wrapped);
    }

    #[test]
    fn the_theme_cycles_and_the_settings_follow_it() {
        let mut app = app(vec![long()], 10);
        assert_eq!(app.render_options().palette_name(), "none");
        app.apply(Action::CycleTheme);
        assert_eq!(app.render_options().palette_name(), "dark");
        app.apply(Action::CycleTheme);
        assert_eq!(app.render_options().palette_name(), "ansi");
    }

    #[test]
    fn syntax_colour_can_be_turned_off_and_stops_being_computed() {
        let mut app = app(vec![long()], 10);
        // The test palette is `none`, which has no room for syntax colour
        // anyway — so nothing is owed.
        assert!(!app.needs_colour());

        app.apply(Action::CycleTheme);
        assert_eq!(app.render_options().palette_name(), "dark");

        app.apply(Action::ToggleSyntax);
        assert!(
            !app.needs_colour(),
            "colour was turned off but is still being computed"
        );
    }

    #[test]
    fn line_numbers_can_be_turned_off() {
        let mut app = app(vec![long()], 10);
        assert!(app.render_options().line_numbers);
        app.apply(Action::ToggleLineNumbers);
        assert!(!app.render_options().line_numbers);
    }

    #[test]
    fn ignoring_whitespace_at_runtime_re_diffs_the_files() {
        // A reindentation and nothing else: it is a change until whitespace
        // stops counting, and then it is not.
        let mut app = app(
            vec![entry(
                "fn main() {\nlet x = 1;\n}\n",
                "fn main() {\n    let x = 1;\n}\n",
                None,
            )],
            10,
        );
        assert!(
            app.document().is_some_and(DiffDocument::has_changes),
            "indentation should be a change to begin with"
        );

        app.apply(Action::CycleWhitespace);
        assert_eq!(app.diff_options().whitespace_name(), "ignore-change");
        assert!(
            !app.document().is_some_and(DiffDocument::has_changes),
            "with whitespace ignored there is nothing left"
        );
    }

    #[test]
    fn move_detection_can_be_turned_off_while_reading() {
        let helper =
            "fn helper(value: u32) -> u32 {\n    let doubled = value * 2;\n    doubled + 1\n}\n";
        let caller = "fn main() {\n    let answer = helper(1);\n    println!(\"hi\");\n}\n";
        let mut app = app(
            vec![entry(
                &format!("{helper}{caller}"),
                &format!("{caller}{helper}"),
                None,
            )],
            10,
        );
        assert!(app.document().is_some_and(|d| d.stats.moved > 0));

        app.apply(Action::ToggleMoves);
        assert!(app.document().is_some_and(|d| d.stats.moved == 0));
        assert!(app.document().is_some_and(|d| d.stats.added > 0));
    }

    #[test]
    fn a_patch_cannot_be_re_diffed_and_says_so() {
        // Entries built from a document alone have no files to diff again.
        let mut app = App::new(
            vec![Entry::new(
                compare(
                    &SourceFile::from_text("a/f.rs", "a\nb\n"),
                    &SourceFile::from_text("b/f.rs", "a\nc\n"),
                    &DiffOptions::default(),
                ),
                None,
                Highlighting::none(),
            )],
            options(),
        );
        app.set_viewport(100, 10);

        app.apply(Action::CycleWhitespace);
        assert!(
            app.notice().is_some_and(|note| note.contains("patch")),
            "should say the setting could not be applied: {:?}",
            app.notice()
        );
    }

    #[test]
    fn the_help_overlay_toggles() {
        let mut app = app(vec![long()], 10);
        assert!(!app.showing_help());
        app.apply(Action::ToggleHelp);
        assert!(app.showing_help());
        app.apply(Action::ToggleHelp);
        assert!(!app.showing_help());
    }

    #[test]
    fn focus_moves_between_the_panes_and_back() {
        let mut app = app(vec![long(), long()], 10);
        assert_eq!(app.focus(), Focus::Diff);
        app.apply(Action::ToggleFocus);
        assert_eq!(app.focus(), Focus::Files);
        app.apply(Action::ToggleFocus);
        assert_eq!(app.focus(), Focus::Diff);
    }

    #[test]
    fn the_file_list_cannot_be_focused_when_there_is_nothing_to_choose() {
        // One file: focusing the list would be a dead end where every key does
        // nothing and only another Tab gets you out.
        let mut app = app(vec![long()], 10);
        assert!(!app.can_choose_file());
        app.apply(Action::ToggleFocus);
        assert_eq!(app.focus(), Focus::Diff);
    }

    #[test]
    fn with_the_file_list_focused_the_movement_keys_choose_a_file() {
        let mut app = app(vec![long(), long(), long()], 10);
        app.apply(Action::ToggleFocus);

        app.apply(Action::Down);
        assert_eq!(app.selected(), 1);
        app.apply(Action::Down);
        assert_eq!(app.selected(), 2);
        app.apply(Action::Up);
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn choosing_a_file_does_not_scroll_the_diff_instead() {
        // The bug this replaces: the focus recoloured a border and nothing
        // else, so `j` scrolled the diff whichever pane you thought you were in.
        let mut app = app(vec![long(), long()], 10);
        app.apply(Action::Down);
        app.apply(Action::Down);
        let scrolled = app.scroll();
        assert!(scrolled > 0, "the diff should have scrolled while focused");

        app.apply(Action::ToggleFocus);
        app.apply(Action::Down);
        assert_eq!(app.selected(), 1, "the file should have changed");
        // A new file starts at the top, rather than inheriting the last one's
        // scroll position.
        assert_eq!(app.scroll(), 0);
    }

    #[test]
    fn the_file_list_selection_stops_at_both_ends() {
        let mut app = app(vec![long(), long()], 10);
        app.apply(Action::ToggleFocus);

        app.apply(Action::Up);
        assert_eq!(app.selected(), 0);
        for _ in 0..5 {
            app.apply(Action::Down);
        }
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn top_and_bottom_jump_to_the_first_and_last_file() {
        let mut app = app(vec![long(), long(), long(), long()], 10);
        app.apply(Action::ToggleFocus);

        app.apply(Action::Bottom);
        assert_eq!(app.selected(), 3);
        app.apply(Action::Top);
        assert_eq!(app.selected(), 0);
    }

    #[test]
    fn enter_leaves_the_file_list_for_the_file_it_chose() {
        let mut app = app(vec![long(), long()], 10);
        app.apply(Action::ToggleFocus);
        app.apply(Action::Down);
        app.apply(Action::OpenFile);

        assert_eq!(app.focus(), Focus::Diff);
        assert_eq!(app.selected(), 1, "the chosen file stays chosen");
        // …and the movement keys are the diff's again.
        app.apply(Action::Down);
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn the_bracket_keys_still_change_file_from_the_diff() {
        // Choosing from the list is the addition, not a replacement: moving
        // between files without leaving the diff is the faster path when you
        // are reading rather than looking for something.
        let mut app = app(vec![long(), long()], 10);
        assert_eq!(app.focus(), Focus::Diff);
        app.apply(Action::NextFile);
        assert_eq!(app.selected(), 1);
        assert_eq!(app.focus(), Focus::Diff);
    }

    #[test]
    fn a_narrower_terminal_relays_out_and_keeps_the_scroll_valid() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::Bottom);

        // Half the width: lines wrap, so there are more rows, not fewer.
        app.set_viewport(50, 10);
        assert!(app.scroll() <= app.max_scroll());
    }

    #[test]
    fn a_shorter_terminal_pulls_the_scroll_back_into_range() {
        let mut app = app(vec![long()], 10);
        app.apply(Action::Bottom);
        let deep = app.scroll();

        app.set_viewport(100, 40);
        assert!(app.scroll() <= app.max_scroll());
        assert!(app.scroll() <= deep);
    }

    #[test]
    fn an_empty_browser_does_not_panic_on_any_action() {
        let mut app = app(Vec::new(), 10);
        assert!(app.is_empty());
        for action in [
            Action::Down,
            Action::NextChange,
            Action::NextFile,
            Action::ToggleFold,
            Action::SearchStart,
            Action::SearchCommit,
            Action::Bottom,
        ] {
            app.apply(action);
        }
        assert_eq!(app.scroll(), 0);
    }
}
