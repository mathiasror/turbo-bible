//! Shared dialog primitive: a centered, double-bordered window with a
//! drop shadow on top of the dithered desktop. Renders the chrome; callers
//! draw their content into the inner rect.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

use crate::theme;

pub fn draw_dialog(area: Rect, title: &str, buf: &mut Buffer) -> Rect {
    theme::draw_shadow(buf, area);

    // Fill window background with blue so cells under the border are clean.
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");
            cell.set_style(Style::new().bg(theme::blue()));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(theme::bright_white()).bg(theme::blue()))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::new().fg(theme::bright_white()).bg(theme::blue()),
        )))
        .style(Style::new().bg(theme::blue()));
    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

/// Center a w×h rect within `outer`. Clamps if outer is too small.
pub fn center(outer: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(outer.width);
    let h = h.min(outer.height);
    let x = outer.x + (outer.width.saturating_sub(w)) / 2;
    let y = outer.y + (outer.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
