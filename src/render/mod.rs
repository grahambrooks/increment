//! The surfaces. Each one consumes already-computed rows; none of them diffs.
//!
//! - `unified` — styled structured stdout, one column. The default.
//! - `split` — the product: two panes, a shared centre gutter carrying both
//!   line numbers, block colour by change kind, inline highlighting on
//!   modified pairs, folded unchanged regions, and the change map.
//! - `json` — the single serializer every surface shares, so the CLI, the TUI
//!   and any machine reader render from one function rather than three.
//! - `width` — unicode column arithmetic, tab expansion and wrapping. CJK,
//!   emoji and tabs must not shear the panes, so no renderer counts `char`s.
