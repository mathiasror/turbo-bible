//! Notify-only update check. On startup the splash screen asks GitHub for
//! the latest release tag and, if it's newer than the running binary, shows
//! a one-line banner with the upgrade command for *how this copy was
//! installed* (Homebrew / cargo / curl). It never downloads or replaces the
//! binary — that's a deliberate choice: self-replacing fights the package
//! managers and adds a security/Windows surface we don't want.
//!
//! Design mirrors the on-demand translation fetch in [`crate::fetch`]: we
//! shell out to `curl` (no HTTP crate), parse the `releases/latest` redirect
//! (no GitHub API, no token, no JSON), and throttle to once per 24h via a
//! tiny `update.toml` cache. The check runs on a worker thread and is drained
//! non-blocking by the main loop (see `main::poll_update_check`), so launch
//! is never delayed; on any failure (offline, curl missing, parse) it is
//! silent and retried next launch.

use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::paths;

/// A `major.minor.patch` release version. We hand-roll this rather than pull
/// the `semver` crate: release tags are always plain `vX.Y.Z`, so a numeric
/// triple compare is all we need and it keeps the dependency tree minimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    // Field order matters: the derived `Ord` compares major, then minor, then
    // patch — exactly release precedence.
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// Parse `vX.Y.Z` (or `X.Y.Z`). Returns `None` for anything that isn't
    /// exactly three numeric dot-parts — a leading `v`/`V` is stripped, but a
    /// pre-release suffix (`v1.2.3-rc1`), a two-part (`1.2`), or a four-part
    /// (`1.2.3.4`) string is rejected rather than guessed at.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let mut it = s.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None; // four-or-more parts
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// The running binary's version, from `CARGO_PKG_VERSION`. The crate
    /// version is authored as plain semver, so this never fails in practice;
    /// `None` would only mean a malformed `Cargo.toml`, in which case we just
    /// skip the check.
    pub fn current() -> Option<Self> {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// True when `latest` is strictly newer than `current`.
pub fn is_newer(latest: Version, current: Version) -> bool {
    latest > current
}

/// Pull the version out of a `releases/latest` redirect URL
/// (`https://github.com/owner/repo/releases/tag/v0.2.0`): drop any query or
/// fragment, take the last non-empty path segment, and parse it.
pub fn parse_tag(redirect_url: &str) -> Option<Version> {
    let url = redirect_url
        .split(['?', '#'])
        .next()
        .unwrap_or(redirect_url);
    let last = url
        .trim_end_matches('/')
        .rsplit('/')
        .find(|s| !s.is_empty())?;
    Version::parse(last)
}

/// How this copy of the binary was installed — selects the upgrade command we
/// show. We never act on it beyond printing the right hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    Homebrew,
    Cargo,
    CurlOrManual,
}

/// Pure substring classification of an executable path. Homebrew is checked
/// FIRST: a Homebrew install on Intel macOS lives under `/usr/local/Cellar/…`,
/// which also contains the curl installer's `/usr/local/bin` substring — so
/// the `/Cellar/` (and `/opt/homebrew/`) test has to win before we fall
/// through to "curl or manual".
fn classify_path(exe: &str) -> InstallMethod {
    if exe.contains("/Cellar/") || exe.contains("/opt/homebrew/") {
        InstallMethod::Homebrew
    } else if exe.contains("/.cargo/bin/") {
        InstallMethod::Cargo
    } else {
        InstallMethod::CurlOrManual
    }
}

/// Detect the install method from the executable path, then — only if the
/// path alone is inconclusive (`CurlOrManual`) — consult `$HOMEBREW_PREFIX` /
/// `$CARGO_HOME` for non-default install prefixes that wouldn't match the
/// hard-coded substrings above.
pub fn detect_install_method(exe: &Path) -> InstallMethod {
    let exe_str = exe.to_string_lossy();
    let by_path = classify_path(&exe_str);
    if by_path != InstallMethod::CurlOrManual {
        return by_path;
    }
    if let Ok(prefix) = std::env::var("HOMEBREW_PREFIX")
        && !prefix.is_empty()
        && exe_str.starts_with(&prefix)
    {
        return InstallMethod::Homebrew;
    }
    if let Ok(cargo_home) = std::env::var("CARGO_HOME")
        && !cargo_home.is_empty()
        && exe_str.starts_with(&cargo_home)
    {
        return InstallMethod::Cargo;
    }
    InstallMethod::CurlOrManual
}

/// The upgrade command to show for a given install method.
pub fn upgrade_hint(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::Homebrew => "brew upgrade turbo-bible",
        InstallMethod::Cargo => "cargo install turbo-bible --force",
        InstallMethod::CurlOrManual => "curl -fsSL turbo.bible/install.sh | sh",
    }
}

/// The full banner line, e.g. `Update available: v0.2.0 · brew upgrade turbo-bible`.
pub fn banner_text(latest: &Version, method: InstallMethod) -> String {
    format!(
        "Update available: {latest} \u{00b7} {}",
        upgrade_hint(method)
    )
}

/// Base URL of the GitHub repo for the release-latest redirect. Overridable
/// with `TB_UPDATE_CHECK_URL` so tests / local mirrors don't hit GitHub
/// (parallels `fetch::base_url`'s `TB_RELEASE_URL`).
fn update_base_url() -> String {
    if let Ok(u) = std::env::var("TB_UPDATE_CHECK_URL") {
        return u.trim_end_matches('/').to_string();
    }
    "https://github.com/mathiasror/turbo-bible".to_string()
}

/// Ask GitHub for the newest release tag by reading the `releases/latest`
/// redirect — no API call, no token, no JSON. Shells `curl` with the same
/// hardened flags as [`crate::fetch`]'s `run_curl`, plus tight connect/total
/// timeouts so a slow network can't stall the worker for long.
///
/// `curl -I -w '%{redirect_url}'` prints the `Location` the 302 points at
/// (`…/releases/tag/vX.Y.Z`) to stdout while the header body is discarded to
/// the platform null sink.
///
/// # Errors
/// `curl` missing/failed, or the redirect target didn't parse as `vX.Y.Z`.
pub fn latest_release_tag() -> Result<Version> {
    let url = format!("{}/releases/latest", update_base_url());
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            // HTTPS only on the request and on any redirect curl reports.
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "3",
            "--max-time",
            "5",
            "-I",
            "-o",
            null,
            "-w",
            "%{redirect_url}",
        ])
        .arg(url)
        .output()
        .context("spawn curl (is it installed?)")?;
    if !out.status.success() {
        return Err(anyhow!("curl exited with {}", out.status));
    }
    let redirect = String::from_utf8_lossy(&out.stdout);
    parse_tag(redirect.trim())
        .ok_or_else(|| anyhow!("could not parse a version from redirect {redirect:?}"))
}

/// The throttle/last-result cache at `~/.config/turbo-bible/update.toml`.
/// `latest_seen` lets an offline launch within the 24h window still surface a
/// previously-discovered update without re-checking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateCache {
    pub last_checked_unix: i64,
    pub latest_seen: String,
}

fn cache_path() -> Result<std::path::PathBuf> {
    Ok(paths::config_dir()?.join("update.toml"))
}

/// Read the cache, tolerating a missing or malformed file (→ `Default`). The
/// check is best-effort, so a bad cache must never be fatal.
pub fn load_cache() -> UpdateCache {
    let Ok(path) = cache_path() else {
        return UpdateCache::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the cache (creating the config dir if needed).
///
/// # Errors
/// Config dir can't be created, TOML serialization fails, or the write fails.
pub fn write_cache(cache: &UpdateCache) -> Result<()> {
    let path = cache_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, toml::to_string_pretty(cache)?)?;
    Ok(())
}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

const DAY_SECS: i64 = 86_400;

/// Whether the once-per-day window has elapsed. `last == 0` (never checked)
/// against a real `now` is always due.
pub fn should_check(last_checked_unix: i64, now: i64) -> bool {
    now - last_checked_unix >= DAY_SECS
}

/// Whether to spawn a fresh network check: enabled in config, no opt-out env
/// (`TB_NO_UPDATE_CHECK`), not in CI (`CI`), and outside the 24h window.
pub fn should_spawn(cfg_check: bool, cache: &UpdateCache, now: i64) -> bool {
    cfg_check
        && std::env::var_os("TB_NO_UPDATE_CHECK").is_none()
        && std::env::var_os("CI").is_none()
        && should_check(cache.last_checked_unix, now)
}

/// Banner text seeded from the cache (offline-graceful): `Some` when the
/// last-seen version parses and is newer than the running one.
pub fn cached_banner(
    cache: &UpdateCache,
    current: Version,
    method: InstallMethod,
) -> Option<String> {
    let seen = Version::parse(&cache.latest_seen)?;
    is_newer(seen, current).then(|| banner_text(&seen, method))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_accepts_plain_and_v_prefixed() {
        assert_eq!(Version::parse("0.2.0"), Version::parse("v0.2.0"));
        assert_eq!(Version::parse("V1.0.0"), Version::parse("1.0.0"));
        assert!(Version::parse("0.2.0").is_some());
    }

    #[test]
    fn version_parse_rejects_malformed() {
        assert!(Version::parse("0.2").is_none(), "two-part");
        assert!(Version::parse("0.2.0.1").is_none(), "four-part");
        assert!(Version::parse("v0.2.0-rc1").is_none(), "pre-release suffix");
        assert!(Version::parse("").is_none());
        assert!(Version::parse("vabc").is_none());
    }

    #[test]
    fn ordering_is_numeric_not_lexical() {
        let a = Version::parse("0.10.0").unwrap();
        let b = Version::parse("0.9.0").unwrap();
        assert!(
            is_newer(a, b),
            "0.10.0 must beat 0.9.0 (numeric, not string)"
        );
        assert!(!is_newer(b, a));
        let v = Version::parse("1.2.3").unwrap();
        assert!(!is_newer(v, v), "equal is not newer");
    }

    #[test]
    fn display_round_trips_with_v_prefix() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "v1.2.3");
    }

    #[test]
    fn current_version_parses() {
        assert!(Version::current().is_some(), "CARGO_PKG_VERSION must parse");
    }

    #[test]
    fn parse_tag_from_release_redirect() {
        assert_eq!(
            parse_tag("https://github.com/mathiasror/turbo-bible/releases/tag/v0.2.0"),
            Version::parse("0.2.0")
        );
        assert_eq!(
            parse_tag("https://example.test/releases/tag/v1.4.9?foo=bar#frag"),
            Version::parse("1.4.9")
        );
        assert!(parse_tag("https://github.com/owner/repo/releases").is_none());
        assert!(parse_tag("").is_none());
    }

    #[test]
    fn classify_path_matrix() {
        // Homebrew wins over the /usr/local/bin curl-installer substring.
        assert_eq!(
            classify_path("/usr/local/Cellar/turbo-bible/0.1.0/bin/turbo-bible"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify_path("/opt/homebrew/bin/turbo-bible"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify_path("/home/me/.cargo/bin/turbo-bible"),
            InstallMethod::Cargo
        );
        assert_eq!(
            classify_path("/home/me/.local/bin/turbo-bible"),
            InstallMethod::CurlOrManual
        );
        assert_eq!(
            classify_path("/usr/local/bin/turbo-bible"),
            InstallMethod::CurlOrManual
        );
    }

    #[test]
    fn upgrade_hints_match_install_methods() {
        assert_eq!(
            upgrade_hint(InstallMethod::Homebrew),
            "brew upgrade turbo-bible"
        );
        assert_eq!(
            upgrade_hint(InstallMethod::Cargo),
            "cargo install turbo-bible --force"
        );
        assert_eq!(
            upgrade_hint(InstallMethod::CurlOrManual),
            "curl -fsSL turbo.bible/install.sh | sh"
        );
    }

    #[test]
    fn banner_text_format() {
        let v = Version::parse("0.2.0").unwrap();
        assert_eq!(
            banner_text(&v, InstallMethod::Homebrew),
            "Update available: v0.2.0 \u{00b7} brew upgrade turbo-bible"
        );
    }

    #[test]
    fn should_check_window() {
        assert!(should_check(0, DAY_SECS), "never-checked is due");
        assert!(should_check(1_000, 1_000 + DAY_SECS), "exactly 24h is due");
        assert!(!should_check(1_000, 1_000 + DAY_SECS - 1), "within window");
    }

    #[test]
    fn cached_banner_only_when_newer() {
        let current = Version::parse("0.1.0").unwrap();
        let newer = UpdateCache {
            last_checked_unix: 1,
            latest_seen: "0.2.0".to_string(),
        };
        assert!(cached_banner(&newer, current, InstallMethod::Cargo).is_some());

        let same = UpdateCache {
            last_checked_unix: 1,
            latest_seen: "0.1.0".to_string(),
        };
        assert!(cached_banner(&same, current, InstallMethod::Cargo).is_none());

        let empty = UpdateCache::default();
        assert!(cached_banner(&empty, current, InstallMethod::Cargo).is_none());
    }
}
