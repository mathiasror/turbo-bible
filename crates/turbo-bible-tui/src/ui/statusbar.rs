//! Bottom status bar showing F-key shortcuts and a vim-style mode indicator.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;

pub struct Shortcut<'a> {
    pub key: &'a str,    // e.g. "F1"
    pub action: &'a str, // e.g. "Help"
}

/// Render shortcuts left-aligned. If `mode_tag` is non-empty, draw it
/// right-aligned in an inverted block — vim-style mode pill.
pub fn render(items: &[Shortcut<'_>], area: Rect, buf: &mut Buffer, mode_tag: &str) {
    let base = theme::menubar_style();
    let key = Style::new()
        .fg(theme::bright_white())
        .bg(theme::menubar_style().bg.unwrap_or_else(theme::light_grey))
        .add_modifier(Modifier::BOLD);
    // VISUAL gets a high-contrast yellow pill so the eye doesn't have to read
    // the four letters — the colour shift alone signals mode change. Other
    // modes share the standard cyan pill.
    let pill_bg = if mode_tag.contains("VISUAL") {
        theme::yellow()
    } else {
        theme::mode_pill_bg()
    };
    let mode_style = Style::new()
        .fg(theme::black())
        .bg(pill_bg)
        .add_modifier(Modifier::BOLD);
    // Bevel cells: ▌ fills the left half of its cell with a bright_white
    // highlight, ▐ fills the right half with a dark_grey shadow; the
    // remaining half of each cell stays in the pill bg, so the pair reads
    // as a raised period pill catching light from the upper-left.
    let bevel_left = Style::new().fg(theme::bright_white()).bg(pill_bg);
    let bevel_right = Style::new().fg(theme::dark_grey()).bg(pill_bg);

    for x in area.left()..area.right() {
        let cell = &mut buf[(x, area.y)];
        cell.set_symbol(" ");
        cell.set_style(base);
    }

    // Reserve the trailing mode pill FIRST, then fit as many shortcuts as the
    // remaining width allows — dropping whole entries from the end. This keeps
    // the mode tag from ever being clipped: the old layout appended the pill
    // last, so a long shortcut list truncated "-- NORMAL --" down to "-- N".
    let total = area.width as usize;
    let mode_text = if mode_tag.is_empty() {
        String::new()
    } else {
        format!(" {mode_tag} ")
    };
    // Pill = ▌ bevel + mode_text + ▐ bevel.
    let pill_width = if mode_tag.is_empty() {
        0
    } else {
        mode_text.chars().count() + 2
    };
    let keys_budget = total.saturating_sub(pill_width);

    let mut spans: Vec<Span> = Vec::with_capacity(items.len() * 3 + 4);
    let mut used = 0usize;
    if keys_budget > 0 {
        spans.push(Span::styled(" ", base));
        used += 1;
    }
    for s in items {
        // " " (in the key span's trailing format) + key + " action  ":
        // key + action + 3 cells.
        let entry_w = s.key.chars().count() + s.action.chars().count() + 3;
        if used + entry_w > keys_budget {
            break; // elide this entry (and the rest) to protect the pill
        }
        spans.push(Span::styled(s.key.to_string(), key));
        spans.push(Span::styled(format!(" {}  ", s.action), base));
        used += entry_w;
    }

    // Right-align the mode tag, wrapped in a two-cell bevel (▌ + ▐).
    if !mode_tag.is_empty() {
        let pad = keys_budget.saturating_sub(used);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), base));
        }
        spans.push(Span::styled("\u{258C}", bevel_left));
        spans.push(Span::styled(mode_text, mode_style));
        spans.push(Span::styled("\u{2590}", bevel_right));
    }

    Paragraph::new(Line::from(spans))
        .style(base)
        .render(area, buf);
}
