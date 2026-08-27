//! Reviewing a branch: a commit list, and the diff of whichever commit is
//! selected.
//!
//! The shape is tig's, and the reasoning for taking that shape — and for
//! stopping where it stops — is in `design/003-review-flow.md`. The short
//! version: a commit list with `Enter` to open its diff, a split with the
//! selection driving the diff below it, and a view stack where `q` closes a
//! view rather than the program. Not staging, not a tree browser, not blame.
//! Those would make gdiff a second git browser; it is a diff viewer that can
//! now be pointed at a commit without being told which one.
//!
//! Like [`super::state`], this knows nothing about terminals. It does not know
//! about git either: diffs arrive through a [`Loader`], so the whole flow is
//! testable against a handful of fabricated commits.

use crate::render::Options;
use crate::source::git::Commit;

use super::state::{Action, App, Entry};

/// A row of the review list.
///
/// The working tree is a row like any other, at the top. That is what makes the
/// tool usable in the middle of doing the work rather than only after
/// committing it — the state you most often want to look at is the one you have
/// not committed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Everything not yet committed: `HEAD` against the working tree.
    Worktree,
    Commit(Commit),
}

impl Item {
    /// What identifies this row to a loader.
    pub fn key(&self) -> &str {
        match self {
            // Not a valid object id, deliberately: nothing should try to
            // resolve it as one.
            Self::Worktree => "<worktree>",
            Self::Commit(commit) => &commit.id,
        }
    }

    pub fn short_id(&self) -> &str {
        match self {
            Self::Worktree => "•",
            Self::Commit(commit) => &commit.short_id,
        }
    }

    pub fn date(&self) -> &str {
        match self {
            Self::Worktree => "",
            Self::Commit(commit) => &commit.date,
        }
    }

    pub fn summary(&self) -> &str {
        match self {
            Self::Worktree => "uncommitted changes",
            Self::Commit(commit) => &commit.summary,
        }
    }

    pub fn is_worktree(&self) -> bool {
        matches!(self, Self::Worktree)
    }
}

impl From<Commit> for Item {
    fn from(commit: Commit) -> Self {
        Self::Commit(commit)
    }
}

/// Turns a row of the review list into the files it changed.
///
/// A callback rather than a call into `source::git`, so this module stays
/// ignorant of git and the tests stay free of repositories.
pub type Loader<'a> = Box<dyn FnMut(&Item) -> Result<Vec<Entry>, String> + 'a>;

/// Which pane the keys are talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Log,
    Diff,
}

/// How much is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The commit list, full screen.
    List,
    /// The list above, the selected commit's diff below.
    Split,
}

/// What a keypress means in the review flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Close the current view, or quit if it is the last one.
    Close,
    /// Quit outright, whatever is open.
    QuitAll,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    /// Open the selected commit, splitting the view.
    Open,
    ToggleFocus,
    /// Anything the diff view handles itself.
    Diff(Action),
}

pub struct Review<'a> {
    items: Vec<Item>,
    selected: usize,
    scroll: usize,
    /// Rows the commit list can show. Set by the drawer.
    height: usize,
    mode: Mode,
    focus: Pane,
    diff: Option<App>,
    /// Which commit `diff` holds, so re-selecting it costs nothing.
    loaded: Option<String>,
    loader: Loader<'a>,
    options: Options,
    notice: Option<String>,
    quit: bool,
    loading: bool,
    /// The selection moved and the diff has not caught up yet.
    ///
    /// Loading on the keystroke makes holding `j` unusable: every repeat waits
    /// for a whole commit to be diffed. The load is deferred until the reader
    /// stops moving — see [`Review::needs_settle`].
    pending: bool,
}

impl<'a> Review<'a> {
    pub fn new(items: Vec<Item>, loader: Loader<'a>, options: Options) -> Self {
        Self {
            items,
            selected: 0,
            scroll: 0,
            height: 1,
            mode: Mode::List,
            focus: Pane::Log,
            diff: None,
            loaded: None,
            loader,
            options,
            notice: None,
            quit: false,
            loading: false,
            pending: false,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Add commits found since the list was first shown.
    ///
    /// The selection is kept where it is: rows only ever arrive at the end, so
    /// appending must not move what the reader is looking at.
    pub fn extend(&mut self, more: impl IntoIterator<Item = Item>) {
        self.items.extend(more);
        self.follow();
    }

    /// Whether more rows are still arriving.
    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Say something in the status line.
    pub fn report(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn focus(&self) -> Pane {
        self.focus
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn diff(&self) -> Option<&App> {
        self.diff.as_ref()
    }

    pub fn diff_mut(&mut self) -> Option<&mut App> {
        self.diff.as_mut()
    }

    pub fn item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    /// Tell the review how tall the commit list is.
    pub fn set_log_viewport(&mut self, height: usize) {
        self.height = height.max(1);
        self.follow();
    }

    pub fn apply(&mut self, event: Event) {
        self.notice = None;

        match event {
            Event::QuitAll => self.quit = true,
            Event::Close => match self.mode {
                // `q` closes a view rather than the program, so a reader who
                // opened a commit gets back to the list they came from. Only
                // the last view quits.
                Mode::Split => {
                    self.mode = Mode::List;
                    self.focus = Pane::Log;
                }
                Mode::List => self.quit = true,
            },
            Event::Open => self.open(),
            Event::ToggleFocus => {
                self.focus = match (self.mode, self.focus) {
                    // Nothing to move to until something is open.
                    (Mode::List, _) => Pane::Log,
                    (Mode::Split, Pane::Log) => Pane::Diff,
                    (Mode::Split, Pane::Diff) => Pane::Log,
                };
            }
            Event::Up
            | Event::Down
            | Event::PageUp
            | Event::PageDown
            | Event::Top
            | Event::Bottom => self.move_by(&event),
            Event::Diff(action) => {
                if let Some(diff) = self.diff.as_mut() {
                    diff.apply(action);
                }
            }
        }
    }

    fn move_by(&mut self, event: &Event) {
        // With the diff focused these are its scrolling keys, not the list's.
        if self.focus == Pane::Diff {
            if let Some(diff) = self.diff.as_mut() {
                diff.apply(match event {
                    Event::Up => Action::Up,
                    Event::Down => Action::Down,
                    Event::PageUp => Action::PageUp,
                    Event::PageDown => Action::PageDown,
                    Event::Top => Action::Top,
                    _ => Action::Bottom,
                });
            }
            return;
        }

        let last = self.items.len().saturating_sub(1);
        self.selected = match event {
            Event::Up => self.selected.saturating_sub(1),
            Event::Down => (self.selected + 1).min(last),
            Event::PageUp => self.selected.saturating_sub(self.height),
            Event::PageDown => (self.selected + self.height).min(last),
            Event::Top => 0,
            _ => last,
        };
        self.follow();

        // Cursor tracking: with the split open, the diff follows the selection.
        // That is the review motion — move down the log and watch what each
        // commit did — and it is the difference between this and a menu.
        if self.mode == Mode::Split {
            self.pending = true;
        }
    }

    /// Whether a deferred diff load is waiting.
    ///
    /// The event loop calls this when no keypress is queued, so scrolling the
    /// log stays instant and the diff catches up the moment the reader pauses.
    pub fn needs_settle(&self) -> bool {
        self.pending
    }

    /// Do the deferred load.
    pub fn settle(&mut self) {
        if self.pending {
            self.load();
        }
    }

    fn open(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.load();
        if self.diff.is_some() {
            self.mode = Mode::Split;
            self.focus = Pane::Diff;
        }
    }

    /// Load the selected commit's diff, unless it is already loaded.
    fn load(&mut self) {
        self.pending = false;
        let Some(item) = self.items.get(self.selected).cloned() else {
            return;
        };
        if self.loaded.as_deref() == Some(item.key()) {
            return;
        }

        match (self.loader)(&item) {
            Ok(entries) => {
                self.loaded = Some(item.key().to_owned());
                let mut diff = App::new(entries, self.options);
                diff.nest();
                self.diff = Some(diff);
            }
            Err(error) => {
                // Keep whatever was on screen and say what went wrong. Blanking
                // the diff pane would leave the reader unable to tell a failure
                // from an empty commit.
                self.notice = Some(error);
            }
        }
    }

    /// Keep the selection on screen.
    fn follow(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.height {
            self.scroll = self.selected + 1 - self.height;
        }
        let max = self.items.len().saturating_sub(self.height);
        self.scroll = self.scroll.min(max);
    }

    /// The commits currently on screen.
    pub fn visible(&self) -> std::ops::Range<usize> {
        self.scroll..(self.scroll + self.height).min(self.items.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Options as DiffOptions, compare};
    use crate::highlight::Highlighting;
    use crate::model::SourceFile;
    use crate::theme::Theme;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn commit(n: usize) -> Commit {
        Commit {
            short_id: format!("{n:07x}"),
            id: format!("{n:040x}"),
            summary: format!("commit number {n}"),
            author: "Someone".to_owned(),
            date: "2026-08-26".to_owned(),
        }
    }

    fn commits(count: usize) -> Vec<Item> {
        (0..count).map(|n| Item::Commit(commit(n))).collect()
    }

    fn entry() -> Entry {
        let old = SourceFile::from_text("a/f.rs", "let x = 1;\n");
        let new = SourceFile::from_text("b/f.rs", "let x = 2;\n");
        Entry::new(
            compare(&old, &new, &DiffOptions::default()),
            None,
            Highlighting::none(),
        )
    }

    fn options() -> Options {
        Options {
            theme: Theme::none(),
            width: Some(100),
            ..Options::default()
        }
    }

    /// A review whose loader records which commits it was asked for.
    fn review(count: usize, height: usize) -> (Review<'static>, Rc<RefCell<Vec<String>>>) {
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorder = Rc::clone(&asked);
        let loader: Loader<'static> = Box::new(move |item: &Item| {
            recorder.borrow_mut().push(item.key().to_owned());
            Ok(vec![entry()])
        });

        let mut review = Review::new(commits(count), loader, options());
        review.set_log_viewport(height);
        (review, asked)
    }

    #[test]
    fn the_worktree_row_describes_itself_without_pretending_to_be_a_commit() {
        let worktree = Item::Worktree;
        assert!(worktree.is_worktree());
        assert_eq!(worktree.summary(), "uncommitted changes");
        // Not something anything should try to resolve as an object.
        assert!(!worktree.key().chars().all(|c| c.is_ascii_hexdigit()));
        assert!(worktree.date().is_empty());
    }

    #[test]
    fn the_worktree_row_loads_like_any_other() {
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorder = Rc::clone(&asked);
        let loader: Loader<'static> = Box::new(move |item: &Item| {
            recorder.borrow_mut().push(item.key().to_owned());
            Ok(vec![entry()])
        });

        let mut items = vec![Item::Worktree];
        items.extend(commits(3));
        let mut review = Review::new(items, loader, options());
        review.set_log_viewport(5);

        review.apply(Event::Open);
        assert_eq!(review.mode(), Mode::Split);
        assert_eq!(asked.borrow().as_slice(), [Item::Worktree.key().to_owned()]);
    }

    #[test]
    fn it_starts_on_the_list_with_nothing_loaded() {
        let (review, asked) = review(10, 5);
        assert_eq!(review.mode(), Mode::List);
        assert_eq!(review.focus(), Pane::Log);
        assert!(review.diff().is_none());
        assert!(asked.borrow().is_empty(), "nothing should load unasked");
    }

    #[test]
    fn opening_a_commit_splits_the_view_and_loads_it() {
        let (mut review, asked) = review(10, 5);
        review.apply(Event::Down);
        review.apply(Event::Open);

        assert_eq!(review.mode(), Mode::Split);
        assert_eq!(review.focus(), Pane::Diff);
        assert!(review.diff().is_some());
        assert_eq!(asked.borrow().as_slice(), [commit(1).id]);
    }

    #[test]
    fn close_returns_to_the_list_before_it_quits() {
        // tig's rule, and the reason `q` is not simply "quit": a reader who
        // opened a commit expects to get back to the list they came from.
        let (mut review, _) = review(10, 5);
        review.apply(Event::Open);
        assert_eq!(review.mode(), Mode::Split);

        review.apply(Event::Close);
        assert_eq!(review.mode(), Mode::List);
        assert!(!review.should_quit(), "the first close should not quit");

        review.apply(Event::Close);
        assert!(review.should_quit());
    }

    #[test]
    fn quit_all_quits_from_the_split() {
        let (mut review, _) = review(10, 5);
        review.apply(Event::Open);
        review.apply(Event::QuitAll);
        assert!(review.should_quit());
    }

    #[test]
    fn the_diff_follows_the_selection_once_the_split_is_open() {
        // The review motion: move down the log, watch what each commit did.
        let (mut review, asked) = review(10, 5);
        review.apply(Event::Open);
        review.apply(Event::ToggleFocus);
        assert_eq!(review.focus(), Pane::Log);

        // Each move defers the load; the event loop settles it when the reader
        // pauses. Both steps together are one "move and look".
        review.apply(Event::Down);
        review.settle();
        review.apply(Event::Down);
        review.settle();

        assert_eq!(
            asked.borrow().as_slice(),
            [commit(0).id, commit(1).id, commit(2).id]
        );
    }

    #[test]
    fn racing_down_the_log_does_not_diff_every_commit_on_the_way() {
        // Loading on the keystroke makes holding `j` unusable: every repeat
        // waits for a whole commit to be diffed. Only where the reader stops
        // is worth loading.
        let (mut review, asked) = review(20, 5);
        review.apply(Event::Open);
        review.apply(Event::ToggleFocus);
        asked.borrow_mut().clear();

        for _ in 0..10 {
            review.apply(Event::Down);
        }
        assert!(
            asked.borrow().is_empty(),
            "commits were diffed mid-scroll: {:?}",
            asked.borrow()
        );

        assert!(review.needs_settle());
        review.settle();
        assert_eq!(asked.borrow().as_slice(), [commit(10).id]);
        assert!(!review.needs_settle(), "settling should clear the debt");
    }

    #[test]
    fn rows_arriving_later_do_not_move_the_selection() {
        // A background walk appends while the reader is already looking at
        // something. Their place must not shift under them.
        let (mut review, _) = review(5, 5);
        review.apply(Event::Down);
        review.apply(Event::Down);
        let selected = review.selected();

        review.extend((100..140).map(|n| Item::Commit(commit(n))));

        assert_eq!(review.selected(), selected);
        assert_eq!(review.items().len(), 45);
    }

    #[test]
    fn moving_in_the_list_alone_loads_nothing() {
        // Before the split is open, scrolling the list is just scrolling. A
        // diff per keystroke on a large repository would make it unusable.
        let (mut review, asked) = review(10, 5);
        for _ in 0..5 {
            review.apply(Event::Down);
        }
        assert!(asked.borrow().is_empty(), "{:?}", asked.borrow());
    }

    #[test]
    fn re_selecting_the_same_commit_does_not_reload_it() {
        let (mut review, asked) = review(10, 5);
        review.apply(Event::Open);
        review.apply(Event::ToggleFocus);
        review.apply(Event::Down);
        review.settle();
        review.apply(Event::Up);
        review.settle();
        review.apply(Event::Down);
        review.settle();

        // 0, then 1, then 0, then 1 — but never the same one twice running.
        let asked = asked.borrow();
        assert!(
            asked.windows(2).all(|pair| pair[0] != pair[1]),
            "a commit was loaded twice in a row: {asked:?}"
        );
    }

    #[test]
    fn with_the_diff_focused_the_movement_keys_scroll_it() {
        let (mut review, asked) = review(10, 5);
        review.apply(Event::Open);
        assert_eq!(review.focus(), Pane::Diff);

        let before = review.selected();
        review.apply(Event::Down);
        assert_eq!(review.selected(), before, "the list selection moved");
        assert_eq!(asked.borrow().len(), 1, "scrolling the diff reloaded it");
    }

    #[test]
    fn the_selection_stays_on_screen() {
        let (mut review, _) = review(50, 5);
        review.apply(Event::Bottom);
        assert!(review.visible().contains(&review.selected()));

        review.apply(Event::Top);
        assert!(review.visible().contains(&review.selected()));
        assert_eq!(review.scroll(), 0);
    }

    #[test]
    fn the_selection_stops_at_both_ends() {
        let (mut review, _) = review(3, 5);
        review.apply(Event::Up);
        assert_eq!(review.selected(), 0);

        for _ in 0..10 {
            review.apply(Event::Down);
        }
        assert_eq!(review.selected(), 2);
    }

    #[test]
    fn focus_cannot_move_to_a_diff_that_is_not_open() {
        let (mut review, _) = review(10, 5);
        review.apply(Event::ToggleFocus);
        assert_eq!(review.focus(), Pane::Log);
    }

    #[test]
    fn a_loader_failure_says_so_and_keeps_what_was_on_screen() {
        let loader: Loader<'static> = Box::new(|item: &Item| {
            if item.key().ends_with('0') {
                Ok(vec![entry()])
            } else {
                Err("object 1234abc is missing".to_owned())
            }
        });
        let mut review = Review::new(commits(3), loader, options());
        review.set_log_viewport(5);

        review.apply(Event::Open);
        assert!(review.diff().is_some());

        review.apply(Event::ToggleFocus);
        review.apply(Event::Down);
        review.settle();
        assert!(
            review.notice().is_some_and(|note| note.contains("missing")),
            "the failure should be reported: {:?}",
            review.notice()
        );
        assert!(
            review.diff().is_some(),
            "blanking the pane would hide whether it failed or was empty"
        );
    }

    #[test]
    fn an_empty_history_does_not_panic_on_any_event() {
        let loader: Loader<'static> = Box::new(|_: &Item| Ok(vec![entry()]));
        let mut review = Review::new(Vec::new(), loader, options());
        review.set_log_viewport(5);

        for event in [
            Event::Down,
            Event::Open,
            Event::ToggleFocus,
            Event::Bottom,
            Event::PageDown,
        ] {
            review.apply(event);
        }
        assert_eq!(review.mode(), Mode::List, "nothing to open");
        assert!(review.diff().is_none());
    }
}
