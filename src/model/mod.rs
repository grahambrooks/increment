//! The diff document: files, changes, and the aligned rows a renderer draws.
//!
//! Pure data. No I/O, no terminal, no colour — serde-serializable end to end,
//! because `--format json` is this module's `Display`.
//!
//! Phase 1 fills this in: `file` (source text, line index, encoding and EOL
//! handling), `change` (`Equal` / `Added` / `Removed` / `Modified` with inline
//! spans, grouped into blocks) and `row` (a display row holding at most one
//! left line and at most one right line, which is how the two panes stay
//! aligned).
