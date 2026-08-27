# 002 — Architecture and delivery plan

Status: draft for review · Date: 2026-08-26 · Depends on: `001-visual-diff-state-of-the-art.md`

## 1. Shape

Single binary crate, `inc`, directory modules under `src/` — no workspace. Pure Rust: no C
toolchain, no runtime dependency on git, diff or any external binary.

```
src/
  main.rs              # thin: parse args, dispatch, map errors to exit codes
  cli/                 # clap definitions, option resolution, ui/format selection
  model/               # the diff document — no I/O, no terminal, serde-serializable
    file.rs            #   source file, line index, encoding/EOL handling
    change.rs          #   Change{Equal,Added,Removed,Modified{spans}}, Hunk, Block
    row.rs             #   aligned display rows (left slot, right slot, kind)
  diff/                # engine layer
    engine.rs          #   trait + imara-diff histogram/myers impl
    tokens.rs          #   line and unicode-word tokenizers into one interner
    align.rs           #   O4-C: filler rows + intra-block similarity pairing
    inline.rs          #   word-level spans inside a Modified pair
    fold.rs            #   collapse runs of Equal beyond the context window
    moves.rs           #   (phase 5) block move detection
  highlight/           # syntect + two-face, lazy per region, cached, degrades to plain
  theme/               # semantic tokens -> styles; truecolor/256/16; colourblind theme
  render/
    unified.rs         # styled structured stdout, one column
    split.rs           # the JetBrains view: two panes, shared gutter, change map
    json.rs            # --format json, the single serializer every surface shares
    width.rs           # unicode-width column arithmetic, tabs, wrapping
  source/              # inputs
    files.rs           #   two paths
    git.rs             #   gix: blobs, worktree vs index vs revision
    patch.rs           #   pager mode: parse unified diff from stdin
  tui/                 # (phase 4) ratatui browser
    state.rs           #   navigation state machine — no terminal, unit-tested
    draw.rs            #   drawing only, snapshot-tested via TestBackend
```

**The load-bearing rule:** `model` and `diff` know nothing about terminals, colour or width.
Rendering consumes an already-computed `Vec<Row>`. That is what makes the alignment logic —
the hard part — testable without a screen, and what keeps the three surfaces (stdout, JSON, TUI)
from drifting.

## 2. Pipeline

```
source ──▶ FilePair ──▶ line diff (histogram) ──▶ blocks
                                                   │
                                    pair modified lines by similarity
                                                   │
                                        word diff inside pairs
                                                   │
                                    fold equal runs ──▶ Vec<Row>
                                                   │
                        ┌──────────────────────────┼──────────────────────────┐
                     unified                     split                      json
                                                   │
                                                  tui
```

## 3. Dependencies (versions current as of 2026-08-26)

| Crate | Version | For |
|---|---|---|
| `imara-diff` | 0.2 | line and word diff, histogram default |
| `syntect` | 5.3 | highlighting — `default-features = false, features = ["regex-fancy"]` (avoids the `onig` C library) |
| `two-face` | 0.5 | bat's syntax and theme assets — **also** `default-features = false, features = ["syntect-fancy"]`, or it drags `onig` back in |
| `anstream` / `anstyle` | 1.0 | colour that self-strips when piped |
| `unicode-width` | 0.2 | column arithmetic |
| `unicode-segmentation` | 1 | word tokenizer |
| `clap` | 4 | CLI |
| `serde` / `serde_json` | 1 | `--format json` |
| `terminal-size` | 0.4 | the terminal's width; `None` means "not a terminal", which is what stops the split view padding a pipe |
| `gix` | latest | phase 3 git integration, pure Rust |
| `ratatui` | 0.30 | phase 4 TUI |
| `insta` (dev) | 1 | snapshot tests for every renderer |

## 4. Phases

Each phase ends in a state that is shippable and verifiable from outside the code.

### Phase 0 — Scaffold — **done (2026-08-26), pending the remote**
Single crate, lib + bin; module skeleton carrying the architecture as doc comments; `Makefile`;
`prek` (fmt and clippy at commit, tests at push); GitHub Actions; CalVer; in-repo Homebrew
formula generated at release time; trunk-based, straight to `main`.

Two things went beyond the line above, both deliberate:

- **CI runs three platforms, not one `ubuntu-latest` job.** The portfolio default is a single
  Linux job, but increment's entire surface is terminal handling, unicode column arithmetic and line
  endings — CRLF, a Windows console and a macOS terminal each break assumptions Linux never
  surfaces. Windows CI has already earned its keep on brake and bx for exactly this reason.
- **The surface rule is code with tests from day one**, not a note for phase 4:
  `cli::surface::resolve` plus a CLI contract test asserting a redirected `--ui tui` exits 2 with
  a message. A constraint that only exists in prose is one a later phase gets to reinterpret.

**Done when:** CI is green on `main` with `cargo test`, `cargo clippy -- -D warnings` and
`cargo fmt --check` all passing. *Locally green; the GitHub remote does not exist yet, so the
CI half of this criterion is not met.*

### Phase 1 — Core model and unified renderer — **done (2026-08-26)**
Line diff via imara-diff; block classification; intra-block pairing; word-level spans; the
`Row` model; `render::unified`; `--format json`.

**Done when:** `inc a.rs b.rs` prints a styled unified diff with changed substrings
highlighted; `inc --format json a.rs b.rs` emits the same document as JSON; piping produces no
escape codes; insta snapshots cover the fixture set. *All met.*

Three things the work settled that the plan had not:

- **A fourth row kind, `Replaced`.** Two lines in the same position that are *not* an edit of
  each other. Stacking them — a removal above an addition — doubles the height of every replaced
  block and reads badly; both JetBrains and GitHub put them side by side. `Modified` claims a
  correspondence and carries inline emphasis; `Replaced` claims none and carries none, each side
  keeping its own colour. The markers `~` and `!` keep them apart without colour.
- **Similarity is weighted by word length.** Unweighted, `(`, `)` and `;` count for as much as an
  identifier, and `let b = 2;` pairs with `let inserted = 0;` at 0.6 — a confident and completely
  wrong set of highlights. Two tests pin the cases that drove the change.
- **The unified renderer's output is a real patch**, asserted by a test that applies it back to
  the old file and compares. It may interleave a `+` before a `-` within a hunk where the
  alignment ran that way; that is legal unified diff and applies correctly, but it is not the
  grouping `diff -u` emits.

### Phase 2 — The split view (the product) — **done (2026-08-26)**
`render::split`: two panes, shared centre gutter with both line numbers, filler rows, block
colour by kind, inline highlight on modified pairs, syntax highlighting both panes, folding of
unchanged runs with a fold summary row, wrapping and the narrow-terminal fallback.

**Done when:** rendering the reference sample side by side reproduces the structure in
`JetBrainsDiff.png` — aligned corresponding lines, add/remove/modify distinguished, inline
highlight inside modified lines, unchanged regions folded — verified by snapshot tests at 80,
120 and 200 columns, and by eye against the screenshot. *All met.*

Decisions taken during the work:

- **The change map is deferred to the TUI** — this answers open question 2 below. In a scrolling
  pager it restates the marker column already in the gutter; it only says something new when
  there is a viewport for it to sit outside of. Stdout gets a summary header instead:
  `old → new  +18 -14 ~1`.
- **Syntax highlighting composes with the diff, or it does not run.** Token colour goes on the
  *foreground*, change kind on the *background*, so the two coexist. `--syntax auto` therefore
  highlights only where the palette leaves the foreground free — the `dark` palette, not `ansi`,
  which has already spent colour saying what changed. `--syntax on` overrides.
- **`two-face` had to be pinned as well.** Taking `syntect` with `default-features = false` was
  not enough to keep the `onig` C library out: `two-face` depends on syntect *with* defaults and
  cargo unifies features across the graph. Both need `syntect-fancy`. `cargo tree -i onig`
  finding nothing is the check worth repeating.
- **Highlighting is a precomputed input to rendering, not part of the model.** A syntax parser is
  stateful, so colours have to come from the whole file — including the lines folding hides — and
  `model` may not know about colour. `Highlighting::of(&old, &new)` is built at the call site and
  handed to the renderer.

### Phase 3 — Git integration — **done (2026-08-26)**
`source::git` via `gix` (worktree/index/revision, `HEAD~1..HEAD`, path filters); `source::patch`
pager mode for `core.pager` compatibility, documented as reduced-fidelity; `git difftool` setup
instructions.

**Done when:** `inc git`, `inc git <rev>` and `git diff | inc --patch` all render, and the
README documents both the pager and difftool configurations. *All met.* Open question 3 is
answered by shipping the pager mode with its limits stated in both the README and the module.

What the work turned up:

- **The racy-index problem is real and had to be handled.** Comparing a revision against the
  working tree uses the index's stat data to avoid reading every tracked file — git's own
  optimisation. But an entry written in the same second as the index, at the same byte length, is
  indistinguishable from an untouched one by stat alone. Trusting it there makes an ordinary edit
  *silently invisible*: no output, exit 0, as though the file were clean. `Stat::is_racy` settles
  it, and a test pins the case. Three integration tests failed on this before it was fixed.
- **Git is the oracle for the git tests.** They build fixture repositories with the `git` binary
  and assert increment selects the same paths `git diff --name-only` does. The claim worth testing is
  not that the code runs but that it agrees with git. Note the asymmetry: git is a *test*
  dependency only — the product reads the repository in process via `gix`.
- **Binary files are named, never dropped.** `Binary file b/logo.png differs`, and they count
  towards the exit code. A diff tool that silently omits a change is lying by omission.
- **Every source produces the same shape** (`Changes`), so renderers never learn where a diff
  came from. The patch source is the exception that proves it: it produces *documents* rather
  than file pairs, because it never had the files — which is also why it cannot be syntax
  highlighted from source.
- **`gix` needed `sha1` naming explicitly.** Without it the build fails inside `gix-hash` with a
  `compile_error!`. Features chosen: `basic,revision,status,blob-diff,index,sha1`, no defaults —
  `cargo tree` confirms no C dependency (`zlib-rs` is a Rust implementation).

### Phase 4 — TUI browser — **done (2026-08-26)**
`ratatui`: changed-file list, split panes, next/previous change navigation, fold toggle, search,
and the change map — the piece deferred from phase 2, which earns its column here because only
part of the file is on screen.

**Done when:** the navigation state machine has unit tests with no terminal; layout has
`TestBackend` snapshots; `--ui auto` never resolves to `tui`; redirecting the TUI exits 2 with a
message; both renderers are driven from one shared fixture document. *All met* — and the last
one more strongly than asked: `render::split` was split into `compose` (layout, no I/O) and
`render` (serialisation), so the browser and the pager call the *same* layout function rather
than merely sharing a fixture. A test asserts their rows are identical.

Three bugs the work turned up, all of them navigation:

- **A jump near the end of a file appeared to stick.** The scroll clamps — there is nothing below
  the last screenful to show — so jumping to the final change leaves the scroll short of it.
  Searching for the *next* change from the scroll then found that same change again, and `n` did
  nothing. Fixed by tracking an anchor separate from the scroll: jumps count from where the
  reader was sent, manual movement puts the anchor back under their control.
- **`+0 -0` in the file list.** A file whose every change is an edit showed no additions and no
  removals, which reads as "nothing happened here". The modified count is shown too.
- **The change map marked everything.** The viewport indicator covered every cell when the file
  fitted on screen, which says nothing. It only draws when there is somewhere else to be.

And one interface wart: `inc --ui tui git` was read as the two-file form with `git` as the
first path. `args_conflicts_with_subcommands` was the cause; without it clap resolves the
subcommand from either position, and a test now covers both orders.

### Phase 5 — Semantics — **done (2026-08-26), except structural diff, which is declined**
Block move detection with alternating tints; whitespace-change modes.

**Done when:** a commit that moves a function shows it as a move rather than delete+add, with a
test asserting exactly that. *Met* — and checked against git: on the same pure move,
`git diff --color-moved=zebra` and increment mark the same four lines on each side.

- **Move detection runs before alignment, not after.** Alignment pairs unmatched lines by
  similarity, so by the time rows exist the moved block has already been paired off against
  whatever happened to sit opposite it. Detection therefore consumes engine *blocks* and
  alignment consults the result.
- **A run must be substantial**: three lines and twenty non-whitespace characters. Without a
  floor every `}` in the file "moves", and the marking is noise that buries the real ones.
- **A moved block that was also edited is not claimed whole**, which matches git — only lines
  that survived unchanged can be matched by content. Claiming the edited line too would hide a
  real change inside something the reader has been told to skip.
- **Within one file only.** A function moved to a *different* file is not detected, because each
  comparison is diffed on its own. Catching it means a pass over the whole change set before any
  of it is aligned — worth doing, not done here, and stated rather than implied.
- **Whitespace modes normalise what is compared, never what is shown** (`-w`, `-b`). A line still
  renders exactly as it is on disk; normalising for display would turn "your reformatting is
  hidden" into "increment lied about the file".
- **`+0 -0 ~0` was a bug.** A diff that is entirely a move reported no additions, no removals and
  no edits, which reads as nothing having happened. Move counts now appear in the split header,
  the browser's file list and its status line — the same failure the file list had in phase 4.

#### Structural diff: declined (2026-08-26)

**`--structural` via tree-sitter cannot be built without breaking the project's pure-Rust
constraint.** tree-sitter's core is C (`lib.c`, `stack.c`, `lexer.c`) and every grammar ships
`parser.c` and `scanner.c`; both compile through `cc`. Adding it makes `make pure-rust` fail by
design, and puts a C toolchain in the path of every build on every platform.

That is a constraint conflict rather than a scheduling problem. **Decided: ceded to difftastic.**
It is what difftastic is excellent at, increment's pitch is alignment rather than syntax-awareness,
and the two compose — `difft` for "what changed semantically", `inc` for "show me the two
versions". The alternatives considered and rejected: relaxing the constraint behind an
off-by-default cargo feature (then "pure Rust, no C toolchain" is true only of the default
features, and `make pure-rust` has to know the difference), and waiting for a pure-Rust parsing
stack with comparable language coverage (nothing today is close).

`--structural` is therefore not a planned flag, and O2 in `001` is settled at option A rather
than C.

### Release
CalVer (`2026.9.0`), GitHub Actions builds the binaries, Homebrew formula in this repo updated by
the release job, crates.io publishing left behind a manual workflow trigger.

**Done when:** a tagged release produces installable binaries and `brew install` from this repo's
formula works on this machine.

## 5. Testing strategy

- **Alignment is unit-tested as data** — `Vec<Row>` in, assertions on slots and kinds, no terminal.
- **One fixture document, every renderer.** Unified, split, JSON and TUI all render the same
  fixture so they cannot drift on what a diff contains.
- **Snapshot tests (`insta`) per renderer per width.**
- **Property test:** for any pair of inputs, every left line and every right line appears exactly
  once across the rows. Alignment must never drop or duplicate content — that is the one bug that
  would make the tool untrustworthy.
- No unreliable check goes into the pre-commit hook; a high-false-positive gate is how the hook
  gets disabled permanently.

## 6. Open questions for Graham

1. **Scope of v1** — is the split view against two files enough to call it v1, with git support
   in the following release, or should phase 3 land before the first tag?
2. ~~**The change map** — right-edge minimap column in stdout mode too, or TUI only?~~
   **Answered during phase 2: TUI only.** In a pager it restates the gutter.
3. ~~**Pager mode** — worth shipping given it structurally cannot do folding or the change map,
   or skip it and stand on `git difftool` plus `inc git`?~~ **Answered in phase 3: shipped,
   with its limits stated where someone configuring it will read them.** It re-diffs each hunk,
   so the pairing and emphasis are still increment's.
4. ~~**Structural diff** — a real goal for this project, or explicitly ceded to difftastic?~~
   **Answered 2026-08-26: ceded.** tree-sitter is C, so it cannot be added without breaking
   "pure Rust, no C toolchain". See the phase 5 note above.
