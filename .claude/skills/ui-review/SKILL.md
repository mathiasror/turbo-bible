---
name: ui-review
description: Designer-style review of a UI from screenshot(s). Use when the user shares a screenshot or image of an interface, says "review this UI", "what do you think of this design", "give me UI/UX feedback", "audit this screen", "look at this capture", "is this readable", "design review", or otherwise asks for opinion on a captured UI surface. Tailored to turbo-bible's Turbo Vision aesthetic (CGA palette, ▒ glyph, TUI grid) but the general rubric applies to any UI. Produces a severity-ranked findings list tied to specific regions of the screenshot. Trigger even for loose phrasings — the user usually says "what do you think" rather than "review".
---

# UI/UX review (skeptical designer)

A senior designer's review of a UI surface from a screenshot. The bar is the
same as `rust-review`: every finding cites a region, quotes the visual evidence,
proposes a concrete fix, and is ranked by severity. Taste-only opinions are
labelled as such.

## Inputs

- **One screenshot** (`docs/screenshots/shot-03-reading.png`, an attached
  PNG, a `demo/demo.gif` still). Read it with the `Read` tool — it renders
  images directly.
- **Multiple screenshots** — a list of paths, several attachments, or a
  glob like `docs/screenshots/*.png`. See "Multi-image triage" below; the
  workflow gains a cross-cutting consistency pass.
- **A directory** (`docs/screenshots/`) — list the contents, apply the
  triage rules, then proceed as multi-image.
- **An image attached inline** to the message: use it as-is.

If the user names a surface in words ("review the splash") without a
screenshot, suggest they regenerate via `just screenshots` (or the relevant
`regen-assets` recipe) and re-invoke. Don't review from memory.

### Multi-image triage

The right depth depends on how many shots there are.

| Count       | Approach                                                                                                                                                                              |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **1**       | Inline review. Skip cross-cutting section.                                                                                                                                            |
| **2–3**     | Per-shot section for each + cross-cutting consistency pass. Inline output unless asked to write to file.                                                                              |
| **4–10**    | Ask the user which 2–3 to deep-review. The rest still count as context for the cross-cutting pass (do skim them — drift between a deep-reviewed shot and a skimmed one is a finding). |
| **≥11**     | Treat it as a **series review** (e.g., the full `docs/screenshots/` tour). Identify the story the series tells, run the consistency pass across all of them, then deep-dive only on the 2–3 surfaces with the most findings. Write to `ui-review.md`. |

A *pair* (two shots) is a special case: usually a before/after or A/B,
and the user wants to know **which one is better and why**. Lead with
that verdict.

## Workflow

1. **Identify each surface.** Splash? Reading view? Goto/Find dialog?
   Compare panes? Sidebar? Use the screenshot filename, the codebase
   (`README.md` keymap section, `crates/turbo-bible-tui/src/ui/`), and
   visible chrome to place it. State your identification in one
   sentence per shot so the user can correct.
2. **Read the rubric** (`references/rubric.md`) before drawing conclusions.
   For TUI surfaces — anything from turbo-bible — also read
   `references/tui-specific.md`. The TUI file encodes project-specific rules
   (yellow-slot, CGA discipline, sidebar width threshold) that override the
   general rubric where they conflict.
3. **Cross-check the backlog.** Open `MEMORY.md` and scan for `UI review
   backlog`. Already-decided items (Bucket A shipped, B/C decisions logged)
   should not be re-raised as new findings — acknowledge them instead.
4. **Per-shot pass.** For each in-scope shot, evaluate across every
   dimension in the rubric. For each finding give: region, problem
   (quoting the visual evidence), fix (concrete; cite the file if known),
   rubric dimension, severity.
5. **Cross-cutting pass (multi-image only, ≥2 shots).** Look for things
   no single shot can show:
   - **Drift** — same concept rendered differently across surfaces
     (dialog header in `mid_cyan` here, `bright_white` there; cursor row
     fill `teal` in one shot but missing in another).
   - **Inconsistent vocabulary** — "Cancel" in one dialog, "Close" in
     the next, for the same action.
   - **Pattern breaks** — a shot that doesn't follow a pattern every
     other shot establishes (drop shadow on five dialogs but not the
     sixth; mode pill present everywhere except one screen).
   - **Series narrative** — for a screenshot tour, does the sequence
     tell a coherent story? Are captions self-consistent? Is the same
     translation/passage used unless a shot's job is to show a different
     one?
   - **Capture-environment drift** — different terminals, different
     window sizes, different themes between shots in what should be a
     uniform set.
   Findings here belong in a separate "Cross-cutting findings" section,
   not folded into a single shot's per-shot section.
6. **Produce the review** using `assets/review-template.md`.
   - **1 shot:** inline; skip the cross-cutting section.
   - **2–3 shots:** inline; include per-shot sections + cross-cutting.
   - **≥4 shots:** save to `ui-review.md` at the repo root (gitignored
     per convention; same pattern as `rust-review.md`).
7. **Self-evaluate.** Rate confidence per dimension. Name dimensions you
   couldn't assess from the shots ("states beyond NORMAL", "long-content
   overflow", "responsive at narrow widths").
8. **Offer the top fixes** one at a time. Do not batch.

## Output style

- **Be specific.** `Sidebar header "References" at col 82, row 2: rendered
  in yellow, which the project reserves for verse numbers + mode pills only
  — switch to mid_cyan (theme.rs::mid_cyan) to match the dialog header
  treatment` is the bar.
- **Quote what you see.** Vague descriptions of the UI are not findings.
  "The hierarchy feels off" is not a finding; "Verse number `16` at col 6 is
  the same weight and color as the body text immediately following it,
  collapsing the hierarchy that yellow is supposed to provide" is.
- **Cite the rubric dimension** by name (hierarchy, alignment, palette
  discipline, microcopy, etc.) so the user can trace the reasoning.
- **Cite the file** when the fix lives somewhere obvious
  (`crates/turbo-bible-tui/src/ui/sidebar.rs`, `theme.rs`).
- **Label taste vs. correctness.** If a finding is preference rather than
  usability/legibility, mark it `(taste)` and demote at least one severity
  level.

## Severity scale

- **Blocker** — UI is broken, illegible, or unusable. Ship-blocker.
  Contrast failure on body text; truncation that hides meaning;
  unreadable in a supported terminal.
- **Major** — wrong hierarchy, confuses users, breaks an established
  pattern. Should fix before the next release.
- **Minor** — polish or small inconsistency. Fix opportunistically.
- **Nit** — taste or refinement. Mention once; don't keep raising.

## When NOT to use this skill

- **Writing new UI code.** That's a `code-review` job, or just implement.
- **Picking a color.** Use the constants in `theme.rs`; don't invent.
- **Regenerating screenshots.** That's `regen-assets`.
- **Critiquing prose, code, or diagrams.** This rubric is for visual UI.
- **Reviewing a non-turbo-bible UI when you don't have a screenshot.**
  Ask for one; don't review from imagination.

## Reference files

- `references/rubric.md` — the dimension-by-dimension checklist. Read
  before evaluating any surface.
- `references/tui-specific.md` — turbo-bible / Turbo Vision rules: CGA
  palette discipline, yellow-slot, ▒ glyph, grid + width constraints,
  pane focus state. Read for any TUI screenshot.
- `assets/review-template.md` — the output structure. Use it verbatim.
