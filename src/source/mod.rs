//! Where the two sides come from.
//!
//! - `files` — two paths.
//! - `git` — blobs, index and revisions via `gix`, in pure Rust. This is the
//!   path that reads both sides *in full*, which is what folding and the change
//!   map need in order to exist at all.
//! - `patch` — a unified diff on stdin, for `core.pager` compatibility. Honest
//!   about its limits: you only ever see the context git chose to emit, so
//!   folding and the change map cannot work from it.
