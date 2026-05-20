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
use etcetera::{BaseStrategy, choose_base_strategy};
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
    let config = config::load();
    let raw = state_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .or_else(|| {
            legacy_state_path()
                .ok()
                .and_then(|p| fs::read_to_string(p).ok())
        });
    migrate(raw.as_deref(), config)
}

/// Pure migration step: takes the raw state text (TOML or legacy JSON, or
/// `None` if no file existed) and an already-loaded `Config`, applies the
/// legacy-translation rename, and hoists `default_translation` out of the
/// state file if config didn't carry one. Split out for unit-testing without
/// touching the filesystem.
fn migrate(raw: Option<&str>, mut config: Config) -> (Option<PersistedState>, Config) {
    let Some(txt) = raw else {
        return (None, config);
    };
    let Some(legacy) = parse_any(txt) else {
        return (None, config);
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> Config {
        Config::default()
    }

    #[test]
    fn parse_any_handles_toml() {
        let s = parse_any(
            r#"
translation = "en-kjv"
book = "JHN"
chapter = 3
verse = 16
"#,
        )
        .expect("toml should parse");
        assert_eq!(s.translation, "en-kjv");
        assert_eq!(s.book, "JHN");
        assert_eq!(s.chapter, 3);
        assert_eq!(s.verse, 16);
        assert_eq!(s.default_translation, None);
    }

    #[test]
    fn parse_any_handles_json_with_legacy_default_translation() {
        // The Slice-C interim format put default_translation in state.json.
        let s = parse_any(
            r#"{
                "translation": "nb-2024",
                "book": "MRK",
                "chapter": 1,
                "verse": 1,
                "default_translation": "nb-2024"
            }"#,
        )
        .expect("json should parse");
        assert_eq!(s.translation, "nb-2024");
        assert_eq!(s.default_translation.as_deref(), Some("nb-2024"));
    }

    #[test]
    fn parse_any_returns_none_on_garbage() {
        assert!(parse_any("totally not a state file").is_none());
        assert!(parse_any("{ unbalanced").is_none());
    }

    #[test]
    fn parse_any_returns_none_on_missing_fields() {
        // Missing required `verse` field — neither TOML nor JSON parser accepts.
        assert!(
            parse_any(
                r#"translation = "en-kjv"
book = "JHN"
chapter = 3
"#
            )
            .is_none()
        );
    }

    #[test]
    fn rename_legacy_str_rewrites_nb2024() {
        assert_eq!(rename_legacy_str("nb-2024".into()), "nb-1930");
    }

    #[test]
    fn rename_legacy_str_passes_through_other_translations() {
        assert_eq!(rename_legacy_str("en-kjv".into()), "en-kjv");
        assert_eq!(rename_legacy_str("es-rv1909".into()), "es-rv1909");
    }

    #[test]
    fn migrate_returns_none_when_no_file_present() {
        let (state, cfg) = migrate(None, empty_config());
        assert!(state.is_none());
        assert_eq!(cfg.default_translation, None);
    }

    #[test]
    fn migrate_silently_drops_garbage_state() {
        // A malformed state.toml should not poison the loaded config —
        // load_with_migration returns (None, config) so the binary launches.
        let (state, _) = migrate(Some("not valid state"), empty_config());
        assert!(state.is_none());
    }

    #[test]
    fn migrate_renames_nb2024_in_state_and_default() {
        let raw = r#"{
            "translation": "nb-2024",
            "book": "MRK",
            "chapter": 1,
            "verse": 1,
            "default_translation": "nb-2024"
        }"#;
        let (state, cfg) = migrate(Some(raw), empty_config());
        let s = state.expect("state should parse");
        assert_eq!(s.translation, "nb-1930");
        assert_eq!(cfg.default_translation.as_deref(), Some("nb-1930"));
    }

    #[test]
    fn migrate_does_not_clobber_existing_config_default_translation() {
        // If the user has set default_translation in config.toml, the legacy
        // value from state.json must NOT win — user intent in config wins.
        let cfg = Config {
            default_translation: Some("es-rv1909".into()),
            ..Config::default()
        };
        let raw = r#"{
            "translation": "en-kjv",
            "book": "GEN",
            "chapter": 1,
            "verse": 1,
            "default_translation": "nb-2024"
        }"#;
        let (_, cfg_after) = migrate(Some(raw), cfg);
        assert_eq!(cfg_after.default_translation.as_deref(), Some("es-rv1909"));
    }

    #[test]
    fn migrate_rewrites_pre_existing_nb2024_in_config() {
        // If config carries the legacy default already, migration rewrites it.
        let cfg = Config {
            default_translation: Some("nb-2024".into()),
            ..Config::default()
        };
        let raw = r#"
translation = "en-kjv"
book = "GEN"
chapter = 1
verse = 1
"#;
        let (_, cfg_after) = migrate(Some(raw), cfg);
        assert_eq!(cfg_after.default_translation.as_deref(), Some("nb-1930"));
    }
}
