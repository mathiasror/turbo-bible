# UI review — <subject>

<!--
Subject line is:
  - "<surface> (<screenshot filename>)" for a single shot
  - "<N> shots from <source>" for multi-image (e.g. "14 shots from docs/screenshots/")
  - "<A> vs <B>" for a comparison pair

Use this template verbatim. Skip the cross-cutting section for single-shot
reviews; skip per-shot details for series ≥11 (deep-dive only on the
2–3 with the most findings, per the triage rules in SKILL.md).
-->

## Identification

One sentence per in-scope shot, in turbo-bible's own vocabulary
("`shot-03-reading.png` — the reading view of Psalm 23 with the
References sidebar visible on the right"). Give the user one chance to
correct before going further.

For ≥11-shot series reviews, also give a one-paragraph **series
read**: what story do the shots tell taken as a sequence? What's the
intended through-line (feature tour, before/after, theme variants)?

## Summary

2–4 sentences. What works, what doesn't, the single top concern. No
hedging — if it's good, say so; if it's broken, lead with that.

For comparison pairs, **state the verdict here**: which one is better,
and why, in one sentence.

## Findings

| #   | Severity | Shot              | Region                | Finding                          |
| --- | -------- | ----------------- | --------------------- | -------------------------------- |
| 1   | Blocker  | `shot-03`         | <region in plain EN>  | <one-line problem>               |
| 2   | Major    | `shot-07`         | <region>              | <one-line>                       |
| 3   | Major    | **cross**         | <pattern across set>  | <one-line>                       |
| 4   | Minor    | `shot-03`         | <region>              | <one-line>                       |
| 5   | Nit      | `shot-12`         | <region>              | <one-line> (taste)               |

For single-shot reviews, drop the `Shot` column. The `cross` value
identifies cross-cutting findings (defined below).

Order: Blocker → Major → Minor → Nit. Within a severity: cross-cutting
findings first (they affect multiple shots), then by impact.

## Per-shot findings

<!-- One section per in-scope shot. For ≥11-shot series, only the 2–3
shots flagged for deep-dive get a section here; the rest are covered by
the cross-cutting pass + a one-line note in Identification. -->

### `shot-03-reading.png` — reading view (Psalm 23)

#### 1. <Finding title> — Blocker

**Region:** <where on the screenshot, in plain language — "the sidebar
header at the top-right", "row 4 of the book picker", "the status bar
mode pill">

**Problem:** <what's visually wrong, quoting evidence: colors used,
glyphs, spacing measurements in cells, contrast estimates>

**Fix:** <concrete change. File + symbol if known
(`crates/turbo-bible-tui/src/ui/sidebar.rs::render_header`). Color name
from `theme.rs` if a palette change. Cell count if a layout change.>

**Rubric:** <dimension name from rubric.md — "palette discipline",
"hierarchy", "alignment", "microcopy", etc.>

---

(Repeat per finding. Use `---` between findings for breathing room.)

### `shot-07-goto.png` — Goto dialog mid-input

(Same structure: numbered findings, region/problem/fix/rubric for each.)

## Cross-cutting findings

<!-- Multi-image only (≥2 shots). Drop this whole section for single-shot
reviews. -->

Findings that show up across the set, or that only exist *because* the
set isn't consistent. These are usually the most valuable findings in a
multi-shot review.

### C1. <Title> — Major

**Affects:** `shot-03`, `shot-07`, `shot-12` (3 of 14)

**Pattern:** <what's drifting across these shots — e.g., "dialog title
is `mid_cyan` in shot-03 but `bright_white` in shot-07; the rubric
dimension §3 'palette discipline' wants one role per color">

**Evidence:** <one-line cite per affected shot>

**Fix:** <single change that resolves all instances; cite the file>

**Rubric:** <dimension>

---

(Repeat per cross-cutting finding.)

## Series read (≥11-shot series only)

<!-- For full screenshot tours. Drop for smaller sets. -->

- **Story coherence:** does the sequence build understanding, or does it
  feel like a random tour? Where does it lose the thread?
- **Caption / filename discipline:** are captions parallel? Does the
  numbering reflect the intended order?
- **Translation / passage discipline:** is the same translation used
  except where a shot's job is to show a different one? Same passage
  unless variety is the point?
- **Cumulative palette / glyph discipline:** taken together, does the
  set teach the user what each color and glyph means, or does it muddy
  the signal?

## Self-evaluation

- **Confidence:** <high | medium | low> overall.
- **Per-dimension coverage:**
  - Hierarchy: <high/medium/low>
  - Alignment: <high/medium/low>
  - Typography: <high/medium/low>
  - Color: <high/medium/low>
  - … (only list dimensions you actually engaged with)
- **Thin coverage:** <dimensions or states you couldn't assess from the
  shots — "states beyond NORMAL", "long-content overflow", "narrow
  terminal degradation", "non-Latin scripts">.
- **Shots skimmed but not deep-reviewed** (multi-shot only): list them.

## Next steps

The fixes I'd ship first, in order:

1. **#<N>** — <one-line restatement>
2. **#<N>** — <one-line restatement>
3. **#<N>** — <one-line restatement>

Cross-cutting fixes are usually higher leverage than per-shot fixes
(one change, many shots) — bias toward those when they exist.

Want me to start applying these? I'll do them one at a time, one commit
per fix.
