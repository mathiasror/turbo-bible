//! Session state in `~/.config/turbo-bible/state.toml`.
//!
//! Holds last-position-on-quit bookkeeping only. User preferences (e.g. the
//! picker default) live in `config.toml`. v1 of this app wrote a JSON file
//! `state.json`; v2 (Slice C interim) also stored `default_translation` here.
//! Both legacy shapes are migrated on load: `default_translation` is hoisted
//! out into `Config` via [`load_with_migration`], and `nb-2024` translation
//! references are rewritten to `nb-1930` (same versification).

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use etcetera::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};

pub const LEGACY_TRANSLATION: &str = "nb-2024";
pub const REPLACEMENT_TRANSLATION: &str = "nb-1930";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub translation: String,
    pub book: String,
    pub chapter: i64,
    pub verse: i64,
}

/// Read-only shape used for migration: accepts the old `default_translation`
/// field so we can hoist it into `config.toml`.
#[derive(Debug, Deserialize)]
struct LegacyState {
    translation: String,
    book: String,
    chapter: i64,
    verse: i64,
    #[serde(default)]
    default_translation: Option<String>,
}

fn config_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.config_dir();
    p.push("turbo-bible");
    Ok(p)
}

fn state_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("state.toml");
    Ok(p)
}

fn legacy_state_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("state.json");
    Ok(p)
}

fn parse_any(txt: &str) -> Option<LegacyState> {
    toml::from_str::<LegacyState>(txt)
        .ok()
        .or_else(|| serde_json::from_str::<LegacyState>(txt).ok())
}

/// Load state, hoist a legacy `default_translation` value into `config` if
/// present, and return both. Callers should `save()` both after handling so
/// the on-disk format converges.
pub fn load_with_migration() -> (Option<PersistedState>, Config) {
    let mut config = config::load();

    let raw = state_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .or_else(|| {
            legacy_state_path()
                .ok()
                .and_then(|p| fs::read_to_string(p).ok())
        });
    let Some(txt) = raw else { return (None, config) };
    let Some(legacy) = parse_any(&txt) else { return (None, config) };

    // Hoist default_translation out of state. Don't clobber an existing config
    // value — config wins, because the user may have edited it deliberately.
    if config.default_translation.is_none()
        && let Some(t) = legacy.default_translation
    {
        config.default_translation = Some(rename_legacy_str(t));
    }

    let mut state = PersistedState {
        translation: rename_legacy_str(legacy.translation),
        book: legacy.book,
        chapter: legacy.chapter,
        verse: legacy.verse,
    };
    if let Some(t) = &config.default_translation
        && t == LEGACY_TRANSLATION
    {
        config.default_translation = Some(REPLACEMENT_TRANSLATION.into());
    }
    rename_legacy(&mut state);
    (Some(state), config)
}

fn rename_legacy_str(s: String) -> String {
    if s == LEGACY_TRANSLATION {
        REPLACEMENT_TRANSLATION.into()
    } else {
        s
    }
}

fn rename_legacy(s: &mut PersistedState) {
    if s.translation == LEGACY_TRANSLATION {
        s.translation = REPLACEMENT_TRANSLATION.into();
    }
}

pub fn save(state: &PersistedState) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = state_path()?;
    let txt = toml::to_string_pretty(state)?;
    fs::write(path, txt)?;
    // Drop the legacy JSON file once we've safely written the new TOML.
    if let Ok(legacy) = legacy_state_path() {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}
