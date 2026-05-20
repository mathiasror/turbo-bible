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
        .bg(theme::menubar_style().bg.unwrap_or(theme::light_grey()))
        .add_modifier(Modifier::BOLD);
    let mode_style = Style::new()
        .fg(theme::black())
        .bg(theme::cyan())
        .add_modifier(Modifier::BOLD);

    for x in area.left()..area.right() {
        let cell = &mut buf[(x, area.y)];
        cell.set_symbol(" ");
        cell.set_style(base);
    }

    let mut spans: Vec<Span> = Vec::with_capacity(items.len() * 3 + 1);
    spans.push(Span::styled(" ", base));
    for s in items {
        spans.push(Span::styled(s.key.to_string(), key));
        spans.push(Span::styled(format!(" {}  ", s.action), base));
    }

    // Right-align the mode tag.
    if !mode_tag.is_empty() {
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let mode_text = format!(" {mode_tag} ");
        let pad = (area.width as usize)
            .saturating_sub(used)
            .saturating_sub(mode_text.chars().count());
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), base));
        }
        spans.push(Span::styled(mode_text, mode_style));
    }

    Paragraph::new(Line::from(spans))
        .style(base)
        .render(area, buf);
}
