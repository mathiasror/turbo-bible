---
description: "Default entry point for building a feature in this repo. Auto-runs when the user asks in natural language (no slash) to implement / build / add / create a non-trivial feature or capability — \"implement X\", \"add a Y that does Z\", \"build so-and-so\", \"can we have …\". Drives the full pipeline: research → plan → isolate in a worktree → implement → designer loop (if visual) → independent reviewer↔implementer loop → PR with before/after screenshots. Do NOT use for quick questions, one-line fixes, pure refactors/renames, doc tweaks, or when the user invoked a different explicit command."
argument-hint: "<feature description>   [--auto | --gates=plan,design,pr] [--no-research]"
---

You are the **conductor** of turbo-bible's feature pipeline. Your job is to take
a feature request from idea to a reviewed PR, delegating the specialist work to
sub-agents and parallelizing everything that can run independently. You do not
do all the work yourself — you spawn agents, sequence the phases, hold the
gates, and keep the worktree clean.

**Feature request:** $ARGUMENTS

Parse any trailing flags out of `$ARGUMENTS` first; the rest is the feature
description.

- `--auto` — skip all human gates; run straight through and present the PR.
- `--gates=a,b,c` — override which gates pause (subset of `plan,design,pr`).
- `--no-research` — skip Phase 1 (use for a tiny, well-understood change).

## Standing rules (these override convenience — they are the user's settled preferences)

- **Isolate in a worktree.** All non-trivial edits happen in a dedicated git
  worktree on its own branch, because other agents run against this repo
  concurrently. Never edit the shared working tree. (memory: worktree-isolation)
- **The deliverable is a PR.** Not commits to `main`, not a pile of local
  changes. branch → `just check`/`audit`/`deny` → push → `gh pr create`.
- **History is additive — never force-push.** If review asks for changes, add
  commits; suggest squash-on-merge instead of rewriting. (memory: no-force-push)
- **Any visual change ships before/after screenshots in the PR body.** Real VHS
  captures, never hand-drawn mocks: before = `main`, after = the branch.
  (memories: pr-before-after-screenshots, website-imagery-real-captures)
- **Palette/visual discipline is the designer's call**, not yours — defer
  yellow-slot and CGA questions to the `ui-review` skill. (memory: yellow-slot)
- The repo's guardrail hooks fire automatically: `cargo check` after every `.rs`
  edit, `cargo fmt` + a UI-dirty nudge at end of turn. Let them work; don't
  re-run them by hand unless a hook surfaced an error.

## Gates

Default: **pause after PLAN** (cheap to redirect scope before code exists) and
**before opening the PR** (final human look). The designer and reviewer loops
run autonomously and only surface to the user if they fail to converge.
`--auto` removes all gates; `--gates=` sets them explicitly. At each active
gate, summarize what's done, state the next step, and stop for the user.

---

## Phase 1 — Research  ★parallel  (skip with `--no-research`)

Gather best-practice sources *before* deciding how to build. Spin up a small
fan-out **in a single message (multiple `Agent` calls so they run concurrently)**,
each on a distinct angle. Pick 2–4 angles that fit the request, e.g.:

- The relevant **ratatui / crossterm** idiom or widget pattern for this feature
  (allowed `WebFetch` domains: `ratatui.rs`, `github.com`; `WebSearch` is on).
- **Prior art** — how comparable TUIs (vim, fzf, lazygit, helix, less) handle it,
  and the keymap/UX conventions users already expect.
- **Pitfalls & constraints** specific to this codebase (SQLite/FTS5 limits, the
  10-ATTACH ceiling, 24-bit-RGB terminal assumptions, the en-kjv-only embed).

Use `Explore`/`general-purpose` agents for the sweep. **Escalate to the
`deep-research` skill** instead of the light sweep when the feature is novel or
architecturally risky (new subsystem, data-model change, anything touching
fetch/install/manifest integrity). Each agent returns a short **cited** brief
(claim → source URL); you merge them into a 5–10 line research summary that
feeds the plan. Note any source that contradicts an assumption in the request.

## Phase 2 — Plan

Synthesize the research into a concrete plan: the files you'll touch, the
approach, the keymap/config surface, the test strategy, and explicitly **whether
this is a visual change** (see the detector in Phase 5). List the risks the
research surfaced and how the plan addresses them. Keep it tight.

▶ **GATE `plan`** (default on): present the research summary + plan and stop.

## Phase 3 — Isolate

Create the worktree and make it buildable:

1. `git worktree add .claude/worktrees/<branch> -b <branch>` (or `EnterWorktree`),
   naming the branch for the feature (`feat/<slug>` / `fix/<slug>`).
2. **Copy the bundled assets** — `assets/*.db.zst` is gitignored and absent in a
   fresh worktree, so `include_bytes!` fails to build. Copy at minimum
   `crates/turbo-bible-tui/assets/en-kjv.db.zst` and `assets/manifest.json` from
   this checkout into the worktree (copy the whole `assets/` dir if a visual
   change will need screenshot regen, which uses the other translations).
3. All subsequent edits and commands run **inside the worktree**.

## Phase 4 — Implement

Build the feature per the plan, inside the worktree. For a contained change,
implement directly; for a broad one, hand the plan to an **implementer** agent.
The PostToolUse `cargo check` hook keeps you honest in-loop. Add or update tests
(unit next to the code; e2e in `crates/turbo-bible-tui/tests/e2e.rs` if behavior
is user-visible over the PTY). Commit logically-scoped work as you go.

## Phase 5 — Designer loop  ★parallel regen  (only if the change is visual)

**Visual-change detector** — run the designer loop when the diff touches any of:
`crates/turbo-bible-tui/src/ui/`, `render.rs`, `theme.rs`, `poetry.rs`,
`reference.rs`, or any `demo/*.tape`. (Same glob the `ui-change-check` hook uses.)
If none are touched, skip straight to Phase 6.

Loop until the designer signs off — this happens **before** the code-review loop:

1. **Regenerate the affected captures** via the `regen-assets` skill /
   `just screenshots` (it may need `just bundle-translations <scrollmapper>`
   first — a checkout lives at `~/git/oss/bible_databases`). Regen only the
   surfaces the change affects; independent surfaces can render in parallel.
2. **Designer review:** invoke the `ui-review` skill on the regenerated
   screenshot(s). It already encodes the rubric, the TUI-specific rules, and a
   cross-check against the UI backlog — treat its severity-ranked findings as
   the spec.
3. **Developer fixes:** address every Blocker/Major (and Minor where cheap) in
   the worktree. Skip a finding only with a one-line reason.
4. **Re-render and re-review.** Repeat 1–3 until the designer raises no new
   Blocker/Major findings. If it hasn't converged after **3 rounds**, stop and
   surface the disagreement to the user — don't loop forever.

▶ **GATE `design`** (default off): if enabled, present the approved before/after
and stop before the review loop.

## Phase 6 — Review loop:  reviewer ⇄ implementer  ★parallel review

A real, independent back-and-forth — the reviewer is a different agent from
whoever wrote the code, so the critique stays honest. (memory: pr-reviewer-implementer-loop)

1. **Reviewer agent** (fresh `general-purpose`, skeptical diff-review prompt):
   reviews the full branch diff for correctness, scope creep, edge cases, test
   coverage, error handling via `anyhow::Result` + `.context`, and adherence to
   the project conventions in `CLAUDE.md`/`CONTRIBUTING.md`. It must also run
   `just check` and treat any failure as a blocking finding. Fan its review out
   by dimension (correctness / tests / conventions / perf) in parallel for a big
   diff. It returns severity-ranked findings, or an explicit **sign-off**.
   *(For a release-grade pass, escalate to the `rust-review` skill as the
   reviewer's lens; the fresh agent is the default.)*
2. **Implementer agent** addresses the findings in the worktree.
3. **Loop** reviewer → implementer → reviewer until the reviewer signs off with
   no outstanding Blocker/Major findings. Cap at **3 rounds**, then surface a
   non-converging review to the user rather than rubber-stamping.

## Phase 7 — Open the PR

1. Final local gate, inside the worktree: `just check && just audit && just deny`.
2. Push the branch (never force).
3. `gh pr create` with a body that includes: what & why, the research sources
   that informed it, the test plan, and — **for any visual change** —
   **before/after screenshots** embedded via `raw.githubusercontent.com` URLs
   (before = `main`, after = the branch). A PR that changed a UI surface without
   before/after images is incomplete.

▶ **GATE `pr`** (default on): if enabled, show the assembled PR body + screenshot
URLs and stop for approval before creating it (or create it as a draft and let
the user promote it).

Leave the worktree in place for the user to inspect; do not `ExitWorktree` or
delete it unless asked.

---

## Parallelism cheat-sheet

Fan these out (one message, multiple `Agent` calls — or the `Workflow` tool for
a large sweep); keep the rest serial:

- **Phase 1 research** — angles run concurrently.
- **Phase 5 screenshot regen** — independent surfaces render concurrently.
- **Phase 6 review** — review dimensions (correctness / tests / conventions /
  perf) analyze concurrently, then merge into one findings list.

Serial by nature: plan synthesis, the implement→designer and implement→reviewer
*loops* (each round depends on the last), and PR assembly.

## What this command does NOT do

- It doesn't cut releases — that's `/release-checklist` + a `v*` tag.
- It doesn't invent colors or override the designer — `theme.rs` constants and
  the `ui-review` verdict are authoritative.
- It doesn't hand-edit generated artifacts (demo gif / screenshots / og-image) —
  it edits the `.tape` source and re-renders via `regen-assets`.
