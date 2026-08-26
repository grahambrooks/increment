# 001 — Visual diff: state of the art and options for gdiff

Status: draft for review · Date: 2026-08-26 · Supersedes: nothing

The reference target is `JetBrains diff` (see `../JetBrainsDiff.png`): two syntax-highlighted
panes, a shared line-number gutter, changed regions aligned across the panes, block colour by
change kind, word-level highlight inside modified lines, unchanged regions collapsed, and a
change map down the right edge.

This document surveys what exists, extracts the techniques that actually make a diff readable,
and lists the options open to gdiff with a recommendation for each. The plan that follows from
it is `002-architecture-and-plan.md`.

---

## 1. What the field looks like in 2026

### 1.1 Terminal diff renderers (line-based, the mainstream)

| Tool | Language | What it contributes |
|---|---|---|
| **delta** | Rust | The bar to clear. Syntax highlighting via syntect, side-by-side mode with line numbers in both panes, word-level highlight, hyperlinks, blame and grep decoration, 20+ themes. Works as `core.pager`, so it re-parses git's *unified* output rather than re-diffing content. |
| **diff-so-fancy** | Perl | Popularised removing the `+`/`-` noise and highlighting the changed substring rather than the whole line. |
| **riff** | Rust | Focused entirely on intra-line highlighting quality — which characters actually changed. |
| **git-split-diffs**, **dunk**, **icdiff** | TS / Python / Python | Split (side-by-side) rendering in a terminal, i.e. the layout gdiff wants. All three show the same two hard problems: what to do when the pane is too narrow, and how to align the two sides. |
| **diffnav**, **critique**, **drft** | Go / Rust | The next layer up: a *browser* — file tree plus diff pane, i.e. review as a TUI rather than a pager. |

Takeaway: the styled-pager niche is solved and crowded. The *side-by-side with real alignment*
niche is not — split renderers exist but none reproduces the JetBrains alignment, folding and
change-map together.

### 1.2 Structural / syntactic diff

- **difftastic** (Rust) parses both sides with tree-sitter (50+ grammars) and diffs the syntax
  trees, so reformatting is not a change and moved expressions are reported as moves. It falls
  back to line diff when a file is too large or has no grammar.
- **diffsitter** takes the cheaper route: tree-sitter parse, then LCS over the *leaves*.

Takeaway: structural diff is real and shipping, but note what it costs — difftastic's own output
is not a patch, cannot be fed to `git apply`, and is much harder to lay out in two aligned
columns because tree nodes do not respect line boundaries. It is a mode, not a foundation.

### 1.3 Diff algorithms

Four are in practical use: **Myers** (git default), **minimal**, **patience**, and **histogram**
(patience extended to low-occurrence common elements). Git's own documentation says histogram
produces more readable output than Myers for large diffs, function moves and refactors.
`--color-moved=zebra` layers block-move detection on top of any of them.

In Rust: **`imara-diff` 0.2** implements histogram and Myers, is generic over an interned token
type (so it diffs lines *or* words with one engine), and benchmarks well ahead of `similar` on
real corpora (Linux kernel, rustc, VS Code). **`similar` 3.2** is the more ergonomic library —
Myers/patience/LCS, unicode word and grapheme splitting, inline highlighting helpers — but its
engine is the slower one.

### 1.4 Syntax highlighting

- **syntect 5.3** — Sublime Text syntax definitions, the engine behind `bat` and `delta`. With
  **`two-face` 0.5** you inherit bat's whole curated syntax and theme set. Caveat: syntect's
  default regex engine is `onig`, a **C library**; `default-features = false` plus the
  `regex-fancy` feature keeps it pure Rust.
- **tree-sitter-highlight 0.26** — accurate and context-aware, but needs a grammar crate plus
  query files per language, and the grammars are C.

### 1.5 What the research and the tools agree makes a diff readable

1. **Anchor on rare lines** (patience/histogram) rather than the shortest edit script — the
   minimal diff is frequently the least readable one.
2. **Show the changed *substring***, not the changed line. This is the single highest-value
   feature in every tool above.
3. **Align the two sides.** A split view where the panes scroll independently is barely better
   than two files open side by side; the value is in corresponding lines being level.
4. **Suppress the unchanged.** Folding, not just `-U3`, with the fold expandable.
5. **Distinguish move from delete+add.** A moved function is one change, not two.
6. **Give a whole-file overview** — the change map/minimap. Diff tools that only show a viewport
   lose the sense of how much changed and where.
7. **Never let colour be the only channel**, and never emit escape codes into a pipe.

---

## 2. Where gdiff fits

**Positioning: the JetBrains split view, in the terminal, as structured output.**

Not another styled pager (delta owns that), not a structural differ (difftastic owns that). The
gap is a diff that *aligns*: correspondence between the two sides made visible, with folding and
a change map, rendered as ordinary stdout that survives a pipe, plus an explicit interactive
browser for when you want to navigate rather than read.

---

## 3. Options and recommendations

### O1 — Diff engine

| Option | Trade-off |
|---|---|
| A. `similar` for everything | Best ergonomics, unicode word/grapheme splitting built in, slowest engine. |
| B. `imara-diff` for everything | Fastest; histogram by default; generic over interned tokens, so the *same* engine does line diff and word diff — we supply the tokenizer. |
| C. Own implementation | No. Solved problem, and the heuristics are where the value is. |

**Recommend B.** One engine, histogram default, `--algorithm myers|histogram` exposed. Tokenize
lines for the outer diff and unicode word boundaries for the inner diff, both through the same
interner. Keep the engine behind an internal `diff::Engine` trait so A remains available if
imara's token model gets awkward for word diff.

### O2 — Textual vs structural

| Option | Trade-off |
|---|---|
| A. Line/word textual only | Predictable, alignable, patch-compatible, every file type works. |
| B. Structural first (tree-sitter) | Superior semantics, but no grammar → no diff, poor fit for column alignment, heavy C dependencies against the pure-Rust rule. |
| C. Textual core, structural as an opt-in mode later | Keeps the layout engine line-oriented; structural results are *projected back onto lines* for display. |

**Recommend C**, with A shipping first. Structural is a phase-5 `--structural` flag, gated on
whether it can be made to respect the two-column layout. Record it as deliberately deferred, not
forgotten.

### O3 — Syntax highlighting

**Recommend syntect + two-face, `default-features = false, features = ["regex-fancy"]`** — one
dependency covers ~200 languages and bat's themes with no C toolchain. tree-sitter arrives only
if and when O2-C does, where its parse is needed anyway. Highlighting must be *lazy per visible
region and cached*, and must degrade to plain text rather than failing.

### O4 — Alignment: the core of the product

| Option | Trade-off |
|---|---|
| A. Stack hunks, panes independent | Trivial; loses the whole point. |
| B. Row alignment with filler rows | GitHub/JetBrains split model: each display row holds ≤1 left line and ≤1 right line; deletions pad the right, insertions pad the left. |
| C. B + intra-block line pairing | Inside a change block, pair old and new lines by similarity (e.g. ≥ 0.5 token overlap); paired lines render as *modified* (blue) with word-level highlight, unpaired as pure add/delete. |

**Recommend C.** This is precisely what produces the blue "modified" blocks with inline
highlighting in the reference screenshot, and it is where a naive split renderer visibly fails.
The alignment result is a `Vec<Row>` — a pure data structure, computed with no terminal
involved, and unit-testable on its own.

### O5 — Narrow terminals

Side-by-side halves the usable width; the reference screenshot is truncating.

**Recommend:** wrap by default with a continuation glyph in the gutter, `--wrap=off` to truncate,
and an automatic fallback to the unified renderer below `--min-split-width` (default 120
columns). In the TUI, horizontal scroll instead. Column arithmetic goes through
`unicode-width` — CJK, emoji and tabs must not shear the panes.

### O6 — Output surface

**Recommend structured stdout as the default, TUI as an explicit request.** Colour via
`anstream` so it self-strips when piped. `--ui auto` must **never** resolve to `tui`: an
alternate screen cannot be piped, redirected or read by CI, and `auto` is what CI hits.
Redirecting the TUI exits 2 with a message rather than dumping escape codes into a file. A
`--format json` serializer lives in the core library so every surface renders from one function.

### O7 — Input

| Option | Trade-off |
|---|---|
| A. Two file paths | Obvious, needed for `git difftool` and standalone use. |
| B. Pager mode: parse unified diff on stdin | The adoption path (`core.pager`), but you only ever see the context git chose — no folding, no whole-file change map, degraded word diff. |
| C. Native git via `gix` (pure Rust) | Reads both blobs in full, so folding, the change map and re-diffing all work properly. |

**Recommend A first, then C, with B as a compatibility mode** that is honest about its limits.
The full-content path is what makes the JetBrains-style view possible at all.

### O8 — Move detection

`--color-moved=zebra`-style block move detection, alternating tints so adjacent moved blocks stay
distinguishable. **Recommend phase 5**, after alignment and folding are solid.

### O9 — Colour and accessibility

**Recommend:** semantic tokens (`added`/`removed`/`modified`/`moved`/`emphasis`) resolved through
a theme, truecolor with a 256- and 16-colour fallback, a colourblind-safe theme, and a
`--marker` mode that carries change kind in glyphs so colour is never the only channel.

---

## 4. Deliberately not building

Recorded so a later session does not "complete" them: three-way merge and conflict resolution,
editing (gdiff is a viewer), a GUI, a directory-tree review UI in v1, an in-process plugin API,
and automatic crates.io publishing.

---

## 5. Sources

- [difftastic](https://github.com/Wilfred/difftastic) · [manual: tree diffing](https://difftastic.wilfred.me.uk/tree_diffing.html) · [line-based diffs](https://github.com/Wilfred/difftastic/wiki/Line-Based-Diffs)
- [git-delta](https://crates.io/crates/git-delta) · [delta docs](https://www.terminal.guide/tools/git-tool/git-delta/)
- [imara-diff](https://github.com/pascalkuthe/imara-diff) · [announcement and benchmarks](https://users.rust-lang.org/t/announcing-imara-diff-a-reliably-performant-diffing-library-for-rust/83276)
- [git diff-options (`--diff-algorithm`, `--color-moved`)](https://git-scm.com/docs/diff-options/2.6.7) · [algorithm comparison](https://toolpage.dev/guides/how-diff-algorithms-work/)
- [syntect](https://github.com/trishume/syntect/) · [tree-sitter-highlight](https://crates.io/crates/tree-sitter-highlight)
- [awesome-diff-tools](https://github.com/mmueller2012/awesome-diff-tools)
