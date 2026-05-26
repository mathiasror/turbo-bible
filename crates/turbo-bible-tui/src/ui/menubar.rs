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
    // Clip to the backing buffer first. A degenerate frame (e.g. a 0-row
    // terminal) hands us a rect whose row isn't in the buffer; the raw
    // `buf[(x, area.y)]` loop below would then panic ("index outside of
    // buffer"). Intersecting makes that a no-op draw instead.
    let area = area.intersection(buf.area);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let base = theme::menubar_style();

    for x in area.left()..area.right() {
        let cell = &mut buf[(x, area.y)];
        cell.set_symbol(" ");
        cell.set_style(base);
    }

    let title_style = Style::new()
        .fg(theme::black())
        .bg(base.bg.unwrap_or_else(theme::light_grey))
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

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: a 0-row terminal hands `render` a rect whose row isn't in the
    // (0-height) frame buffer; the raw per-cell loop used to panic with "index
    // outside of buffer". Every degenerate size must now clip to a no-op draw.
    #[test]
    fn render_into_degenerate_buffer_does_not_panic() {
        for (w, h) in [(0u16, 0u16), (20, 0), (0, 1), (1, 1)] {
            let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
            render(" Turbo Bible ", Rect::new(0, 0, w.max(1), 1), &mut buf);
        }
    }
}
