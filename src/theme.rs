//! Classic CGA / Turbo Vision palette, configurable at runtime via
//! `config.toml`. Call [`init`] once at startup before any rendering.

use std::sync::OnceLock;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::config::ThemeConfig;

static THEME: OnceLock<ThemeConfig> = OnceLock::new();

pub fn init(t: ThemeConfig) {
    let _ = THEME.set(t);
}

fn theme() -> &'static ThemeConfig {
    THEME.get_or_init(ThemeConfig::default)
}

pub fn blue() -> Color {
    theme().blue.to_color()
}
pub fn cyan() -> Color {
    theme().cyan.to_color()
}
pub fn bright_white() -> Color {
    theme().bright_white.to_color()
}
pub fn light_grey() -> Color {
    theme().light_grey.to_color()
}
pub fn dark_grey() -> Color {
    theme().dark_grey.to_color()
}
pub fn yellow() -> Color {
    theme().yellow.to_color()
}
pub fn hotkey_red() -> Color {
    theme().hotkey_red.to_color()
}
pub fn black() -> Color {
    theme().black.to_color()
}

pub fn menubar_style() -> Style {
    Style::new().fg(black()).bg(light_grey())
}

/// Paint a drop shadow on the cells immediately to the right and below `rect`.
/// Uses solid dark cells so the shadow reads cleanly at modern font weights.
pub fn draw_shadow(buf: &mut Buffer, rect: Rect) {
    let buf_area = buf.area;
    let shadow = Style::new().bg(black());
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
            cell.set_style(shadow);
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
            cell.set_style(shadow);
        }
    }
}
