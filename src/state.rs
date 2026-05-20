//! Persistent state in `~/.config/turbo-bible/state.json`.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use etcetera::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub translation: String,
    pub book: String,
    pub chapter: i64,
    pub verse: i64,
}

fn config_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.config_dir();
    p.push("turbo-bible");
    Ok(p)
}

fn state_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("state.json");
    Ok(p)
}

pub fn load() -> Option<PersistedState> {
    let path = state_path().ok()?;
    let txt = fs::read_to_string(path).ok()?;
    serde_json::from_str(&txt).ok()
}

pub fn save(state: &PersistedState) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = state_path()?;
    let txt = serde_json::to_string_pretty(state)?;
    fs::write(path, txt)?;
    Ok(())
}
