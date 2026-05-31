//! Top-level frame layout: menu bar / body / status bar.

#![allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) documents crate-internal intent in a binary crate \
              with an empty lib.rs; the lint's suggestion to use bare `pub` \
              is the wrong direction for our convention"
)]

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
use ratatui::widgets::{Block, Borders, Widget};

use crate::db::Passage;

/// One reading column's render inputs. The reading view holds a slice of
/// these — one per compare pane.
pub struct PaneRender<'a> {
    pub passage: &'a Passage,
    pub cursor_verse: i64,
    pub selection: Option<(i64, i64)>,
    pub bookmarked: &'a std::collections::BTreeSet<i64>,
    /// The focused pane draws a filled `bright_cyan` title bar + double-line
    /// border + mode pill; others dim to a single-line border. (The loud
    /// focus chrome only applies when there's more than one pane — see
    /// [`passage::PassageView::compare_mode`].)
    pub is_focused: bool,
    /// Set only when the pane was opened from the `K` xref popup via `s`: the
    /// source reference (`"John 3:16"`), rendered as `… ← John 3:16` in the
    /// title. `None` for `Ctrl-W v` compares and the single-pane view.
    pub origin_label: Option<&'a str>,
    /// The focused pane's cursor verse, threaded into each *unfocused* pane so
    /// it can faintly tint the matching verse (a passive cross-pane locator).
    /// `None` on the focused pane and the single-pane view.
    pub peer_verse: Option<i64>,
    /// This pane's cross-pane word diff (verse → diverging word keys), or
    /// `None` when the toggle is off or there's nothing to compare against.
    pub word_diff: Option<&'a crate::worddiff::PaneDiff>,
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
        // In a multi-pane split, only the rightmost pane keeps its drop
        // shadow (it falls on the blue desktop); interior panes suppress it so
        // adjacent columns tile flush instead of smudging a shadow onto the
        // next pane's border. A single pane always keeps its shadow.
        let compare_mode = self.panes.len() > 1;
        let last = self.panes.len().saturating_sub(1);
        for (i, (rect, pane)) in rects.iter().zip(self.panes).enumerate() {
            passage::PassageView {
                passage: pane.passage,
                cursor_verse: pane.cursor_verse,
                selection: pane.selection,
                bookmarked: pane.bookmarked,
                is_focused: pane.is_focused,
                compare_mode,
                origin_label: pane.origin_label,
                peer_verse: pane.peer_verse,
                suppress_shadow: compare_mode && i != last,
                word_diff: pane.word_diff,
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
/// refuses to add another. A bordered pane spends ~6 cols on chrome (the
/// 1-col panel pad, the 1-col gutter, the 3-col verse number, the 2-col gutter
/// gap), so a 40-col interior leaves ~34 cols of prose — about 5–7 words a
/// line, the floor for comfortable reading. 28 (the original) frayed badly at
/// 3 panes (~44 cols apiece), wrapping every verse to 2–3 words a line; 40 is
/// the readable floor that still lets the width be the natural pane-count
/// limiter (no hard cap on panes — a wide terminal can fit several).
pub const MIN_PANE_W: u16 = 40;
/// Columns of blue desktop left between adjacent compare panes.
const PANE_GAP: u16 = 1;

/// The interior (text) width the *narrowest* of `n` evenly-split panes would
/// get across a body `total` cols wide — the figure the open-pane guard
/// checks against [`MIN_PANE_W`]. Mirrors [`panes_layout`]'s column math
/// exactly (the inter-column [`PANE_GAP`]s and the 2-col drop-shadow inset
/// each pane loses), so the guard can't approve a split that the layout then
/// renders below the readable threshold.
#[must_use]
pub fn min_pane_interior(total: u16, n: usize) -> u16 {
    let n_u16 = u16::try_from(n).unwrap_or(u16::MAX).max(1);
    let gaps = PANE_GAP.saturating_mul(n_u16.saturating_sub(1));
    let each = total.saturating_sub(gaps) / n_u16;
    each.saturating_sub(2)
}

/// Lay out `n` reading panes across `body`, returning one rect per pane
/// (left-to-right) plus an optional sidebar rect.
///
/// `n == 1` delegates to [`body_layout`], preserving the centered-pane +
/// optional-sidebar behavior (and its tests) verbatim. `n >= 2` splits the
/// body into equal columns (remainder distributed left-to-right) and
/// suppresses the sidebar — there's no room for it beside multiple panes.
///
/// TODO(design): a slim sidebar on very wide terminals (keeping notes visible
/// alongside 2 panes when the body is, say, ≥200 cols) is deferred polish, not
/// implemented here.
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

/// The interior `(wrap_width, viewport_height)` of each reading pane for the
/// given terminal `area`, pane count, and layout settings — one entry per
/// pane, left-to-right. Delegates to the same [`panes_layout`] (and so the
/// same [`body_layout`]) that [`Frame::render`] draws into, then insets each
/// rect by its border the way [`passage::PassageView`] does, so the figures
/// match what's actually on screen.
///
/// Surfaced so the run loop can size viewport-relative paging (`Ctrl-D` /
/// `Ctrl-F` / `Space`) to the visible rows instead of a fixed verse count,
/// without reaching into the draw closure for the rects.
pub(crate) fn pane_viewports(
    area: Rect,
    panes: usize,
    max_reading_width: u16,
    show_sidebar: bool,
) -> Vec<(u16, u16)> {
    pane_content_rects(area, panes, max_reading_width, show_sidebar)
        .iter()
        .map(|r| (r.width, r.height))
        .collect()
}

/// The text-interior [`Rect`] of each reading pane for the given terminal
/// `area`, pane count, and layout — one entry per pane, left-to-right, in
/// absolute screen coordinates. Shares the same [`panes_layout`] (and so
/// [`body_layout`]) that [`Frame::render`] draws into, then insets each pane
/// rect by its border exactly as [`passage::PassageView`] does, so a mouse
/// hit-test lands on the very cells the draw painted.
///
/// This is the single source the run loop's click handling and the
/// viewport-sizing [`pane_viewports`] both derive from, so the geometry the
/// mouse tests against can't drift from the geometry that was rendered.
pub(crate) fn pane_content_rects(
    area: Rect,
    panes: usize,
    max_reading_width: u16,
    show_sidebar: bool,
) -> Vec<Rect> {
    let (_menu, body, _status) = split(area);
    let (rects, _sidebar) = panes_layout(body, panes, max_reading_width, show_sidebar);
    rects
        .iter()
        // PassageView wraps each rect in a bordered Block (Borders::ALL); the
        // text interior is that block's inner rect — inset one cell per side.
        .map(|r| Block::default().borders(Borders::ALL).inner(*r))
        .collect()
}

/// The body rect — the terminal `area` minus the one-row menu strip and the
/// one-row status bar. The region the reading panes and the splash are laid
/// out within; exposed so the run loop's mouse handling can reconstruct the
/// same region the draw used (see [`Frame::render`] and the splash draw).
pub(crate) fn body_area(area: Rect) -> Rect {
    split(area).1
}

#[cfg(test)]
mod tests {
    use super::{body_layout, pane_content_rects, pane_viewports, panes_layout};
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

    #[test]
    fn pane_viewports_match_the_drawn_pane_interiors() {
        // The run loop sizes paging off pane_viewports; it must equal the inner
        // (border-inset) dims of the rects panes_layout actually draws, for both
        // the centered single pane and the even multi-pane split.
        let area = Rect::new(0, 0, 200, 40);
        let (_menu, body, _status) = super::split(area);
        for n in 1..=3usize {
            let vps = pane_viewports(area, n, 80, true);
            let (rects, _) = panes_layout(body, n, 80, true);
            assert_eq!(vps.len(), rects.len(), "one viewport per pane (n={n})");
            for (vp, rect) in vps.iter().zip(&rects) {
                assert_eq!(
                    *vp,
                    (rect.width.saturating_sub(2), rect.height.saturating_sub(2)),
                    "viewport must equal the pane rect inset by its border (n={n})",
                );
            }
        }
    }

    #[test]
    fn pane_content_rects_are_the_border_inset_of_the_drawn_panes() {
        // The mouse hit-test maps clicks against these rects, so each must be
        // the pane rect panes_layout draws, inset one cell per border side, in
        // absolute screen coords — the interior PassageView renders verses into.
        let area = Rect::new(0, 0, 200, 40);
        let (_menu, body, _status) = super::split(area);
        for n in 1..=3usize {
            let contents = pane_content_rects(area, n, 80, true);
            let (rects, _) = panes_layout(body, n, 80, true);
            assert_eq!(
                contents.len(),
                rects.len(),
                "one content rect per pane (n={n})"
            );
            for (c, rect) in contents.iter().zip(&rects) {
                assert_eq!(c.x, rect.x + 1, "inset left by border (n={n})");
                assert_eq!(c.y, rect.y + 1, "inset top by border (n={n})");
                assert_eq!(
                    c.width,
                    rect.width.saturating_sub(2),
                    "interior width (n={n})"
                );
                assert_eq!(
                    c.height,
                    rect.height.saturating_sub(2),
                    "interior height (n={n})"
                );
            }
        }
    }

    #[test]
    fn min_pane_interior_matches_narrowest_layout_column() {
        // The open-pane guard must check the *actual* narrowest column width
        // panes_layout would produce, not an over-estimate — otherwise it can
        // approve a split that then renders a sub-readable sliver.
        for &total in &[60u16, 84, 112, 137, 200] {
            for n in 2..=4usize {
                let (rects, _) = panes_layout(Rect::new(0, 1, total, 20), n, 80, false);
                let narrowest = rects.iter().map(|r| r.width).min().unwrap();
                assert_eq!(
                    super::min_pane_interior(total, n),
                    narrowest,
                    "guard width must equal the narrowest rendered column (total={total}, n={n})"
                );
            }
        }
    }
}
