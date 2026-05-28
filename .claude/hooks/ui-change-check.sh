#!/usr/bin/env bash
# Stop-hook helper. Echoes a one-line nudge when any UI-affecting file
# (TUI rendering source, theme, poetry layout, or a screenshot tape)
# is dirty vs HEAD — surfacing the change in the conversation transcript
# so Claude (and the user) can decide whether to invoke the ui-review
# skill before committing.
#
# Re-fires every turn while the change is uncommitted; goes quiet on
# commit. No state of its own — debouncing is purely git-driven.
set -eu

paths=$(git status --porcelain 2>/dev/null | cut -c4- || true)
if [ -z "$paths" ]; then
  exit 0
fi

if printf '%s\n' "$paths" \
  | grep -qE '^crates/turbo-bible-tui/src/(ui/|render\.rs|theme\.rs|poetry\.rs|reference\.rs)|^demo/[^/]*\.tape$'; then
  echo '🎨 UI surface modified since HEAD. If you want a designer pass on the affected screenshots, ask Claude to run /ui-review on docs/screenshots/ (regenerate first via just screenshots).'
fi
