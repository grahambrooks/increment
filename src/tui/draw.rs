//! Drawing, and nothing else.
//!
//! Every question of *where the reader is* has been answered by [`super::state`]
//! before this module runs. What is left is turning rows into cells — and the
//! rows come from `render::split::compose`, the same function the stdout
//! renderer uses, so the browser and the pager cannot disagree about what a
//! diff looks like.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::model::RowKind;
use crate::theme::marker;

use super::keys::{HINTS, NESTED_HINTS, REVIEW_HINTS};
use super::review::{Mode, Pane, Review};
use super::state::{App, Focus, Search};

/// Columns given to the file list. Hidden entirely for a single file, where it
/// would be a sidebar with one row in it.
const FILE_LIST_WIDTH: u16 = 32;

/// The change map: a whole-file overview in one column.
///
/// This is the piece deliberately left out of the stdout renderer. In a pager
/// it would restate the marker already in the gutter; here, where only part of
/// the file is on screen, it is the only thing that says how much is left and
/// where.
const MAP_WIDTH: u16 = 1;

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    diff_view(frame, app, frame.area());
}

/// Draw the diff browser into a given area.
///
/// Taking an area rather than the whole frame is what lets the review flow put
/// a commit list above it without the two drawing over each other.
pub fn diff_view(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let [body, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let show_files = app.entries().len() > 1;
    let [files, diff, map] = Layout::horizontal([
        Constraint::Length(if show_files { FILE_LIST_WIDTH } else { 0 }),
        Constraint::Min(10),
        Constraint::Length(MAP_WIDTH),
    ])
    .areas(body);

    if show_files {
        draw_files(frame, app, files);
    }

    // The drawer is the only thing that knows how much room there is, so it
    // tells the state machine before asking it for anything.
    let inner = diff.height.saturating_sub(2).max(1);
    app.set_viewport(diff.width.saturating_sub(2) as usize, inner as usize);

    draw_diff(frame, app, diff);
    draw_map(frame, app, map);
    draw_status(frame, app, status);
}

fn draw_files(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .entries()
        .iter()
        .map(|entry| {
            let stats = entry.document.stats;
            // All three counts, not just additions and removals. A file whose
            // every change is an edit would otherwise be listed as `+0 -0`,
            // which reads as "nothing happened here".
            let mut counts = vec![
                (format!("+{}", stats.added), Color::Green),
                (format!("-{}", stats.removed), Color::Red),
                (format!("~{}", stats.modified), Color::Blue),
            ];
            if stats.moved > 0 {
                counts.push((format!("⇄{}", stats.moved), Color::Yellow));
            }
            let width_of_counts: usize = counts
                .iter()
                .map(|(text, _)| text.chars().count() + 1)
                .sum();

            let mut spans = vec![Span::raw(short(
                entry.name(),
                (area.width as usize).saturating_sub(width_of_counts + 3),
            ))];
            for (text, colour) in counts {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(text, Style::new().fg(colour)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected()));

    frame.render_stateful_widget(
        List::new(items)
            .block(bordered(" files ", app.focus() == Focus::Files))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        area,
        &mut state,
    );
}

fn draw_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let title = match app.entry() {
        Some(entry) => format!(" {} ", entry.name()),
        None => " no changes ".to_owned(),
    };
    let block = bordered(&title, app.focus() == Focus::Diff);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows: Vec<Line<'_>> = app
        .layout()
        .iter()
        .skip(app.scroll())
        .take(inner.height as usize)
        .map(|row| {
            Line::from(
                row.cells
                    .iter()
                    .map(|cell| Span::styled(cell.text.clone(), convert(cell.style)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

/// The change map, one cell per screen row.
///
/// Each cell stands for a slice of the file and takes the most significant kind
/// in it — a single changed line in a hundred unchanged ones must not vanish
/// because it was outvoted.
fn draw_map(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let kinds = app.row_kinds();
    let height = area.height as usize;
    if kinds.is_empty() || height == 0 {
        return;
    }

    // Only worth marking when there is somewhere else to be: on a file that
    // fits on screen, "you are here" over every cell says nothing.
    let scrollable = app.max_scroll() > 0;
    let visible = app.scroll()..app.scroll() + height.max(1);
    let lines: Vec<Line<'_>> = (0..height)
        .map(|cell| {
            let from = cell * kinds.len() / height;
            let to = ((cell + 1) * kinds.len() / height)
                .max(from + 1)
                .min(kinds.len());
            let kind = kinds[from..to].iter().max_by_key(|kind| weight(kind));

            let (glyph, colour) = match kind {
                Some(RowKind::Added) => ('▐', Color::Green),
                Some(RowKind::Removed) => ('▐', Color::Red),
                Some(RowKind::Modified) => ('▐', Color::Blue),
                Some(RowKind::Replaced) => ('▐', Color::Magenta),
                Some(RowKind::Moved { .. }) => ('▐', Color::Yellow),
                Some(RowKind::Fold { .. }) => ('╌', Color::DarkGray),
                _ => ('│', Color::DarkGray),
            };

            // Where the reader is, drawn over the top of what is there.
            let style = if scrollable && visible.contains(&from) {
                Style::new().fg(colour).add_modifier(Modifier::REVERSED)
            } else {
                Style::new().fg(colour)
            };
            Line::from(Span::styled(glyph.to_string(), style))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

/// How much a change kind deserves to win a cell of the map.
fn weight(kind: &RowKind) -> u8 {
    match kind {
        RowKind::Equal => 0,
        RowKind::Fold { .. } => 1,
        // Below a real addition or removal: a move is code the reader has
        // already seen, and it should not outrank new code in the overview.
        RowKind::Moved { .. } => 2,
        RowKind::Added | RowKind::Removed => 3,
        RowKind::Modified | RowKind::Replaced => 4,
    }
}

fn draw_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Whatever the reader most needs to know, in priority order: what they are
    // typing, then anything the last action wanted to tell them, then where
    // they are.
    let line = match (app.search(), app.notice()) {
        (Search::Typing(query), _) => Line::from(vec![
            Span::styled("/", Style::new().fg(Color::Yellow)),
            Span::raw(query.clone()),
            Span::styled("▏", Style::new().fg(Color::Yellow)),
        ]),
        (_, Some(notice)) => Line::from(Span::styled(
            notice.to_owned(),
            Style::new().fg(Color::Yellow),
        )),
        (Search::Active { query, matches, at }, None) => Line::from(vec![
            Span::styled(format!("/{query}"), Style::new().fg(Color::Yellow)),
            Span::raw(format!("  match {}/{}  ", at + 1, matches.len())),
            Span::styled(hints(app), Style::new().fg(Color::DarkGray)),
        ]),
        (Search::Off, None) => Line::from(vec![
            Span::raw(position(app)),
            Span::raw("  "),
            Span::styled(hints(app), Style::new().fg(Color::DarkGray)),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
}

fn position(app: &App) -> String {
    let total = app.layout().len();
    let Some(document) = app.document() else {
        return "no changes".to_owned();
    };
    let stats = document.stats;
    let moved = if stats.moved > 0 {
        format!("  ⇄{}", stats.moved)
    } else {
        String::new()
    };
    format!(
        "row {}/{}  {}{} {}{} {}{}{moved}",
        (app.scroll() + 1).min(total.max(1)),
        total,
        marker::ADDED,
        stats.added,
        marker::REMOVED,
        stats.removed,
        marker::MODIFIED,
        stats.modified,
    )
}

fn hints(app: &App) -> String {
    if app.is_nested() { NESTED_HINTS } else { HINTS }
        .iter()
        .map(|(keys, what)| format!("{keys} {what}"))
        .collect::<Vec<_>>()
        .join("  ")
}

fn bordered(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::new().fg(Color::Blue)
        } else {
            Style::new().fg(Color::DarkGray)
        })
        .title(title.to_owned())
}

/// Trim a path from the left, so the filename — the part that identifies it —
/// survives.
fn short(name: &str, width: usize) -> String {
    if name.chars().count() <= width || width <= 1 {
        return name.to_owned();
    }
    let tail: String = name
        .chars()
        .skip(name.chars().count() - width + 1)
        .collect();
    format!("…{tail}")
}

/// `anstyle` to `ratatui`.
///
/// The renderers speak `anstyle` because they write to a stream; this is the
/// one place that has to translate, and keeping it in one place is why adding a
/// surface does not mean rewriting the palette.
fn convert(style: anstyle::Style) -> Style {
    let mut out = Style::new();
    if let Some(colour) = style.get_fg_color() {
        out = out.fg(colour_of(colour));
    }
    if let Some(colour) = style.get_bg_color() {
        out = out.bg(colour_of(colour));
    }

    let effects = style.get_effects();
    for (effect, modifier) in [
        (anstyle::Effects::BOLD, Modifier::BOLD),
        (anstyle::Effects::DIMMED, Modifier::DIM),
        (anstyle::Effects::ITALIC, Modifier::ITALIC),
        (anstyle::Effects::UNDERLINE, Modifier::UNDERLINED),
    ] {
        if effects.contains(effect) {
            out = out.add_modifier(modifier);
        }
    }
    out
}

fn colour_of(colour: anstyle::Color) -> Color {
    use anstyle::AnsiColor as A;
    match colour {
        anstyle::Color::Rgb(rgb) => Color::Rgb(rgb.0, rgb.1, rgb.2),
        anstyle::Color::Ansi256(indexed) => Color::Indexed(indexed.0),
        anstyle::Color::Ansi(ansi) => match ansi {
            A::Black => Color::Black,
            A::Red => Color::Red,
            A::Green => Color::Green,
            A::Yellow => Color::Yellow,
            A::Blue => Color::Blue,
            A::Magenta => Color::Magenta,
            A::Cyan => Color::Cyan,
            // anstyle's `White` is the dim one and `BrightWhite` the bright
            // one; ratatui names them `Gray` and `White`.
            A::White => Color::Gray,
            A::BrightBlack => Color::DarkGray,
            A::BrightRed => Color::LightRed,
            A::BrightGreen => Color::LightGreen,
            A::BrightYellow => Color::LightYellow,
            A::BrightBlue => Color::LightBlue,
            A::BrightMagenta => Color::LightMagenta,
            A::BrightCyan => Color::LightCyan,
            A::BrightWhite => Color::White,
        },
    }
}

/// Draw the review: a commit list, and the selected commit's diff below it.
pub fn review(frame: &mut Frame<'_>, review: &mut Review<'_>) {
    let area = frame.area();

    match review.mode() {
        Mode::List => {
            let [list, status] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
            review.set_log_viewport(list.height.saturating_sub(2).max(1) as usize);
            draw_commits(frame, review, list);
            draw_review_status(frame, review, status);
        }
        Mode::Split => {
            // Enough of the log to keep your place in it, and the rest to the
            // diff — which is the thing being read.
            let log_height = (area.height * 35 / 100).clamp(4, 14);
            let [list, diff] =
                Layout::vertical([Constraint::Length(log_height), Constraint::Min(4)]).areas(area);

            review.set_log_viewport(list.height.saturating_sub(2).max(1) as usize);
            draw_commits(frame, review, list);

            // The diff draws its own status line, so the review does not add a
            // second one competing for the same row.
            if let Some(app) = review.diff_mut() {
                diff_view(frame, app, diff);
            } else {
                frame.render_widget(
                    Paragraph::new("no diff loaded").block(bordered(" diff ", false)),
                    diff,
                );
            }
        }
    }
}

fn draw_commits(frame: &mut Frame<'_>, review: &Review<'_>, area: Rect) {
    let focused = review.focus() == Pane::Log;
    let title = format!(" {} commits ", review.commits().len());
    let block = bordered(&title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if review.commits().is_empty() {
        frame.render_widget(Paragraph::new("no commits"), inner);
        return;
    }

    // The summary gets whatever the fixed columns do not need. It is the part
    // that is actually read; the id and date are there to be scanned past.
    let fixed = 7 + 2 + 10 + 2;
    let summary_width = (inner.width as usize).saturating_sub(fixed + 2);

    let rows: Vec<Line<'_>> = review
        .visible()
        .map(|index| {
            let commit = &review.commits()[index];
            let selected = index == review.selected();
            let spans = vec![
                Span::styled(
                    commit.short_id.chars().take(7).collect::<String>(),
                    Style::new().fg(Color::Yellow),
                ),
                Span::raw("  "),
                Span::styled(commit.date.clone(), Style::new().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::raw(clip(&commit.summary, summary_width)),
            ];
            let line = Line::from(spans);
            if selected {
                line.style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

fn draw_review_status(frame: &mut Frame<'_>, review: &Review<'_>, area: Rect) {
    let line = match review.notice() {
        Some(notice) => Line::from(Span::styled(
            notice.to_owned(),
            Style::new().fg(Color::Yellow),
        )),
        None => {
            let position = if review.commits().is_empty() {
                "no commits".to_owned()
            } else {
                format!("{}/{}", review.selected() + 1, review.commits().len())
            };
            let hints = REVIEW_HINTS
                .iter()
                .map(|(keys, what)| format!("{keys} {what}"))
                .collect::<Vec<_>>()
                .join("  ");
            Line::from(vec![
                Span::raw(position),
                Span::raw("  "),
                Span::styled(hints, Style::new().fg(Color::DarkGray)),
            ])
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Trim from the right: a commit summary front-loads its meaning.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return String::new();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}…")
}
