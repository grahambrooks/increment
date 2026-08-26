# gdiff

A diff visualizer for the terminal: the **aligned side-by-side view**, rendered
as structured output.

Two syntax-highlighted panes, a shared line-number gutter, corresponding lines
level with each other, block colour by change kind, word-level highlighting
inside modified lines, unchanged regions folded, and a change map down the
right edge — the JetBrains diff view, in a terminal, in a pipe.

## Status

**Phase 0 — scaffold.** The layering, the gate and the release shape are in
place; nothing is diffed yet. `gdiff a.rs b.rs` says so and exits 2 rather than
printing an empty diff. The plan and its per-phase completion criteria are in
[`design/002-architecture-and-plan.md`](design/002-architecture-and-plan.md).

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
