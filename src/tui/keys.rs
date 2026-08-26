//! Key bindings.
//!
//! Separate from both the state machine and the drawer so the bindings can be
//! tested without inventing a terminal, and so that adding a key is a change to
//! one table rather than to a match arm buried in an event loop.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{Action, Search};

/// What a keypress means, given what the reader is in the middle of.
///
/// The context matters for exactly one reason: while a query is being typed,
/// letters are letters. `q` must not quit and `n` must not jump.
pub fn action(key: KeyEvent, search: &Search) -> Option<Action> {
    if matches!(search, Search::Typing(_)) {
        return match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::SearchType(c))
            }
            KeyCode::Backspace => Some(Action::SearchBackspace),
            KeyCode::Enter => Some(Action::SearchCommit),
            KeyCode::Esc => Some(Action::SearchCancel),
            _ => None,
        };
    }

    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, control) {
        (KeyCode::Char('c' | 'd'), true) => Some(Action::Quit),
        (KeyCode::Char('q'), false) | (KeyCode::Esc, _) => Some(Action::Quit),

        (KeyCode::Char('j') | KeyCode::Down, false) => Some(Action::Down),
        (KeyCode::Char('k') | KeyCode::Up, false) => Some(Action::Up),
        (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => Some(Action::PageDown),
        (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => Some(Action::PageUp),
        (KeyCode::Char('g') | KeyCode::Home, false) => Some(Action::Top),
        (KeyCode::Char('G') | KeyCode::End, false) => Some(Action::Bottom),

        (KeyCode::Char('n'), false) => Some(Action::NextChange),
        (KeyCode::Char('N'), false) => Some(Action::PreviousChange),
        (KeyCode::Char(']') | KeyCode::Char('J'), false) => Some(Action::NextFile),
        (KeyCode::Char('[') | KeyCode::Char('K'), false) => Some(Action::PreviousFile),

        (KeyCode::Char('f'), false) => Some(Action::ToggleFold),
        (KeyCode::Tab, _) => Some(Action::ToggleFocus),
        (KeyCode::Char('/'), false) => Some(Action::SearchStart),
        _ => None,
    }
}

/// The key hints for the status line, in the order they are shown.
pub const HINTS: &[(&str, &str)] = &[
    ("n/N", "change"),
    ("[/]", "file"),
    ("f", "fold"),
    ("/", "search"),
    ("q", "quit"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn typing() -> Search {
        Search::Typing("part".to_owned())
    }

    #[test]
    fn ordinary_keys_navigate() {
        assert_eq!(
            action(press(KeyCode::Char('j')), &Search::Off),
            Some(Action::Down)
        );
        assert_eq!(
            action(press(KeyCode::Down), &Search::Off),
            Some(Action::Down)
        );
        assert_eq!(
            action(press(KeyCode::Char('n')), &Search::Off),
            Some(Action::NextChange)
        );
        assert_eq!(
            action(press(KeyCode::Char('N')), &Search::Off),
            Some(Action::PreviousChange)
        );
        assert_eq!(
            action(press(KeyCode::Char('q')), &Search::Off),
            Some(Action::Quit)
        );
    }

    #[test]
    fn control_c_quits_from_anywhere_it_is_read() {
        assert_eq!(action(control('c'), &Search::Off), Some(Action::Quit));
    }

    /// The binding that would otherwise bite: a query containing `q` or `n`.
    #[test]
    fn while_typing_a_query_letters_are_letters() {
        assert_eq!(
            action(press(KeyCode::Char('q')), &typing()),
            Some(Action::SearchType('q'))
        );
        assert_eq!(
            action(press(KeyCode::Char('n')), &typing()),
            Some(Action::SearchType('n'))
        );
        assert_eq!(
            action(press(KeyCode::Char('/')), &typing()),
            Some(Action::SearchType('/'))
        );
    }

    #[test]
    fn while_typing_enter_commits_and_escape_cancels() {
        assert_eq!(
            action(press(KeyCode::Enter), &typing()),
            Some(Action::SearchCommit)
        );
        assert_eq!(
            action(press(KeyCode::Esc), &typing()),
            Some(Action::SearchCancel)
        );
        assert_eq!(
            action(press(KeyCode::Backspace), &typing()),
            Some(Action::SearchBackspace)
        );
    }

    #[test]
    fn while_typing_arrows_do_nothing_rather_than_something_surprising() {
        assert_eq!(action(press(KeyCode::Down), &typing()), None);
    }

    #[test]
    fn an_unbound_key_is_ignored() {
        assert_eq!(action(press(KeyCode::Char('z')), &Search::Off), None);
        assert_eq!(action(press(KeyCode::F(5)), &Search::Off), None);
    }

    #[test]
    fn every_hint_names_a_key_that_is_actually_bound() {
        // A status line advertising a key that does nothing is worse than no
        // status line.
        for (keys, what) in HINTS {
            let first = keys.chars().next().expect("a key");
            assert!(
                action(press(KeyCode::Char(first)), &Search::Off).is_some(),
                "the status line offers {keys:?} for {what}, but it is not bound"
            );
        }
    }
}
