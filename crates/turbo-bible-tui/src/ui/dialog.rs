//! Shared dialog primitive: a centered, double-bordered window with a
//! drop shadow on top of the dithered desktop. Renders the chrome; callers
//! draw their content into the inner rect.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

use crate::theme;

/// Paint a modal dialog over a dimmed-dither backdrop. Use this for any
/// foreground popup that should visually own the screen (Goto, Find,
/// footnote / xref popup, Help, Translations, Bookmarks). The splash
/// uses [`draw_dialog`] directly because it IS the home screen, not a
/// modal layered above other content.
pub fn draw_modal_dialog(outer: Rect, area: Rect, title: &str, buf: &mut Buffer) -> Rect {
    theme::draw_modal_backdrop(buf, outer);
    draw_dialog(area, title, buf)
}

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

/// A Turbo-Vision "sunken" text input field, shared by Goto and Find so the
/// two frame, pad, and render the cursor identically: a dark left rim `▏`, a
/// one-space inset, the typed `text`, a block cursor `█`, an optional
/// `placeholder` (shown only while `text` is empty), padding out to `width`
/// columns, closed by a bright right rim `▕`. Returns the field's spans; the
/// caller prepends any label. `width` is the whole field including both rims.
pub fn input_field(text: &str, placeholder: &str, width: u16) -> Vec<Span<'static>> {
    let field_bg = theme::input_field_bg();
    let body = Style::new()
        .fg(theme::black())
        .bg(field_bg)
        .add_modifier(Modifier::BOLD);
    let ghost = Style::new().fg(theme::dark_grey()).bg(field_bg);
    let cursor = Style::new()
        .fg(theme::black())
        .bg(theme::bright_white())
        .add_modifier(Modifier::BOLD);
    let edge_left = Style::new().fg(theme::dark_grey()).bg(field_bg);
    let edge_right = Style::new().fg(theme::bright_white()).bg(field_bg);

    let interior = (width as usize).saturating_sub(2).max(1);
    let mut spans = vec![Span::styled("\u{258F}", edge_left)];
    let mut content_w = 1 + text.chars().count() + 1; // leading space + text + cursor
    spans.push(Span::styled(format!(" {text}"), body));
    spans.push(Span::styled("\u{2588}", cursor));
    if text.is_empty() && !placeholder.is_empty() {
        spans.push(Span::styled(placeholder.to_string(), ghost));
        content_w += placeholder.chars().count();
    }
    if content_w < interior {
        spans.push(Span::styled(" ".repeat(interior - content_w), body));
    }
    spans.push(Span::styled("\u{2595}", edge_right));
    spans
}

/// Center a w×h rect within `outer`. Clamps if outer is too small.
pub fn center(outer: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(outer.width);
    let h = h.min(outer.height);
    let x = outer.x + (outer.width.saturating_sub(w)) / 2;
    let y = outer.y + (outer.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
