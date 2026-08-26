//! Process exit codes.
//!
//! The convention is `diff(1)`'s, because that is what anything wrapping a diff
//! tool already expects: 0 means the inputs are identical, 1 means they differ,
//! and 2 means the tool could not do its job. A script that treats "differs" as
//! a failure is already broken against `diff` and `git diff --exit-code`; one
//! that treats *trouble* as "differs" would silently pass a broken build.

/// The inputs are identical.
pub const SUCCESS: i32 = 0;

/// The inputs differ. Not an error.
pub const DIFFERENCES: i32 = 1;

/// Trouble: unreadable input, an unusable terminal, a malformed patch.
///
/// This is also what a TUI request exits with when the output is redirected —
/// see [`crate::cli::surface`].
pub const TROUBLE: i32 = 2;
