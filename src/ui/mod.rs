//! Top-level frame layout: menu bar / body / status bar.

pub mod bookmarks;
pub mod desktop;
pub mod dialog;
pub mod find;
pub mod footnote;
pub mod goto;
pub mod help;
pub mod menubar;
pub mod passage;
pub mod sidebar;
pub mod splash;
pub mod statusbar;
pub mod translations;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::db::Passage;

pub struct Frame<'a> {
    pub menu: &'a [menubar::MenuItem<'a>],
    pub status: &'a [statusbar::Shortcut<'a>],
    pub status_mode: &'a str,
    pub passage: Option<&'a Passage>,
    pub cursor_verse: i64,
    pub selection: Option<(i64, i64)>,
    pub bookmarked: &'a std::collections::BTreeSet<i64>,
    pub show_sidebar: bool,
    pub two_line_verses: bool,
    /// Maximum width (cols) of the reading pane. Centered if terminal wider.
    pub max_reading_width: u16,
}

impl<'a> Frame<'a> {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let (menu_area, body_area, status_area) = split(area);
        menubar::render(self.menu, menu_area, buf);
        desktop::render(body_area, buf);
        if let Some(p) = self.passage {
            let (reading, sidebar_rect) = body_layout(body_area, self.show_sidebar, self.max_reading_width);
            passage::PassageView {
                passage: p,
                cursor_verse: self.cursor_verse,
                selection: self.selection,
                bookmarked: self.bookmarked,
                two_line_verses: self.two_line_verses,
            }
            .render(reading, buf);
            if let Some(sb) = sidebar_rect {
                sidebar::SidebarView {
                    passage: p,
                    cursor_verse: self.cursor_verse,
                }
                .render(sb, buf);
            }
        }
        statusbar::render(self.status, status_area, buf, self.status_mode);
    }
}

fn split(area: Rect) -> (Rect, Rect, Rect) {
    let menu = Rect::new(area.x, area.y, area.width, 1);
    let status = Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));
    (menu, body, status)
}

/// Returns (reading_rect, sidebar_rect). The sidebar is only shown if there's
/// enough horizontal room AND the caller asked for it.
fn body_layout(body: Rect, show_sidebar: bool, max_reading_w: u16) -> (Rect, Option<Rect>) {
    // Geometry per pane: 1 col outer margin + drop shadow (2 cols right).
    const GAP: u16 = 2;
    const SIDEBAR_W: u16 = 34;
    let min_terminal_w_for_sidebar = max_reading_w + GAP + SIDEBAR_W + 4;

    let h = body.height.saturating_sub(2);
    let y = body.y + 1;

    if !show_sidebar || body.width < min_terminal_w_for_sidebar {
        // Centered single pane.
        let w = body.width.min(max_reading_w).saturating_sub(2);
        let x = body.x + (body.width.saturating_sub(w)) / 2;
        return (Rect::new(x, y, w, h), None);
    }

    // Two-pane layout: reading flush-left of the centered group, sidebar to
    // its right.
    let total = max_reading_w + GAP + SIDEBAR_W;
    let left = body.x + (body.width.saturating_sub(total)) / 2;
    let reading = Rect::new(left, y, max_reading_w.saturating_sub(2), h);
    let sidebar = Rect::new(
        left + max_reading_w + GAP,
        y,
        SIDEBAR_W.saturating_sub(2),
        h,
    );
    (reading, Some(sidebar))
}
