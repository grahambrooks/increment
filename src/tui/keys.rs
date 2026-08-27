//! Key bindings.
//!
//! Separate from both the state machine and the drawer so the bindings can be
//! tested without inventing a terminal, and so that adding a key is a change to
//! one table rather than to a match arm buried in an event loop.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::review::{Event, Pane};
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
        // Settings, changeable while reading. None of these collide with the
        // navigation keys above, which is why they are the letters they are.
        (KeyCode::Char('w'), false) => Some(Action::ToggleWrap),
        (KeyCode::Char('s'), false) => Some(Action::ToggleSyntax),
        (KeyCode::Char('t'), false) => Some(Action::CycleTheme),
        (KeyCode::Char('#'), false) => Some(Action::ToggleLineNumbers),
        (KeyCode::Char('x'), false) => Some(Action::CycleWhitespace),
        (KeyCode::Char('m'), false) => Some(Action::ToggleMoves),
        (KeyCode::Char('?'), false) => Some(Action::ToggleHelp),
        // Meaningful only with the file list focused, where it says "this one";
        // in the diff it is unbound rather than doing something surprising.
        (KeyCode::Enter, _) => Some(Action::OpenFile),
        (KeyCode::Tab, _) => Some(Action::ToggleFocus),
        (KeyCode::Char('/'), false) => Some(Action::SearchStart),
        _ => None,
    }
}

/// What a keypress means in the review flow.
///
/// Two differences from the plain browser, both from tig: `q` closes the
/// current view rather than the program, and `Q` quits outright. A reader who
/// opened a commit expects `q` to put them back in the list they came from.
pub fn review_action(key: KeyEvent, focus: Pane, search: &Search) -> Option<Event> {
    // While a query is being typed every key belongs to the diff view, `q` and
    // `Q` included — they are letters.
    if matches!(search, Search::Typing(_)) {
        return action(key, search).map(Event::Diff);
    }

    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, control) {
        (KeyCode::Char('c' | 'd'), true) | (KeyCode::Char('Q'), false) => {
            return Some(Event::QuitAll);
        }
        (KeyCode::Tab, _) => return Some(Event::ToggleFocus),
        (KeyCode::Char('q'), false) | (KeyCode::Esc, _) => return Some(Event::Close),
        _ => {}
    }

    match focus {
        // The diff keeps every one of its own bindings.
        Pane::Diff => action(key, search).map(Event::Diff),
        Pane::Log => match (key.code, control) {
            (KeyCode::Char('j') | KeyCode::Down, false) => Some(Event::Down),
            (KeyCode::Char('k') | KeyCode::Up, false) => Some(Event::Up),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => Some(Event::PageDown),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => Some(Event::PageUp),
            (KeyCode::Char('g') | KeyCode::Home, false) => Some(Event::Top),
            (KeyCode::Char('G') | KeyCode::End, false) => Some(Event::Bottom),
            (KeyCode::Enter, _) => Some(Event::Open),
            _ => None,
        },
    }
}

/// The key hints for the review's commit list.
/// Shown only on the commit list, which is the last view open — so `q` there
/// really does quit, and saying "back" would be wrong.
pub const REVIEW_HINTS: &[(&str, &str)] = &[
    ("↵", "open"),
    ("j/k", "move"),
    ("Tab", "focus"),
    ("q", "quit"),
];

/// The key hints for the status line, in the order they are shown.
///
/// Deliberately short. The settings keys are not here — there are seven of
/// them, they would not fit, and `?` is what leads to them.
pub const HINTS: &[(&str, &str)] = &[
    ("n/N", "change"),
    ("[/]", "file"),
    ("f", "whole file"),
    ("/", "search"),
    ("?", "keys"),
    ("q", "quit"),
];

/// Everything `?` lists, grouped as it is drawn.
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "moving",
        &[
            ("j / k", "scroll, or choose a file"),
            ("Ctrl-f / Ctrl-b", "page"),
            ("g / G", "top, bottom"),
            ("n / N", "next, previous change — or search match"),
            ("[ / ]", "previous, next file"),
            ("Tab", "commits, file list, diff"),
            ("/", "search"),
        ],
    ),
    // These name the setting rather than describing it, because the panel
    // shows each one's current value beside it — the value says what the
    // choices are far better than a list of them in prose would.
    (
        "what is shown",
        &[("f", "showing"), ("x", "whitespace"), ("m", "moved blocks")],
    ),
    (
        "how it is drawn",
        &[
            ("w", "long lines"),
            ("s", "syntax colour"),
            ("t", "theme"),
            ("#", "line numbers"),
        ],
    ),
    ("leaving", &[("q", "back, or quit"), ("Q", "quit")]),
];

/// The keys that mean something with the file list focused.
///
/// A different set because the movement keys do a different thing there —
/// showing "n/N change" over a pane where `n` chooses nothing would be the same
/// lie as advertising a key that is not bound.
pub const FILE_HINTS: &[(&str, &str)] = &[("j/k", "choose"), ("↵", "open"), ("Tab", "diff")];

/// The same, for a diff opened from the review — where `q` goes back to the
/// commit list rather than quitting.
pub const NESTED_HINTS: &[(&str, &str)] = &[
    ("n/N", "change"),
    ("[/]", "file"),
    ("f", "fold"),
    ("/", "search"),
    ("Tab", "log"),
    ("q", "back"),
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
    fn in_the_review_q_closes_the_view_and_shift_q_quits() {
        // The difference from the plain browser, and the reason the review has
        // its own router: `q` must not drop the reader out of the program.
        assert_eq!(
            review_action(press(KeyCode::Char('q')), Pane::Diff, &Search::Off),
            Some(Event::Close)
        );
        assert_eq!(
            review_action(press(KeyCode::Char('Q')), Pane::Diff, &Search::Off),
            Some(Event::QuitAll)
        );
        // …whereas outside the review, `q` still quits.
        assert_eq!(
            action(press(KeyCode::Char('q')), &Search::Off),
            Some(Action::Quit)
        );
    }

    #[test]
    fn enter_opens_a_commit_from_the_log() {
        assert_eq!(
            review_action(press(KeyCode::Enter), Pane::Log, &Search::Off),
            Some(Event::Open)
        );
    }

    #[test]
    fn the_log_and_the_diff_read_the_movement_keys_differently() {
        assert_eq!(
            review_action(press(KeyCode::Char('j')), Pane::Log, &Search::Off),
            Some(Event::Down)
        );
        assert_eq!(
            review_action(press(KeyCode::Char('j')), Pane::Diff, &Search::Off),
            Some(Event::Diff(Action::Down))
        );
    }

    #[test]
    fn the_diff_keeps_its_own_bindings_inside_the_review() {
        assert_eq!(
            review_action(press(KeyCode::Char('n')), Pane::Diff, &Search::Off),
            Some(Event::Diff(Action::NextChange))
        );
        assert_eq!(
            review_action(press(KeyCode::Char('/')), Pane::Diff, &Search::Off),
            Some(Event::Diff(Action::SearchStart))
        );
    }

    #[test]
    fn while_typing_a_query_in_the_review_q_is_still_a_letter() {
        let typing = Search::Typing("q".to_owned());
        assert_eq!(
            review_action(press(KeyCode::Char('q')), Pane::Diff, &typing),
            Some(Event::Diff(Action::SearchType('q')))
        );
        assert_eq!(
            review_action(press(KeyCode::Char('Q')), Pane::Diff, &typing),
            Some(Event::Diff(Action::SearchType('Q')))
        );
    }

    #[test]
    fn every_file_list_hint_names_a_key_that_is_bound_there() {
        for (keys, what) in FILE_HINTS {
            let first = keys.chars().next().expect("a key");
            let code = match first {
                '↵' => KeyCode::Enter,
                'T' => KeyCode::Tab,
                c => KeyCode::Char(c),
            };
            assert!(
                action(press(code), &Search::Off).is_some(),
                "the file list offers {keys:?} for {what}, but it is not bound"
            );
        }
    }

    #[test]
    fn enter_chooses_the_file_the_list_is_on() {
        assert_eq!(
            action(press(KeyCode::Enter), &Search::Off),
            Some(Action::OpenFile)
        );
    }

    #[test]
    fn the_hints_say_what_q_does_in_each_place_it_is_shown() {
        // Three status lines, three meanings for `q`: quit from a standalone
        // browser, quit from the commit list (the last view open), and back
        // from a diff opened inside the review. Each line has to say its own.
        assert!(HINTS.iter().any(|(k, what)| *k == "q" && *what == "quit"));
        assert!(
            REVIEW_HINTS
                .iter()
                .any(|(k, what)| *k == "q" && *what == "quit")
        );
        assert!(
            NESTED_HINTS
                .iter()
                .any(|(k, what)| *k == "q" && *what == "back")
        );
    }

    #[test]
    fn the_nested_hints_describe_what_the_keys_do_inside_a_review() {
        // `q` quits from a standalone browser and goes back from a nested one.
        // Advertising "quit" in both places would be a lie in one of them.
        assert!(HINTS.iter().any(|(k, what)| *k == "q" && *what == "quit"));
        assert!(
            NESTED_HINTS
                .iter()
                .any(|(k, what)| *k == "q" && *what == "back")
        );

        for (keys, what) in NESTED_HINTS {
            let first = keys.chars().next().expect("a key");
            let code = if *keys == "Tab" {
                KeyCode::Tab
            } else {
                KeyCode::Char(first)
            };
            assert!(
                review_action(press(code), Pane::Diff, &Search::Off).is_some(),
                "the nested status line offers {keys:?} for {what}, but it is not bound"
            );
        }
    }

    #[test]
    fn every_review_hint_names_a_key_that_is_actually_bound() {
        for (keys, what) in REVIEW_HINTS {
            let first = keys.chars().next().expect("a key");
            let code = match first {
                '↵' => KeyCode::Enter,
                'T' => KeyCode::Tab,
                c => KeyCode::Char(c),
            };
            assert!(
                review_action(press(code), Pane::Log, &Search::Off).is_some(),
                "the review offers {keys:?} for {what}, but it is not bound"
            );
        }
    }

    #[test]
    fn every_key_the_help_lists_is_actually_bound() {
        // The help is the only place most of these appear, so an unbound entry
        // here is a promise nothing keeps.
        for (group, keys) in HELP {
            for (key, what) in *keys {
                let first = key.chars().next().expect("a key");
                let bound = match first {
                    'C' => true, // Ctrl-f / Ctrl-b, checked below.
                    'T' => action(press(KeyCode::Tab), &Search::Off).is_some(),
                    c => {
                        action(press(KeyCode::Char(c)), &Search::Off).is_some()
                            || review_action(press(KeyCode::Char(c)), Pane::Log, &Search::Off)
                                .is_some()
                    }
                };
                assert!(bound, "{group}: {key:?} ({what}) is not bound");
            }
        }
        assert_eq!(action(control('f'), &Search::Off), Some(Action::PageDown));
        assert_eq!(action(control('b'), &Search::Off), Some(Action::PageUp));
    }

    #[test]
    fn the_settings_keys_do_not_collide_with_the_navigation_keys() {
        // Every letter that changes a setting must not already move something.
        for (key, expected) in [
            ('w', Action::ToggleWrap),
            ('s', Action::ToggleSyntax),
            ('t', Action::CycleTheme),
            ('x', Action::CycleWhitespace),
            ('m', Action::ToggleMoves),
            ('?', Action::ToggleHelp),
            ('#', Action::ToggleLineNumbers),
        ] {
            assert_eq!(
                action(press(KeyCode::Char(key)), &Search::Off),
                Some(expected),
                "{key:?} does something else"
            );
        }
    }

    #[test]
    fn settings_keys_are_letters_while_a_search_is_being_typed() {
        let typing = Search::Typing(String::new());
        for key in ['w', 's', 't', 'x', 'm', '?'] {
            assert_eq!(
                action(press(KeyCode::Char(key)), &typing),
                Some(Action::SearchType(key))
            );
        }
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
