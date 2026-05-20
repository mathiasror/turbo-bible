//! Top title strip. Used to be a Turbo-Vision-style clickable menu, but the
//! status bar at the bottom already shows every action shortcut and having
//! two competing rows felt redundant. The strip is now informational only —
//! mouse clicks on it are ignored.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;

pub fn render(title: &str, area: Rect, buf: &mut Buffer) {
    let base = theme::menubar_style();

    for x in area.left()..area.right() {
        let cell = &mut buf[(x, area.y)];
        cell.set_symbol(" ");
        cell.set_style(base);
    }

    let title_style = Style::new()
        .fg(theme::black())
        .bg(base.bg.unwrap_or(theme::light_grey()))
        .add_modifier(Modifier::BOLD);
    let used = title.chars().count();
    let pad_left = (area.width as usize).saturating_sub(used) / 2;
    let spans = vec![
        Span::styled(" ".repeat(pad_left), base),
        Span::styled(title.to_string(), title_style),
    ];
    Paragraph::new(Line::from(spans))
        .style(base)
        .render(area, buf);
}
