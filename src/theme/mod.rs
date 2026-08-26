//! Semantic style tokens, resolved into terminal styles.
//!
//! Renderers ask for `theme.added`, never for "green". That indirection is what
//! lets one palette use background tints on a truecolor terminal and another
//! fall back to the sixteen colours every terminal has, without a renderer
//! knowing which it got.
//!
//! Colour is never the only channel: every change kind also carries a marker
//! glyph, so the output survives `--color=never`, a monochrome terminal and a
//! reader who cannot distinguish the tints.

use anstyle::{AnsiColor, Color, RgbColor, Style};

/// Which palette to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Palette {
    /// Pick from the environment.
    #[default]
    Auto,
    /// Background tints, 24-bit. Assumes a dark terminal.
    Dark,
    /// The sixteen colours every terminal has. Foreground only, so it makes no
    /// assumption about the terminal's background.
    Ansi,
    /// No colour at all. Markers still distinguish the change kinds.
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub header: Style,
    pub added: Style,
    pub removed: Style,
    pub modified: Style,
    /// The changed words within a modified line — deliberately louder than the
    /// row style it sits inside.
    pub added_emphasis: Style,
    pub removed_emphasis: Style,
    /// A block that moved. Two tints, alternating by group: two blocks that
    /// swapped places must not read as one.
    pub moved: Style,
    pub moved_alt: Style,
    pub gutter: Style,
    pub fold: Style,
    /// The empty half of a row where one side has no line.
    pub filler: Style,
    pub separator: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(Palette::Auto)
    }
}

impl Theme {
    pub fn new(palette: Palette) -> Self {
        match palette {
            Palette::Auto => {
                if truecolor_available() {
                    Self::dark()
                } else {
                    Self::ansi()
                }
            }
            Palette::Dark => Self::dark(),
            Palette::Ansi => Self::ansi(),
            Palette::None => Self::none(),
        }
    }

    /// Background tints, in the spirit of the reference: the block colour
    /// carries the change kind and the emphasis brightens within it.
    pub fn dark() -> Self {
        let bg = |r, g, b| Style::new().bg_color(Some(Color::Rgb(RgbColor(r, g, b))));

        Self {
            header: Style::new().bold(),
            added: bg(0x1e, 0x39, 0x2a),
            removed: bg(0x3f, 0x25, 0x25),
            modified: bg(0x1e, 0x2f, 0x45),
            added_emphasis: bg(0x2f, 0x6b, 0x45).bold(),
            removed_emphasis: bg(0x6e, 0x2f, 0x2f).bold(),
            moved: bg(0x3a, 0x33, 0x1e),
            moved_alt: bg(0x2b, 0x30, 0x3f),
            gutter: Style::new().fg_color(Some(Color::Rgb(RgbColor(0x6b, 0x72, 0x80)))),
            fold: Style::new().fg_color(Some(Color::Rgb(RgbColor(0x6b, 0x72, 0x80)))),
            filler: Style::new().bg_color(Some(Color::Rgb(RgbColor(0x1a, 0x1b, 0x1e)))),
            separator: Style::new().fg_color(Some(Color::Rgb(RgbColor(0x3a, 0x3f, 0x4a)))),
        }
    }

    /// Foreground only. Makes no assumption about the terminal's background,
    /// which is why it is the fallback rather than a dimmer set of tints.
    pub fn ansi() -> Self {
        let fg = |colour| Style::new().fg_color(Some(Color::Ansi(colour)));

        Self {
            header: Style::new().bold(),
            added: fg(AnsiColor::Green),
            removed: fg(AnsiColor::Red),
            modified: fg(AnsiColor::Blue),
            added_emphasis: fg(AnsiColor::Green).bold().underline(),
            removed_emphasis: fg(AnsiColor::Red).bold().underline(),
            moved: fg(AnsiColor::Yellow),
            moved_alt: fg(AnsiColor::Cyan),
            gutter: fg(AnsiColor::BrightBlack),
            fold: fg(AnsiColor::BrightBlack),
            filler: Style::new(),
            separator: fg(AnsiColor::BrightBlack),
        }
    }

    pub fn none() -> Self {
        let plain = Style::new();
        Self {
            header: plain,
            added: plain,
            removed: plain,
            modified: plain,
            added_emphasis: plain,
            removed_emphasis: plain,
            moved: plain,
            moved_alt: plain,
            gutter: plain,
            fold: plain,
            filler: plain,
            separator: plain,
        }
    }
}

impl Theme {
    /// Whether change kind is carried in the *background*.
    ///
    /// This decides whether syntax highlighting can be layered on top. A
    /// palette that says "added" with a green foreground has already spent the
    /// channel syntax highlighting needs; painting tokens over it would leave
    /// the reader unable to tell an addition from a removal.
    pub fn carries_change_in_background(&self) -> bool {
        self.added.get_bg_color().is_some()
    }
}

/// The marker glyph for a change kind — the channel that is not colour.
pub mod marker {
    pub const ADDED: char = '+';
    pub const REMOVED: char = '-';
    pub const MODIFIED: char = '~';
    /// `diff -c`'s glyph for a changed line, kept for the same meaning.
    pub const REPLACED: char = '!';
    /// Left from here.
    pub const MOVED_FROM: char = '<';
    /// Arrived here.
    pub const MOVED_TO: char = '>';
    pub const EQUAL: char = ' ';
    pub const FOLD: char = '⋯';
}

/// Whether the terminal claims 24-bit colour.
///
/// `COLORTERM` is the only portable signal, and a false negative is cheap — the
/// sixteen-colour palette is a fair rendering, not a broken one. A false
/// positive is not, so this asks for an explicit claim.
fn truecolor_available() -> bool {
    matches!(
        std::env::var("COLORTERM").as_deref(),
        Ok("truecolor") | Ok("24bit")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_change_kind_is_visually_distinct_in_each_palette() {
        for theme in [Theme::dark(), Theme::ansi()] {
            let styles = [theme.added, theme.removed, theme.modified, theme.moved];
            for (i, a) in styles.iter().enumerate() {
                for b in styles.iter().skip(i + 1) {
                    assert_ne!(a, b, "two change kinds share a style");
                }
            }
        }
    }

    #[test]
    fn the_two_move_tints_differ_from_each_other() {
        // Two blocks that swapped places share a boundary. One tint for both
        // reads as a single block, which is what alternating them prevents.
        for theme in [Theme::dark(), Theme::ansi()] {
            assert_ne!(theme.moved, theme.moved_alt);
        }
    }

    #[test]
    fn emphasis_differs_from_the_row_it_sits_in() {
        for theme in [Theme::dark(), Theme::ansi()] {
            assert_ne!(theme.added, theme.added_emphasis);
            assert_ne!(theme.removed, theme.removed_emphasis);
        }
    }

    #[test]
    fn only_a_background_palette_leaves_room_for_syntax_colour() {
        assert!(Theme::dark().carries_change_in_background());
        // Foreground palettes have already spent the channel.
        assert!(!Theme::ansi().carries_change_in_background());
        assert!(!Theme::none().carries_change_in_background());
    }

    #[test]
    fn the_none_palette_emits_no_styling_at_all() {
        let theme = Theme::none();
        for style in [theme.added, theme.removed, theme.modified, theme.gutter] {
            assert_eq!(style, Style::new());
        }
    }

    #[test]
    fn markers_are_distinct_so_colour_is_never_the_only_channel() {
        let markers = [
            marker::ADDED,
            marker::REMOVED,
            marker::MODIFIED,
            marker::REPLACED,
            marker::MOVED_FROM,
            marker::MOVED_TO,
            marker::EQUAL,
            marker::FOLD,
        ];
        let unique: std::collections::HashSet<_> = markers.iter().collect();
        assert_eq!(unique.len(), markers.len());
    }
}
