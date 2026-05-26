---
description: Final go/no-go gate before cutting a turbo-bible release (tag-driven).
argument-hint: "[version, e.g. v0.2.0]"
allowed-tools: Bash(git status:*), Bash(git branch:*), Bash(git log:*), Bash(git diff:*), Bash(just check), Bash(just audit), Bash(just deny), Bash(cargo publish:*), Bash(cargo package:*), Bash(grep:*), Bash(rg:*), Bash(gh release list:*), Read, Grep, Glob
---

You are running the **pre-release gate** for turbo-bible. The release is
tag-driven: pushing a `v*` tag fires `.github/workflows/release.yml`, which
builds five target tarballs, uploads per-translation `.db.zst` assets, and
runs `cargo publish`. **There is no undo** — `cargo publish` is permanent and
tags are not edited in place (a bad release means fix-forward + a new tag).
So this checklist must be skeptical: surface problems, do not paper over them.

Target version for this release: **$ARGUMENTS** (if empty, read the current
version from `crates/turbo-bible-tui/Cargo.toml` and treat that as the target).

Work through every section below. For each item, actually run the command or
read the file — do not assume. Record each as ✅ pass / ⚠️ warning / ❌ blocker
with a one-line reason. End with an explicit **GO / NO-GO** verdict and, if GO,
the exact tag commands to run.

## 1. Working tree & branch
- On `main` and up to date with `origin/main`? (`git branch --show-current`, `git status`)
- Working tree clean — no uncommitted or untracked files that belong in the release?
- This release's commits are actually merged to `main` (not stranded on a feature branch)?

## 2. The CI gate (run locally — same jobs CI enforces)
- `just check` (fmt + clippy `-D warnings` + `cargo test --workspace --all-features`) is green.
- `just audit` clean (no unpatched RustSec advisories).
- `just deny` clean (licenses / bans / sources / advisories per `deny.toml`).
- If any fail, that is a ❌ blocker — CI will reject the tag's build anyway.

## 3. Version bump
- `version` in `crates/turbo-bible-tui/Cargo.toml` matches the target tag (tag `vX.Y.Z` ⇒ `version = "X.Y.Z"`).
- The version isn't already published: check `gh release list` and, for a re-release of an existing crate version, that crates.io doesn't already have it (crates.io versions are immutable).
- `Cargo.lock` is committed and reflects the bumped version.

## 4. crates.io publish readiness (the `publish-crate` job)
- Only `en-kjv.db.zst` + `manifest.json` are embedded — the published tarball must stay under crates.io's 10 MB limit. Sanity-check with `cargo package -p turbo-bible --allow-dirty --list` (note: assets/ must be populated, e.g. via `just bundle-translations`) or reason from the `include = [...]` list in `Cargo.toml`.
- `include = [...]` in `crates/turbo-bible-tui/Cargo.toml` still lists `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `assets/en-kjv.db.zst`, `assets/manifest.json` — and release.yml still copies those license files into the crate dir before publish.
- License metadata (`MIT OR Apache-2.0`) matches the actual `LICENSE-*` files present at workspace root.
- `CRATES_IO_TOKEN` repo secret is assumed present (can't verify here — flag as a manual confirm).

## 5. Release-asset & data-pipeline consistency
- `SCROLLMAPPER_REF` is **identical** in `.github/workflows/ci.yml` and `.github/workflows/release.yml`. A mismatch means CI tested a different dataset than the release ships. (`grep -n SCROLLMAPPER_REF .github/workflows/*.yml`)
- The 10 fetched translations + `xrefs` are uploaded as individual `.db.zst` release assets AND bundled in `translations.tar.gz` with a `.sha256` sidecar — the binary's `fetch.rs` verifies per-file sha256 against the embedded `manifest.json`, and `install.sh` verifies the bundle sha256. Confirm the upload-translations job still produces all of these.
- `website/install.sh` still points at `releases/latest/download/` and verifies the `<asset>.sha256` before extracting.

## 6. Docs reflect reality (drift is the silent killer)
- **Known drift to verify:** README.md and CLAUDE.md historically claimed the binary "embeds all eleven translations." The current design embeds only `en-kjv` and fetches the rest from GitHub Releases. Confirm the user-facing docs (README "Setup" section especially) describe the fetch-on-demand model, not the all-bundled one. If they still say "embeds all eleven," that's a ❌ blocker for a public release (it misleads offline users).
- `release.yml` comments reference a `RELEASING.md` that does not exist; the canonical steps live in `CONTRIBUTING.md` → "Cutting a release". Flag the dangling reference (⚠️, not a blocker).
- The README translation table (11 rows, codes + licenses) matches what the pipeline actually ships per `data/manifest_source.toml`.
- README/USAGE keymap and config docs match the current code (spot-check anything touched since the last tag via `git log --oneline <last-tag>..HEAD`).

## 7. Generated artifacts
- If reading/rendering or the splash changed since the last tag, `demo/demo.gif` and `docs/screenshots/*.png` are regenerated (`just demo` / `just screenshots`) and committed. Stale marketing assets ship a wrong first impression.
- `website/og-image.png` is current if the social copy changed.

## 8. Final verdict
Summarize the table of results. Then:
- **NO-GO** if any ❌. List exactly what to fix.
- **GO** only if zero blockers. Then print the release commands verbatim:
  ```sh
  git tag $ARGUMENTS
  git push origin main --tags
  ```
  and remind: watch `/actions`, the run takes ~15 min; if a build fails, fix-forward and tag a new patch version (tags are never edited in place).
