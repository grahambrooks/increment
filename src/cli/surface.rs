//! Choosing the output surface.
//!
//! One rule here is a project constraint rather than a preference, so it is
//! encoded as code with a test rather than left to a renderer to remember:
//!
//! **`auto` never resolves to the TUI.** An alternate screen cannot be piped,
//! redirected or read by CI, and `auto` is exactly what CI hits. Interactive
//! browsing is therefore always an explicit request. Asking for it and then
//! redirecting the output is an error — [`crate::exit::TROUBLE`] with a
//! message — not a licence to write escape codes into a file.

use std::fmt;

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Request {
    /// Let gdiff decide. Resolves to a stdout renderer, always.
    #[default]
    Auto,
    /// Styled structured stdout.
    Plain,
    /// The interactive browser. Only ever by explicit request.
    Tui,
}

/// What gdiff will actually do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Plain,
    Tui,
}

/// The TUI was requested somewhere it cannot run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotATerminal;

impl fmt::Display for NotATerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the interactive browser needs a terminal, and this output is redirected.\n\
             Drop `--ui tui` to render to stdout, or `--format json` for a machine reader."
        )
    }
}

impl std::error::Error for NotATerminal {}

/// Resolve a request against whether stdout is a terminal.
///
/// `stdout_is_terminal` is passed in rather than probed here so the rule is
/// testable without a tty.
pub fn resolve(request: Request, stdout_is_terminal: bool) -> Result<Surface, NotATerminal> {
    match request {
        // Deliberately not `if stdout_is_terminal { Tui }`. See the module docs:
        // a TTY is not consent, and CI often has one.
        Request::Auto => Ok(Surface::Plain),
        Request::Plain => Ok(Surface::Plain),
        Request::Tui if stdout_is_terminal => Ok(Surface::Tui),
        Request::Tui => Err(NotATerminal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_never_resolves_to_the_tui_even_on_a_terminal() {
        assert_eq!(resolve(Request::Auto, true), Ok(Surface::Plain));
        assert_eq!(resolve(Request::Auto, false), Ok(Surface::Plain));
    }

    #[test]
    fn plain_is_plain_anywhere() {
        assert_eq!(resolve(Request::Plain, true), Ok(Surface::Plain));
        assert_eq!(resolve(Request::Plain, false), Ok(Surface::Plain));
    }

    #[test]
    fn the_tui_runs_only_when_explicitly_asked_for_on_a_terminal() {
        assert_eq!(resolve(Request::Tui, true), Ok(Surface::Tui));
    }

    #[test]
    fn a_redirected_tui_is_an_error_not_a_silent_fallback() {
        // A fallback here would be worse than the error: the user asked to
        // browse, got a dump of something else, and nothing said so.
        assert_eq!(resolve(Request::Tui, false), Err(NotATerminal));
    }

    #[test]
    fn the_default_request_is_auto() {
        assert_eq!(Request::default(), Request::Auto);
    }
}
