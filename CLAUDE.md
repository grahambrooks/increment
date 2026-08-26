# Graham's Diff (`gdiff`)

A command line and terminal UI Rust diff visualizer that presents diffs in a more human-readable
way. It highlights changes, additions and deletions clearly, making it easier to understand the
differences between two files or sets of code.

The reference target is the JetBrains diff view (`JetBrainsDiff.png`): two syntax-highlighted
panes side by side, a shared line-number gutter, corresponding lines aligned across the panes,
block colour by change kind, word-level highlighting inside modified lines, unchanged regions
folded, and a change map down the right edge.

## Design documents

Read these before writing code — the decisions are made there, not re-derived per session.

- `design/001-visual-diff-state-of-the-art.md` — survey of the field (delta, difftastic, split
  renderers, diff algorithms, highlighting stacks) and the options considered for this project,
  with a recommendation for each.
- `design/002-architecture-and-plan.md` — module layout, dependency choices, the phased plan and
  its per-phase "done when" criteria.

## Positioning

Not another styled pager — `delta` owns that. Not a structural differ — `difftastic` owns that.
gdiff is the **aligned split view in the terminal**: correspondence between the two sides made
visible, with folding and a whole-file change map.

## Standing constraints

- **Single binary crate**, directory modules under `src/`. No Cargo workspace.
- **Pure Rust.** No C toolchain, no runtime dependency on `git`, `diff` or any external binary.
  This is why `syntect` is taken with `default-features = false, features = ["regex-fancy"]`
  (the default pulls in the `onig` C library) and why git access goes through `gix`.
- **`model/` and `diff/` know nothing about terminals, colour or width.** Renderers consume an
  already-computed `Vec<Row>`. This is what keeps the alignment logic testable without a screen.
- **Structured stdout is the default surface; the TUI is an explicit request.** `--ui auto` must
  never resolve to `tui` — an alternate screen cannot be piped, redirected or read by CI, and
  `auto` is what CI hits. Redirecting the TUI exits 2 with a message rather than emitting escape
  codes into a file.
- **One JSON serializer in the core**, so every surface renders from one function.
- **Colour is never the only channel**, and colour self-strips when piped (`anstream`).
- **All renderers share one fixture document** so they cannot drift on what a diff contains.

## Deliberately out of scope

Recorded so a later session does not "complete" something that was cut on purpose: three-way
merge and conflict resolution, editing (gdiff is a viewer), a GUI, a directory-tree review UI in
v1, an in-process plugin API, and automatic crates.io publishing.

## Workflow

- Trunk-based: commit and push directly to `main`. PRs are not required.
- `prek` pre-commit gate — fast checks on changed files at commit, whole repo at push. Never put
  an unreliable check in the hook.
- CI: one `ubuntu-latest` job — build, test, `clippy -D warnings`, `fmt --check`.
- Releases: CalVer, GitHub Actions builds the binaries, Homebrew formula lives in this repo.
  crates.io publishing stays behind a manual workflow trigger.
- Verify against the real artifact, not the local build.
