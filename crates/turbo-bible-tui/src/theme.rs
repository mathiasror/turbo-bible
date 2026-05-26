//! Classic CGA / Turbo Vision palette, configurable at runtime via
//! `config.toml`. Call [`init`] once at startup before any rendering.

use std::sync::OnceLock;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::config::ThemeConfig;

static THEME: OnceLock<ThemeConfig> = OnceLock::new();

pub fn init(t: ThemeConfig) {
    THEME.set(t).expect("theme initialized twice");
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
pub fn bright_cyan() -> Color {
    theme().bright_cyan.to_color()
}
pub fn teal() -> Color {
    theme().teal.to_color()
}
pub fn input_teal() -> Color {
    theme().input_teal.to_color()
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

// --- Semantic role slots (the Color Hierarchy) -----------------------------
// Four distinct cyan/teal roles, named so each call site reads by intent and a
// retheme of one role can't silently collapse into another. Ordered by
// luminance: selection (loudest) > list focus > cursor row > input well.

/// Visual-mode selection range — the loudest "active right now" slab.
pub fn selection_bg() -> Color {
    bright_cyan()
}
/// Focused row in a list dialog (bookmarks, translations, find results,
/// splash book picker).
pub fn list_focus_bg() -> Color {
    cyan()
}
/// Cursor-verse fill in the reading pane (normal mode) — toned down from the
/// list-focus slab so scripture dominates, still findable when scanning.
pub fn cursor_row_bg() -> Color {
    teal()
}
/// Editable input-field background (Goto, Find, splash filter).
pub fn input_field_bg() -> Color {
    input_teal()
}
/// Vim mode-pill background for NORMAL and dialog tags. VISUAL/FILTER use
/// `yellow()` for a louder shift; this keeps the calm modes on the CGA cyan.
pub fn mode_pill_bg() -> Color {
    cyan()
}

pub fn menubar_style() -> Style {
    Style::new().fg(black()).bg(light_grey())
}

/// Paint a Turbo Vision–style dimmed dither across the entire `outer` rect.
/// Used as a modal backdrop so dialogs visually own the screen and the
/// reading-pane frame underneath stops competing for attention. The `▒`
/// glyph + dark_grey-on-black palette matches the existing desktop dither
/// so the overlay reads as period chrome, not a modern dim.
pub fn draw_modal_backdrop(buf: &mut Buffer, outer: Rect) {
    let buf_area = buf.area;
    let style = Style::new().fg(dark_grey()).bg(black());
    let x_end = outer.right().min(buf_area.right());
    // Leave the top menu bar and bottom status bar uncovered so the modal
    // floats over the desktop — period-correct Turbo Vision — rather than
    // blanking the whole screen. The dialog is centred within this body band.
    let y_start = outer.top().saturating_add(1);
    let y_end = outer.bottom().min(buf_area.bottom()).saturating_sub(1);
    for y in y_start..y_end {
        for x in outer.left()..x_end {
            let cell = &mut buf[(x, y)];
            cell.set_symbol("\u{2592}");
            cell.set_style(style);
        }
    }
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
