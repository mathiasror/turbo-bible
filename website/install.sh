#!/bin/sh
# Turbo Bible installer.
#
# Detects the running OS/arch, downloads the matching tarball from the
# latest GitHub release, extracts the binary into ~/.local/bin (or
# /usr/local/bin if it exists and is writable), and prints PATH
# guidance if needed.
#
# Usage:  curl -fsSL turbo.bible/install.sh | sh
#
# Env vars:
#   TB_VERSION=v0.1.0   Pin to a specific tag instead of latest.
#   TB_INSTALL_DIR=...  Override the install directory.
#   TB_REPO=mathiasror/turbo-bible
#
# Exits non-zero on any failure. No telemetry.

set -eu

REPO="${TB_REPO:-mathiasror/turbo-bible}"
VERSION="${TB_VERSION:-latest}"

red()    { printf '\033[31m%s\033[0m\n' "$*" >&2; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
cyan()   { printf '\033[36m%s\033[0m\n' "$*"; }

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    red "missing required tool: $1"
    exit 1
  fi
}

need curl
need tar
need uname
need mkdir
need install

# ── target triple detection ───────────────────────────────────────────
uname_s=$(uname -s)
uname_m=$(uname -m)

case "$uname_s" in
  Linux)  os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  MINGW*|MSYS*|CYGWIN*)
    red "Windows: use the PowerShell installer instead, or download a tarball directly from"
    red "  https://github.com/$REPO/releases"
    exit 1
    ;;
  *)
    red "unsupported OS: $uname_s"
    exit 1
    ;;
esac

case "$uname_m" in
  x86_64|amd64)        arch=x86_64 ;;
  arm64|aarch64)       arch=aarch64 ;;
  *)
    red "unsupported architecture: $uname_m"
    exit 1
    ;;
esac

target="$arch-$os"
asset="turbo-bible-$target.tar.gz"

if [ "$VERSION" = "latest" ]; then
  url="https://github.com/$REPO/releases/latest/download/$asset"
else
  url="https://github.com/$REPO/releases/download/$VERSION/$asset"
fi

# ── install directory ─────────────────────────────────────────────────
if [ -n "${TB_INSTALL_DIR:-}" ]; then
  install_dir="$TB_INSTALL_DIR"
elif [ -w /usr/local/bin ] 2>/dev/null; then
  install_dir=/usr/local/bin
else
  install_dir="$HOME/.local/bin"
fi
mkdir -p "$install_dir"

# ── download + extract ────────────────────────────────────────────────
tmp=$(mktemp -d "${TMPDIR:-/tmp}/turbo-bible.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

cyan "→ downloading $asset"
if ! curl --proto '=https' --tlsv1.2 -fL "$url" -o "$tmp/$asset"; then
  red "download failed: $url"
  red "(maybe no release exists yet for $target — check https://github.com/$REPO/releases)"
  exit 1
fi

cyan "→ extracting"
tar -xzf "$tmp/$asset" -C "$tmp"

bin_src=$(find "$tmp" -type f -name turbo-bible -perm -u+x | head -n1)
if [ -z "$bin_src" ] || [ ! -f "$bin_src" ]; then
  red "tarball did not contain a turbo-bible binary"
  exit 1
fi

cyan "→ installing to $install_dir/turbo-bible"
install -m 0755 "$bin_src" "$install_dir/turbo-bible"

# ── post-install hints ────────────────────────────────────────────────
case ":$PATH:" in
  *:"$install_dir":*) ;;
  *)
    yellow ""
    yellow "$install_dir is not in your PATH. Add this to your shell rc:"
    yellow "  export PATH=\"\$PATH:$install_dir\""
    ;;
esac

cyan ""
cyan "Installed. Run: turbo-bible"
