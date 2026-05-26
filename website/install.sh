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

# Wrapper around sha256sum / shasum -a 256 — Linux ships the former,
# macOS the latter. Prints the hex digest of $1 to stdout.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Verify $1 against the .sha256 sidecar at $2. The release workflow emits
# `<asset>.sha256` next to every release asset; install.sh refuses to
# proceed if the digest doesn't match. The sidecar is fetched over TLS
# from the same GitHub release as the asset, so this catches at-rest
# tampering of the CDN-served bytes, not a compromised release itself.
verify_sha256() {
  asset_path=$1
  sha_url=$2
  sha_file="$asset_path.sha256"
  if ! curl --proto '=https' --tlsv1.2 -fsSL "$sha_url" -o "$sha_file"; then
    red "could not fetch checksum: $sha_url"
    return 1
  fi
  expected=$(awk '{print $1}' < "$sha_file")
  actual=$(sha256_of "$asset_path")
  if [ "$expected" != "$actual" ]; then
    red "checksum mismatch for $(basename "$asset_path")"
    red "  expected: $expected"
    red "  actual:   $actual"
    return 1
  fi
}

# ── target triple detection ───────────────────────────────────────────
uname_s=$(uname -s)
uname_m=$(uname -m)

case "$uname_s" in
  Linux)  os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  MINGW*|MSYS*|CYGWIN*)
    red "This installer doesn't support Windows. Download the Windows zip"
    red "(turbo-bible-x86_64-pc-windows-msvc.zip) from:"
    red "  https://github.com/$REPO/releases/latest"
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

cyan "→ verifying checksum"
if ! verify_sha256 "$tmp/$asset" "$url.sha256"; then
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

# ── pre-fetch the translation pack ────────────────────────────────────
# The binary ships with only KJV embedded; everything else is fetched
# on demand. Pulling translations.tar.gz here keeps the curl-install
# user offline-from-the-jump — no first-launch network round trip.
# XDG_DATA_HOME defaults to ~/.local/share on Linux/macOS; the binary
# resolves the same path via etcetera.
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/turbo-bible/translations"
mkdir -p "$data_dir"
if [ "$VERSION" = "latest" ]; then
  pack_url="https://github.com/$REPO/releases/latest/download/translations.tar.gz"
else
  pack_url="https://github.com/$REPO/releases/download/$VERSION/translations.tar.gz"
fi
cyan "→ pre-fetching translations (~52 MB)"
pack="$tmp/translations.tar.gz"
# Pre-fetch is best-effort: failures here fall through to the binary's
# on-demand fetch path (which has its own per-translation sha256 check
# against the embedded manifest, so the bypass is safe). We still
# verify the bundle's checksum when we do get it.
if curl --proto '=https' --tlsv1.2 -fL "$pack_url" -o "$pack" \
   && verify_sha256 "$pack" "$pack_url.sha256"; then
  cyan "→ staging into $data_dir"
  tar -xzf "$pack" -C "$tmp"
  # Stage .db.zst files next to the binary's data dir; the binary's
  # first-launch install pass picks them up, decompresses, and
  # removes the .zst (see install::extract_into).
  cp "$tmp"/*.db.zst "$data_dir/" 2>/dev/null || true
  yellow ""
  yellow "Translations staged. They'll decompress on first launch."
else
  yellow ""
  yellow "Translation pre-fetch skipped (offline or checksum failure)."
  yellow "The binary will download translations on demand from the"
  yellow "Translations picker, with per-translation sha256 verification."
fi

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
