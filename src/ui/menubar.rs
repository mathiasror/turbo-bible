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

/// Kept for source compatibility with the rest of the code. The fields are
/// ignored — render() always draws the same centered title.
#[allow(dead_code)]
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub hotkey_idx: usize,
}

pub fn render(_items: &[MenuItem<'_>], area: Rect, buf: &mut Buffer) {
    render_title(" Turbo Bible \u{00B7} Bibel 2024 (bokm\u{00E5}l) ", area, buf);
}

pub fn render_title(text: &str, area: Rect, buf: &mut Buffer) {
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
    let used = text.chars().count();
    let pad_left = (area.width as usize).saturating_sub(used) / 2;
    let spans = vec![
        Span::styled(" ".repeat(pad_left), base),
        Span::styled(text.to_string(), title_style),
    ];
    Paragraph::new(Line::from(spans)).style(base).render(area, buf);
}
