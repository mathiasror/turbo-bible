//! The CGA-blue dithered backdrop. Iconic ▒ in cyan-on-blue.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::theme;

pub fn render(area: Rect, buf: &mut Buffer) {
    let style = Style::new().fg(theme::CYAN).bg(theme::BLUE);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_symbol("▒");
            cell.set_style(style);
        }
    }
}
