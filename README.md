# gdiff

A diff visualizer for the terminal: the **aligned side-by-side view**, rendered
as structured output.

Two syntax-highlighted panes, a shared line-number gutter, corresponding lines
level with each other, block colour by change kind, word-level highlighting
inside modified lines, unchanged regions folded, and a change map down the
right edge — the JetBrains diff view, in a terminal, in a pipe.

## Status

**Phases 0–5 done.** gdiff diffs files or a git repository and renders them
side by side — as styled stdout, as JSON, or in an interactive browser — with
alignment, word-level highlighting, folding, syntax colour and move detection.
The per-phase criteria and what was deliberately not built are in
[`design/002-architecture-and-plan.md`](design/002-architecture-and-plan.md).

A block that moved is shown as a move rather than as a wall of deletions and an
equal wall of additions somewhere else — `<` where it left, `>` where it
arrived. Detection is within a file; a block moved between files is not caught.

```sh
gdiff old.rs new.rs              # two files; splits if the terminal is wide enough
gdiff git                        # HEAD against the working tree
gdiff git HEAD~2                 # a revision against the working tree
gdiff git main..feature          # one revision against another
gdiff git HEAD~1..HEAD src       # …restricted to a path
gdiff --format json old.rs new.rs
```

Exit codes follow `diff(1)`, so `gdiff git` in a script says whether anything
changed.

## Reviewing a branch

```sh
gdiff review                     # the current branch
gdiff review main..feature       # a range
gdiff review --limit 500 HEAD    # …further back
```

A commit list with the working tree at the top of it; `Enter` opens what that
commit did in the split view below, and moving the selection with the split open
follows it. `q` closes a view and returns to the list; only the list itself
quits. `Q` quits from anywhere.

The history is walked in the background, so a long branch shows its first screen
immediately rather than after the walk. The diff loads when you stop moving, so
holding `j` down the log stays instant.

| Key | |
|---|---|
| `↵` | open the selected commit |
| `j` `k` | move |
| `Tab` | commits → the commit's file list → the diff → commits |
| `q` | back — or quit, from the list |
| `Q` | quit |

Inside the diff, every key from the browser below still works, and the file list
keeps the focus as you move between commits — so you can work down a branch
looking at one file's history without re-selecting it each time.

## The browser

```sh
gdiff --ui tui git
```

A file list, the split view, and a change map down the right edge showing what
changed across the whole file and where you are in it.

| Key | |
|---|---|
| `j` `k`, `↓` `↑` | scroll |
| `Ctrl-f` `Ctrl-b`, `PgDn` `PgUp` | page |
| `g` `G`, `Home` `End` | top, bottom |
| `n` `N` | next, previous change — or search match, when a search is active |
| `]` `[` | next, previous file, without leaving the diff |
| `f` | unfold, showing every line rather than the context |
| `/` | search; `Enter` commits, `Esc` cancels |
| `Tab` | move to the file list, then back to the diff |
| `q`, `Esc`, `Ctrl-c` | quit |

With the file list focused, `j` `k` (and `g` `G`, `PgUp` `PgDn`) choose a file
and `Enter` returns to the diff to read it. The list is skipped when only one
file changed, since there would be nothing to choose.

The browser is always an explicit request. `--ui auto` never selects it: an
alternate screen cannot be piped, redirected or read by CI, and `auto` is what
CI hits. Redirecting it exits 2 with a message rather than writing escape codes
into a file.

## Git

gdiff reads the repository directly, in process — it never shells out to `git`,
and there is no runtime dependency on it.

### As a difftool

```sh
git config --global difftool.gdiff.cmd 'gdiff "$LOCAL" "$REMOTE"'
git config --global difftool.prompt false
git difftool -y HEAD~1
```

### As a pager

```sh
git config --global core.pager 'gdiff --patch'
```

**This path is deliberately lower fidelity, and it is worth knowing why.** A
pager is handed the diff git already decided to print — a few lines of context
around each change and nothing else. The files themselves are not available, so
there is no whole-file view, nothing to fold that git has not already folded,
and no way to re-diff a region with different settings. gdiff still re-diffs
each hunk, so the pairing and the word-level highlighting are its own, and it
marks the gaps between hunks with what the hunk headers imply.

Where the choice exists, `gdiff git` is the better path: it reads both sides in
full.

| Flag | |
|---|---|
| `--view auto\|split\|unified` | `auto` splits at 120 columns or wider |
| `--format text\|json` | |
| `-U N`, `--full` | context lines either side of a change; `--full` folds nothing |
| `--wrap wrap\|truncate` | what to do with a line too wide for its pane |
| `--theme auto\|dark\|ansi\|none` | `dark` tints backgrounds, `ansi` uses the sixteen colours |
| `--syntax auto\|on\|off` | `auto` highlights only where the palette leaves the foreground free |
| `--color auto\|always\|never` | colour strips itself when piped regardless |
| `--algorithm histogram\|myers` | |
| `-w`, `--ignore-all-space` | whitespace does not count as a change |
| `-b`, `--ignore-space-change` | changes in the *amount* of whitespace do not count |
| `--no-moved` | report a moved block as a deletion and an addition |
| `--patch`, `-p` | read a unified diff from stdin instead of comparing files |
| `--width N`, `--min-split-width N` | |

## Why another diff tool

`delta` owns the styled pager. `difftastic` owns structural diff. Neither of
them, and none of the split renderers, reproduces the thing that makes a
JetBrains diff readable: **alignment** — correspondence between the two sides
made visible, with folding and a whole-file overview. That is the gap gdiff
fills. The survey behind that claim is
[`design/001-visual-diff-state-of-the-art.md`](design/001-visual-diff-state-of-the-art.md).

## Design

- Structured stdout is the default surface; the TUI is always an explicit
  request. `--ui auto` never resolves to `tui`, because an alternate screen
  cannot be piped, redirected or read by CI — and `auto` is what CI hits.
- `model` and `diff` know nothing about terminals, colour or width. Renderers
  consume already-computed rows. The alignment logic is testable without a
  screen.
- Pure Rust: no C toolchain, and no runtime dependency on `git`, `diff` or any
  other binary.

## Exit codes

Following `diff(1)`, so anything already wrapping a diff tool behaves:

| Code | Meaning |
|---|---|
| 0 | the inputs are identical |
| 1 | the inputs differ |
| 2 | trouble — unreadable input, an unusable terminal, a malformed patch |

## Development

```sh
make check    # fmt, clippy, tests — the pre-commit gate
make hooks    # install the prek hooks (fast checks at commit, tests at push)
make test
```

Trunk-based: commits go straight to `main`.

## Licence

MIT © 2026 Graham Brooks
