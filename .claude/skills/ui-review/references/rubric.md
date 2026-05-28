# UI review rubric

Eleven dimensions. Go through each in order. Findings cite the dimension.

## 1. Visual hierarchy

The eye should land on the most important thing first, then second, then
third — without effort.

- Identify the intended primary element. Does it actually win?
- Are two elements competing? (Same size + same color + same position =
  visual tie.)
- Hierarchy levers in priority order: **size > weight > color > position >
  whitespace**. If color is doing the work alone, it's fragile.
- A flat surface (no hierarchy) is often worse than the wrong hierarchy —
  the user has nowhere to start.

## 2. Layout & alignment

Misalignment is the loudest thing on a screen, and the cheapest to fix.

- Pick a grid (column widths, gutter, row baseline). Does every element
  snap to it?
- Padding consistent across similar elements (cards, list rows, dialog
  buttons)?
- Optical vs mathematical centering: a `▒` block centered by math will look
  off; let the eye decide.
- Edges: equal gutters on left and right of the viewport. Equal padding
  inside frames.
- In a TUI: every element snaps to character cells. Off-by-one is visible.

## 3. Typography

- **Type scale.** At least 3 visible steps (title, heading, body) — more
  if the surface earns them. Avoid in-between sizes that look like
  mistakes.
- **Weight.** Use weight to disambiguate, not just decoration. Don't mix
  more than 2 typefaces or 3 weights per surface.
- **Line length.** Body text wants 45–75 characters per line. Wider →
  the eye loses its place returning to the next line.
- **Line height.** 1.4–1.6× the font size for body. Tighter for headings.
- **In a TUI:** the typeface is the user's terminal font. You don't
  control it. You control character count per line, weight via bold,
  and color.

## 4. Color & palette discipline

- **Adhere to the palette.** Off-palette colors are bugs. For turbo-bible,
  the palette lives in `theme.rs`; flag any color that isn't one of the
  named constants.
- **Contrast.** Body text vs background should clear WCAG AA: 4.5:1 for
  small text, 3:1 for large. Brand or accent colors against background
  often fail this — check.
- **Semantic use.** Red = error/danger; green = success; yellow = caution
  or accent. Project-specific overrides take precedence (see
  `tui-specific.md` for the yellow-slot rule).
- **No color-only signals.** Anything you convey with color, also convey
  with size, glyph, or text. Color-blind users + grayscale screenshots
  in docs both depend on this.

## 5. Density & whitespace

- Too dense → overwhelming, nothing has room to read.
- Too sparse → wastes the screen, breaks proximity grouping.
- **Proximity = relatedness.** Items closer together read as one group.
  If two unrelated things are touching, separate them. If two related
  things are far apart, close the gap.
- White space *around* a group says "this is one thing." White space
  *inside* a group says "these are siblings."

## 6. Affordance & discoverability

- Can the user tell what's interactive without trying everything?
- For a TUI: keybindings are the affordance. Are the most-used bindings
  visible in the status bar / menu bar / help? (turbo-bible: status bar
  hints + `F1` help + hotkey-red letters in the menu bar.)
- **Empty states must teach.** A blank list with no copy is a dead end.
  Tell the user what would normally be here and how to get one.
- The first action a user can take should be obvious from the home
  screen. (turbo-bible's splash: book picker + Continue is good; if a
  user has to read help to take their first action, that's a finding.)

## 7. Consistency

- Same pattern repeated across surfaces? (Dialog header treatment,
  status bar layout, focus border.)
- Same vocabulary for the same action? ("Remove" + "Delete" + "Clear"
  for one operation = a finding.)
- Same affordance shape for the same kind of action? (Every cross-ref
  follower uses `s`; every dialog closes with `Esc`.)
- Inconsistency erodes trust faster than ugliness.

## 8. Feedback & state

For every interactive element, four states matter:

- **Default** — what does it look like at rest?
- **Focus / hover** — is the user's current target obvious?
- **Active / pressed** — does the user know their input registered?
- **Disabled** — is it visibly unavailable, with a reason if possible?

Plus:

- **Empty state** — see §6.
- **Loading** — if work takes >100ms, signal it. >1s, indicate progress.
- **Error** — say what's wrong AND how to recover.
- **Success / confirmation** — close the loop on user actions.

## 9. Accessibility

- Contrast (see §4).
- Focus indicator visible without relying on color alone (border, ring,
  inversion).
- No motion-only signals (a thing that fades in and disappears is
  invisible to users who looked away).
- Text resizable / not pinned at the smallest readable size.
- For a TUI: works at the default terminal font size; works without
  256-color if degraded mode is supported.

## 10. Microcopy

- **Active voice, present tense.** "Save bookmark" beats "Bookmark will
  be saved."
- **Specific verbs.** "Open" / "Apply" / "Discard" — not "OK" / "Submit".
- **Error wording.** Say what failed, in plain language, with a fix.
  "Cannot read file: permission denied — try `chmod +r path/to/file`"
  beats "Error 13."
- **Voice.** Pick one tone (terse + technical, or warm + chatty) and
  hold it. Mixed voice reads as machine-translated.
- Sentence case, not Title Case, for buttons unless the platform demands
  otherwise.

## 11. Edge cases

- **Long content** — text longer than the column. Truncate? Wrap? Ellipsis?
  Whichever you pick, do it consistently.
- **Empty content** — see §6.
- **Tiny viewport** — degrade gracefully or refuse with a clear message.
  (turbo-bible refuses compare panes when too narrow — that's the right
  pattern.)
- **Long lists** — scroll? paginate? filter? Decide.
- **Unicode** — non-Latin scripts, RTL text, double-width CJK characters
  on a monospace grid. Don't ignore them.

## Severity scale (also in SKILL.md)

- **Blocker** — broken / illegible / unusable. Ship-blocker.
- **Major** — wrong hierarchy, confuses users, breaks pattern. Fix
  before next release.
- **Minor** — polish, small inconsistency. Fix opportunistically.
- **Nit** — taste, optional. Mention once.

## Anti-patterns to flag specifically

- **Decorative chrome that distracts.** A border, divider, or shadow
  whose only job is to look "designed." Remove it; trust the layout.
- **Symmetrical layouts where asymmetry would clarify.** Centering
  everything is the rookie move.
- **Three accent colors competing.** Pick one accent, demote the others
  to neutral or a tint of the same accent.
- **Icons without labels** in low-frequency surfaces. Users learn icons
  in high-frequency UI; in a once-a-month dialog they're guessing.
- **Status hidden in places the user isn't looking.** "Saved" flashes
  at the top of the screen while the user's eyes are at the bottom.
