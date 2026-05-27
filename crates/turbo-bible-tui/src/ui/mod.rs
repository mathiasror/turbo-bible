//! Top-level frame layout: menu bar / body / status bar.

pub mod bookmarks;
pub mod desktop;
pub mod dialog;
pub mod find;
pub mod footnote;
pub mod goto;
pub mod help;
pub mod listnav;
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

/// One reading column's render inputs. The reading view holds a slice of
/// these — one per compare pane.
pub struct PaneRender<'a> {
    pub passage: &'a Passage,
    pub cursor_verse: i64,
    pub selection: Option<(i64, i64)>,
    pub bookmarked: &'a std::collections::BTreeSet<i64>,
    /// The focused pane draws a bright border + mode pill; others dim.
    pub is_focused: bool,
}

pub struct Frame<'a> {
    pub menu_title: &'a str,
    pub status: &'a [statusbar::Shortcut<'a>],
    pub status_mode: &'a str,
    /// One per compare pane, left-to-right. Always at least one.
    pub panes: &'a [PaneRender<'a>],
    pub show_sidebar: bool,
    /// Maximum width (cols) of the reading pane in single-pane mode.
    /// Centered if the terminal is wider; compare panes split the body
    /// evenly instead.
    pub max_reading_width: u16,
}

impl Frame<'_> {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let (menu_area, body_area, status_area) = split(area);
        menubar::render(self.menu_title, menu_area, buf);
        desktop::render(body_area, buf);
        let (rects, sidebar_rect) = panes_layout(
            body_area,
            self.panes.len(),
            self.max_reading_width,
            self.show_sidebar,
        );
        for (rect, pane) in rects.iter().zip(self.panes) {
            passage::PassageView {
                passage: pane.passage,
                cursor_verse: pane.cursor_verse,
                selection: pane.selection,
                bookmarked: pane.bookmarked,
                is_focused: pane.is_focused,
            }
            .render(*rect, buf);
        }
        // The sidebar appears only in single-pane mode (panes_layout yields
        // `Some` only when n == 1), so it follows the sole pane.
        if let Some(sb) = sidebar_rect
            && let Some(pane) = self.panes.first()
        {
            sidebar::SidebarView {
                passage: pane.passage,
                cursor_verse: pane.cursor_verse,
                selection: pane.selection,
            }
            .render(sb, buf);
        }
        statusbar::render(self.status, status_area, buf, self.status_mode);
    }
}

const fn split(area: Rect) -> (Rect, Rect, Rect) {
    let menu = Rect::new(area.x, area.y, area.width, 1);
    let status = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    let body = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(2),
    );
    (menu, body, status)
}

/// Returns (`reading_rect`, `sidebar_rect`). The sidebar is only shown if there's
/// enough horizontal room AND the caller asked for it.
fn body_layout(body: Rect, show_sidebar: bool, max_reading_w: u16) -> (Rect, Option<Rect>) {
    // Geometry per pane: 1 col outer margin + drop shadow (2 cols right).
    const GAP: u16 = 2;
    const SIDEBAR_W: u16 = 34;
    // Saturating: a pathological `max_reading_w` (e.g. a malformed config) must
    // not overflow — it should just saturate high so this branch falls through
    // to the centered single pane below.
    let min_terminal_w_for_sidebar = max_reading_w
        .saturating_add(GAP)
        .saturating_add(SIDEBAR_W)
        .saturating_add(4);

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
    let total = max_reading_w.saturating_add(GAP).saturating_add(SIDEBAR_W);
    let left = body.x + (body.width.saturating_sub(total)) / 2;
    let reading = Rect::new(left, y, max_reading_w.saturating_sub(2), h);
    let sidebar = Rect::new(
        left.saturating_add(max_reading_w).saturating_add(GAP),
        y,
        SIDEBAR_W.saturating_sub(2),
        h,
    );
    (reading, Some(sidebar))
}

/// Minimum interior width for a compare column before the open-pane action
/// refuses to add another. `render` clamps body text and wraps below this,
/// but a bordered pane with a 3-col verse-number gutter is unusable narrower.
pub const MIN_PANE_W: u16 = 28;
/// Columns of blue desktop left between adjacent compare panes.
const PANE_GAP: u16 = 1;

/// Lay out `n` reading panes across `body`, returning one rect per pane
/// (left-to-right) plus an optional sidebar rect.
///
/// `n == 1` delegates to [`body_layout`], preserving the centered-pane +
/// optional-sidebar behavior (and its tests) verbatim. `n >= 2` splits the
/// body into equal columns (remainder distributed left-to-right) and
/// suppresses the sidebar — there's no room for it beside multiple panes.
fn panes_layout(
    body: Rect,
    n: usize,
    max_reading_w: u16,
    show_sidebar: bool,
) -> (Vec<Rect>, Option<Rect>) {
    if n <= 1 {
        let (reading, sb) = body_layout(body, show_sidebar, max_reading_w);
        return (vec![reading], sb);
    }
    // Mirror body_layout's inner inset: 1-row top pad, height minus 2.
    let h = body.height.saturating_sub(2);
    let y = body.y + 1;
    let n_u16 = u16::try_from(n).unwrap_or(u16::MAX);
    let gaps = PANE_GAP.saturating_mul(n_u16.saturating_sub(1));
    let avail = body.width.saturating_sub(gaps);
    let each = avail / n_u16;
    let mut remainder = avail % n_u16;
    let mut rects = Vec::with_capacity(n);
    let mut x = body.x;
    for _ in 0..n {
        let mut col = each;
        if remainder > 0 {
            col += 1;
            remainder -= 1;
        }
        // Each PassageView insets by its own border; subtract the 2-col drop
        // shadow like body_layout does for the single pane.
        rects.push(Rect::new(x, y, col.saturating_sub(2), h));
        x = x.saturating_add(col).saturating_add(PANE_GAP);
    }
    (rects, None)
}

#[cfg(test)]
mod tests {
    use super::{body_layout, panes_layout};
    use ratatui::layout::Rect;

    #[test]
    fn body_layout_survives_absurd_max_width() {
        // A pathological max_reading_width (e.g. a malformed config that slipped
        // past clamping) must not overflow the layout arithmetic; it falls back
        // to the centered single pane.
        let body = Rect::new(0, 0, 120, 40);
        let (reading, sidebar) = body_layout(body, true, u16::MAX);
        assert!(sidebar.is_none(), "absurd width must force single-pane");
        assert!(reading.width <= body.width);
    }

    #[test]
    fn body_layout_two_pane_when_wide_enough() {
        let body = Rect::new(0, 0, 200, 40);
        let (_reading, sidebar) = body_layout(body, true, 80);
        assert!(
            sidebar.is_some(),
            "wide terminal + sidebar on should yield two panes"
        );
    }

    #[test]
    fn panes_layout_single_delegates_to_body_layout() {
        let body = Rect::new(0, 0, 200, 40);
        let (rects, sidebar) = panes_layout(body, 1, 80, true);
        assert_eq!(rects.len(), 1, "n==1 yields exactly one pane rect");
        assert!(sidebar.is_some(), "single pane keeps the sidebar when wide");
    }

    #[test]
    fn panes_layout_multi_splits_evenly_no_sidebar() {
        let body = Rect::new(0, 1, 240, 40);
        for n in 2..=4usize {
            let (rects, sidebar) = panes_layout(body, n, 80, true);
            assert_eq!(rects.len(), n, "one rect per pane");
            assert!(sidebar.is_none(), "compare mode suppresses the sidebar");
            // Columns are left-to-right and non-overlapping.
            for w in rects.windows(2) {
                assert!(w[0].x < w[1].x, "pane x-coords strictly increasing");
                assert!(
                    w[0].x + w[0].width <= w[1].x + 2,
                    "panes don't overlap (allowing the 2-col shadow inset)"
                );
            }
            // Widths differ by at most 1 (remainder distribution).
            let max = rects.iter().map(|r| r.width).max().unwrap();
            let min = rects.iter().map(|r| r.width).min().unwrap();
            assert!(max - min <= 1, "equal columns up to the remainder");
        }
    }

    #[test]
    fn panes_layout_narrow_does_not_panic() {
        // A terminal far too narrow for the requested panes must still return
        // rects without overflowing the layout arithmetic.
        let body = Rect::new(0, 1, 10, 20);
        let (rects, _) = panes_layout(body, 4, 80, false);
        assert_eq!(rects.len(), 4);
    }
}
