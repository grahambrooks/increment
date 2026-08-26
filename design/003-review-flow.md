# 003 — A tig-shaped review flow

Status: proposal, awaiting a decision · Date: 2026-08-26 · Follows: `002-architecture-and-plan.md`

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

### 6a — the log view (the whole idea, smallest version)
`gdiff review [<revrange>]` opens a commit list. `Enter` opens that commit's diff in the browser
that already exists; `q` returns to the list. No split, no tracking — one view at a time.

**Done when:** `gdiff review HEAD~20..HEAD` lists twenty commits, `Enter` shows one, `q` comes
back, and the navigation is unit-tested with no terminal, as in phase 4.

### 6b — the split, with cursor tracking
Log above, diff below. Moving the selection updates the diff. `Tab` moves focus between them.
This is what makes it feel like tig rather than like a menu.

### 6c — scale
Incremental log loading with a progress indication; diffing the selected commit off the draw
path; `--limit`.

### 6d — the working tree as a row
An "uncommitted changes" entry at the top of the log, opening `gdiff git`. Small, and it is what
makes the tool usable mid-work rather than only after committing.

## 5. The decision this needs

6a changes what gdiff *is* — from "render this diff" to "review this branch". That is a
positioning change, not just a feature, and `CLAUDE.md`'s "Positioning" section would need
rewriting to match. Worth doing if reviewing a branch is the job gdiff is for; not worth doing if
the job is being the thing `git difftool` and `core.pager` call.

Both are defensible. The question is which one gdiff is.
