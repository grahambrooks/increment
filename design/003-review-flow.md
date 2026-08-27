# 003 — A tig-shaped review flow

Status: **6a–6d built (2026-08-26)** · Follows: `002-architecture-and-plan.md`

Phases 0–5 built a diff *visualiser*: point it at two files or a revision and it draws them well.
What it cannot do is the thing you actually sit down to do — **work through a branch, commit by
commit**. You have to know which revision you want before gdiff is any use. tig's answer to that
is a view model worth studying, and partly worth taking.

---

## 1. What tig actually does

- **One view at a time by default**, but the main and log views can *split* to show the commit
  diff underneath.
- **`Enter` on a commit** splits the view: log on top, that commit's diff below.
- **Cursor tracking**: moving in the parent view updates the child. Select a different commit and
  the diff below follows.
- **A view stack**: `q` closes the current view and returns to the previous one; `Q` quits
  everything. `Tab` moves between stacked views.
- **Views**: main, diff, log, tree, blob, blame, status, stage, refs, stash, grep, pager — each
  with its own keymap, falling back to a generic one.
- **Incremental loading**, with elapsed time shown in the title while a long view fills.

Sources: [tig manual](https://jonas.github.io/tig/doc/manual.html),
[tig(1)](https://jonas.github.io/tig/doc/tig.1.html), [tigrc(5)](https://jonas.github.io/tig/doc/tigrc.5.html).

## 2. What is worth taking, and what is not

The test applied here: *does this serve looking at a diff, or does it turn gdiff into a git
browser?* The second is a different product, and one that already exists.

### Take

| From tig | Why it fits gdiff |
|---|---|
| **A commit list, `Enter` to see its diff** | The missing half of the review loop. gdiff renders a commit's diff better than tig does; it just has no way to *choose* one. |
| **Parent/child split with cursor tracking** | Moving down the log and watching the diff follow is the review motion itself. |
| **View stack — `q` closes, `Q` quits** | gdiff's `q` currently quits outright, which is wrong the moment there is more than one view. |
| **Incremental loading** | A branch with 50k commits must not block the first frame. |

### Leave

| From tig | Why not |
|---|---|
| **Status and stage views** | Staging is *editing*. `CLAUDE.md` already lists editing as out of scope, and a diff viewer that half-stages is worse than one that does not. |
| **Tree and blob views** | A file browser. Nothing to do with showing a difference. |
| **Blame** | A different tool with a different data model. |
| **Refs, stash, grep** | Repository browsing. This is the line between "review a change" and "explore a repository", and it is the line gdiff should hold. |
| **Pager mode** | Already exists as `--patch`. |

**The resulting pitch stays intact:** gdiff is not a git browser with a diff view bolted on; it
is a diff viewer that can now be pointed at a commit without you naming it first.

## 3. What this costs

The browser today has one view — a file list beside a diff. A commit list makes that a *stack*,
and three things follow that are not free:

1. **`source::git::log`** — a revision walk (`repo.rev_walk`, already available under the
   `revision` feature) yielding id, summary, author and date.
2. **A view stack in `tui::state`** — `App` currently *is* the diff view. It becomes one view
   among several, with the key router asking the focused view first. This is the part that
   touches existing code.
3. **Loading off the draw path** — diffing a commit takes long enough to notice on a large one,
   and doing it inside `draw` would stutter the log as you scroll it.

Realistically this is phase 4 again in size.

## 4. Proposed phasing

### 6a — the log view — **done**
`gdiff review [<revrange>] [paths…] [--limit N]` opens a commit list. `Enter` opens that
commit's diff in the browser that already exists; `q` returns to the list, and only quits from
the list itself.

**Done when:** `gdiff review HEAD~20..HEAD` lists twenty commits, `Enter` shows one, `q` comes
back, and the navigation is unit-tested with no terminal, as in phase 4. *Met.*

### 6b — the split, with cursor tracking — **done**
Log above, diff below. Moving the selection updates the diff. `Tab` moves focus between them.

Two things the work settled:

- **The review knows nothing about git.** Commits arrive as data and their diffs through a
  `Loader` callback, so the whole flow is tested against fabricated commits with a fake loader —
  no repository, no terminal. The loader is also what makes 6c possible without touching this
  module.
- **Three status lines, three meanings for `q`.** Quit from a standalone browser, quit from the
  commit list (the last view open), back from a diff opened inside the review. Each line has to
  say its own, and a test asserts all three — a hint naming a key that does something else is
  worse than no hint.

#### The bug this found

Highlighting was **eager and per-file**, so opening a commit that changed twenty files parsed all
twenty to show one. Measured in release: 60–80ms per file, so about a second of dead terminal per
commit — and with 6b's cursor tracking, per press of the down arrow. In a debug build it looked
like a hang, which is how it was found.

`highlight`'s own module documentation said to highlight lazily and cache. The rule was written
and then broken. Entries now compute their highlighting the first time they are drawn, and
`colour_file` refuses outright above 20,000 lines — the parser cannot skip ahead, because line
400's colours depend on line 12, so the only lever is not starting.

This is also the first place the `regex-fancy` trade-off shows up in a number: the pure-Rust
engine is materially slower than the `onig` it was chosen over. Still the right call — a C
toolchain in every build costs more than 60ms a file — but worth knowing it is not free.

### 6c — scale — **done**
The history is walked on a background thread and appended as it arrives; the selected commit is
diffed when the reader *stops*, not on the keystroke.

Measured on a synthetic 20,000-commit repository:

| | before | after |
|---|---|---|
| first frame | 688 ms | **7 ms** |
| whole walk | 688 ms | 108 ms |

Two separate findings behind those numbers:

- **Streaming is what fixes the first frame.** 688 ms of blank terminal before anything appeared;
  now the first batch of 128 commits lands in single-digit milliseconds and the rest fill in
  behind it. The list title says `loading…` while they do, because a count that silently changes
  under the reader is worse than one that admits it is still coming.
- **`short_id()` was six sevenths of the walk.** gix computes git's shortest *unique*
  abbreviation, which means asking the object database about every commit — 688 ms against git's
  own 80 ms for the same history. A plain seven-character prefix brings it to 108 ms, comparable
  to git. The trade is that a prefix could in principle be ambiguous; it is a display label, and
  the full id is what anything actually resolves.
- **Diff loading is deferred rather than threaded.** Holding `j` down the log used to wait for a
  whole commit to be diffed on every repeat. It is now marked pending and done when no keypress
  is queued, so scrolling stays instant and the diff catches up on the pause. A thread would have
  meant making the loader and every `Entry` `Send`, for a problem that turned out to be about
  *when* rather than *where*.

### 6d — the working tree as a row — **done**
An "uncommitted changes" row at the top of the list, marked `•`, opening the same view `gdiff
git` shows. Present whenever no explicit range was named — reviewing "this branch" nearly always
means reviewing what is not committed yet as well.

The list is therefore `Item`s rather than commits, and the loader takes an `Item`. That is what
keeps the working tree from being a special case threaded through every function that touches the
list.

## 5. The decision, taken

6a changed what gdiff *is* — from "render this diff" to "review this branch". That was a
positioning change rather than a feature, and it was taken deliberately on 2026-08-26;
`CLAUDE.md`'s positioning section now says so. gdiff is still not a git browser: the line in §2
is what keeps it from becoming one, and it holds.

## 5a. Keyboard navigation for files (2026-08-27)

Tab has three stops rather than two: commits → the commit's file list → the diff. Two things
made that necessary, and the first was a defect rather than a gap.

- **`Focus::Files` was decorative.** Phase 4 gave the browser a file-list focus and drew a border
  for it, but the movement keys always scrolled the diff — the focus changed a colour and nothing
  else. With the list focused, `j`/`k`, `g`/`G` and the page keys now choose a file, and `Enter`
  returns to the diff to read it.
- **From a review the list could not be reached at all.** The file list belongs to the diff view,
  and the review's Tab went straight past it to the diff body, so the keys that drive it were
  unreachable no matter what they did.

The list is skipped when a commit touched one file: focusing something with nothing to choose is
a dead end where every key does nothing and only another Tab gets you out. The focus also carries
across commits, so working down a branch looking at one file's history does not mean re-selecting
it on every one.

The status line follows the focused pane — `file 2/7  j/k choose  ↵ open  Tab diff` — because
showing the diff's keys over a focused file list advertises keys that choose nothing there.

## 6. Still outstanding

- **6c — scale.** The log loads in full before the first frame, and the selected commit is diffed
  on the draw path. Neither is a problem at a few hundred commits; both will be at fifty
  thousand. Incremental loading with a progress indication, and diffing off the draw path.
- **6d — the working tree as a row.** An "uncommitted changes" entry at the top of the log,
  opening `gdiff git`. Small, and it is what makes the tool usable mid-work rather than only
  after committing.
