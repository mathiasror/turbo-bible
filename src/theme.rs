//! Classic CGA / Turbo Vision palette and drawing primitives.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

// CGA palette (24-bit RGB).
pub const BLUE: Color = Color::Rgb(0, 0, 170);
pub const CYAN: Color = Color::Rgb(0, 170, 170);
pub const BRIGHT_WHITE: Color = Color::Rgb(255, 255, 255);
pub const LIGHT_GREY: Color = Color::Rgb(170, 170, 170);
#[allow(dead_code)]
pub const DARK_GREY: Color = Color::Rgb(85, 85, 85);
#[allow(dead_code)]
pub const YELLOW: Color = Color::Rgb(255, 255, 85);
pub const HOTKEY_RED: Color = Color::Rgb(170, 0, 0);
pub const BLACK: Color = Color::Rgb(0, 0, 0);

#[allow(dead_code)]
pub fn window_bg() -> Style {
    Style::new().fg(LIGHT_GREY).bg(BLUE)
}

#[allow(dead_code)]
pub fn window_title() -> Style {
    Style::new().fg(BRIGHT_WHITE).bg(BLUE)
}

#[allow(dead_code)]
pub fn menubar_style() -> Style {
    Style::new().fg(BLACK).bg(LIGHT_GREY)
}

#[allow(dead_code)]
pub fn hotkey_style() -> Style {
    Style::new().fg(Color::Rgb(170, 0, 0)).bg(LIGHT_GREY)
}

#[allow(dead_code)]
pub fn selected_style() -> Style {
    Style::new().fg(BRIGHT_WHITE).bg(Color::Rgb(0, 170, 0))
}

/// Paint a drop shadow on the cells immediately to the right and below `rect`.
/// Uses solid dark cells so the shadow reads cleanly at modern font weights.
#[allow(dead_code)]
pub fn draw_shadow(buf: &mut Buffer, rect: Rect) {
    let buf_area = buf.area;
    // Two-column-wide vertical strip on the right.
    for dx in 1..=2u16 {
        let x = rect.x.saturating_add(rect.width).saturating_add(dx - 1);
        if x >= buf_area.right() {
            break;
        }
        for y in rect.y.saturating_add(1)..rect.y.saturating_add(rect.height).saturating_add(1) {
            if y >= buf_area.bottom() {
                break;
            }
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");
            cell.set_style(Style::new().bg(BLACK));
        }
    }
    // One-row horizontal strip below.
    let y = rect.y.saturating_add(rect.height);
    if y < buf_area.bottom() {
        for x in rect.x.saturating_add(2)..rect.x.saturating_add(rect.width).saturating_add(2) {
            if x >= buf_area.right() {
                break;
            }
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");
            cell.set_style(Style::new().bg(BLACK));
        }
    }
}
