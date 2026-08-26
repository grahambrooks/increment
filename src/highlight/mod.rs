//! Syntax highlighting: `syntect` over bat's syntax and theme assets
//! (`two-face`), taken with `default-features = false` and the `regex-fancy`
//! feature so the `onig` C library stays out of the build.
//!
//! Two rules this module owes the renderers: highlight lazily, per visible
//! region, with the result cached — a 10k-line file must not be parsed to draw
//! 40 rows — and degrade to plain text rather than failing. An unknown language
//! is not an error.
