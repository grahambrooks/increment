//! gdiff — the aligned side-by-side diff view, in the terminal, as structured
//! output.
//!
//! The layering is the load-bearing decision, and it is worth stating where
//! anyone will read it: [`model`] and [`diff`] know nothing about terminals,
//! colour or width. They compute a diff document and, from it, a sequence of
//! aligned display rows. Everything under [`render`] consumes that already
//! computed sequence.
//!
//! That is what keeps the alignment logic — the hard part, and the reason this
//! project exists — testable without a screen, and what stops the three
//! surfaces (styled stdout, JSON, TUI) from drifting on what a diff contains.
//!
//! See `design/002-architecture-and-plan.md` for the full architecture.

#[cfg(feature = "cli")]
pub mod cli;
pub mod diff;
pub mod exit;
pub mod highlight;
pub mod model;
pub mod render;
pub mod source;
pub mod theme;
pub mod tui;
