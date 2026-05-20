//! User preferences in `~/.config/turbo-bible/config.toml`.
//!
//! Layout:
//! ```toml
//! default_translation = "en-kjv"
//!
//! [reading]
//! two_line_verses    = true
//! show_sidebar       = true
//! show_daily_quote   = true
//! max_width          = 80
//!
//! [theme]
//! blue         = "#0000aa"
//! cyan         = "#00aaaa"
//! bright_white = "#ffffff"
//! light_grey   = "#aaaaaa"
//! dark_grey    = "#555555"
//! yellow       = "#ffff55"
//! hotkey_red   = "#aa0000"
//! black        = "#000000"
//!
//! [keys]
//! # Additive: defaults always work; entries here add extra triggers.
//! # Format: "q", "Ctrl-d", "Shift-Tab", "Alt-x", "F5", "Esc", "Enter",
//! #         "Tab", "Space", "Backspace", "Delete", "Up", "Down",
//! #         "Left", "Right", "Home", "End", "PageUp", "PageDown".
//! quit              = []
//! open_translations = []
//! ```

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use etcetera::{choose_base_strategy, BaseStrategy};
use ratatui::style::Color;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub default_translation: Option<String>,
    pub reading: ReadingConfig,
    pub theme: ThemeConfig,
    pub keys: KeysConfig,
}

// --------------------------- Reading ---------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReadingConfig {
    /// Two-line layout per verse (number on its own line) vs single-line.
    pub two_line_verses: bool,
    /// Show the chapter-outline sidebar to the right of the passage.
    pub show_sidebar: bool,
    /// Show the "verse of the day" block on the splash screen.
    pub show_daily_quote: bool,
    /// Maximum width (cols) of the reading pane; centered if terminal is wider.
    pub max_width: u16,
}

impl Default for ReadingConfig {
    fn default() -> Self {
        Self {
            two_line_verses: true,
            show_sidebar: true,
            show_daily_quote: true,
            max_width: 80,
        }
    }
}

// --------------------------- Theme ---------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub blue: HexColor,
    pub cyan: HexColor,
    pub bright_white: HexColor,
    pub light_grey: HexColor,
    pub dark_grey: HexColor,
    pub yellow: HexColor,
    pub hotkey_red: HexColor,
    pub black: HexColor,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        // Classic CGA palette (24-bit RGB), matches the original Turbo Vision
        // theme. Bump any value here to retheme.
        Self {
            blue: HexColor::new(0, 0, 170),
            cyan: HexColor::new(0, 170, 170),
            bright_white: HexColor::new(255, 255, 255),
            light_grey: HexColor::new(170, 170, 170),
            dark_grey: HexColor::new(85, 85, 85),
            yellow: HexColor::new(255, 255, 85),
            hotkey_red: HexColor::new(170, 0, 0),
            black: HexColor::new(0, 0, 0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HexColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl HexColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn to_color(self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

impl Serialize for HexColor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b))
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        let hex = raw.strip_prefix('#').unwrap_or(&raw);
        if hex.len() != 6 {
            return Err(D::Error::custom(format!(
                "expected 6-digit hex color, got {raw:?}"
            )));
        }
        let parse = |s: &str| u8::from_str_radix(s, 16).map_err(D::Error::custom);
        Ok(HexColor {
            r: parse(&hex[0..2])?,
            g: parse(&hex[2..4])?,
            b: parse(&hex[4..6])?,
        })
    }
}

// --------------------------- Keys ---------------------------

/// Extra triggers per action. The hardcoded vim-style defaults always work;
/// these are additive. Multi-key chords (`gg`, `[b`, `]b`, `ZZ`) and the
/// count prefix stay hardcoded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeysConfig {
    pub quit: Vec<KeyBind>,
    pub back: Vec<KeyBind>,
    pub open_goto: Vec<KeyBind>,
    pub open_find: Vec<KeyBind>,
    pub open_help: Vec<KeyBind>,
    pub open_footnote: Vec<KeyBind>,
    pub open_menu: Vec<KeyBind>,
    pub open_bookmarks: Vec<KeyBind>,
    pub open_translations: Vec<KeyBind>,
    pub copy_verse: Vec<KeyBind>,
    pub toggle_sidebar: Vec<KeyBind>,
    pub toggle_visual: Vec<KeyBind>,
    pub toggle_verse_layout: Vec<KeyBind>,
    pub add_bookmark: Vec<KeyBind>,
    pub jump_back: Vec<KeyBind>,
    pub jump_forward: Vec<KeyBind>,
    pub prev_chapter: Vec<KeyBind>,
    pub next_chapter: Vec<KeyBind>,
    pub cursor_down: Vec<KeyBind>,
    pub cursor_up: Vec<KeyBind>,
    pub half_page_down: Vec<KeyBind>,
    pub half_page_up: Vec<KeyBind>,
    pub page_down: Vec<KeyBind>,
    pub page_up: Vec<KeyBind>,
    pub goto_top: Vec<KeyBind>,
    pub goto_bottom: Vec<KeyBind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBind {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBind {
    pub fn matches(self, ev: &KeyEvent) -> bool {
        // Normalize SHIFT for printable chars: a literal "Q" is Char('Q') with
        // SHIFT on some terminals and without on others — treat as equivalent.
        let mut want = self.modifiers;
        let mut got = ev.modifiers;
        if let (KeyCode::Char(c1), KeyCode::Char(c2)) = (self.code, ev.code)
            && (c1.is_ascii_uppercase() || c2.is_ascii_uppercase())
        {
            want.remove(KeyModifiers::SHIFT);
            got.remove(KeyModifiers::SHIFT);
        }
        self.code == ev.code && want == got
    }
}

impl Serialize for KeyBind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&key_to_string(self))
    }
}

impl<'de> Deserialize<'de> for KeyBind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        parse_key(&raw).map_err(D::Error::custom)
    }
}

fn key_to_string(k: &KeyBind) -> String {
    let mut parts: Vec<String> = Vec::new();
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".into());
    }
    if k.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".into());
    }
    if k.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".into());
    }
    parts.push(match k.code {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        other => format!("{other:?}"),
    });
    parts.join("-")
}

fn parse_key(raw: &str) -> Result<KeyBind, String> {
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.is_empty() {
        return Err("empty key string".into());
    }
    let mut modifiers = KeyModifiers::empty();
    for m in &parts[..parts.len() - 1] {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "c" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "meta" | "a" => modifiers |= KeyModifiers::ALT,
            "shift" | "s" => modifiers |= KeyModifiers::SHIFT,
            other => return Err(format!("unknown modifier {other:?}")),
        }
    }
    let last = parts[parts.len() - 1];
    let code = match last {
        "" => return Err(format!("missing key in {raw:?}")),
        "Space" => KeyCode::Char(' '),
        "Esc" | "Escape" => KeyCode::Esc,
        "Enter" | "Return" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" | "Del" => KeyCode::Delete,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" | "PgUp" => KeyCode::PageUp,
        "PageDown" | "PgDn" => KeyCode::PageDown,
        s if s.starts_with('F') => {
            let n: u8 = s[1..]
                .parse()
                .map_err(|_| format!("invalid function key {s:?}"))?;
            KeyCode::F(n)
        }
        s => {
            let mut chars = s.chars();
            let first = chars
                .next()
                .ok_or_else(|| format!("invalid key {raw:?}"))?;
            if chars.next().is_some() {
                return Err(format!("unknown key name {s:?}"));
            }
            KeyCode::Char(first)
        }
    };
    Ok(KeyBind { code, modifiers })
}

// --------------------------- File IO ---------------------------

fn config_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.config_dir();
    p.push("turbo-bible");
    Ok(p)
}

fn config_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("config.toml");
    Ok(p)
}

pub fn load() -> Config {
    let Ok(path) = config_path() else { return Config::default() };
    let Ok(txt) = fs::read_to_string(path) else { return Config::default() };
    toml::from_str(&txt).unwrap_or_else(|e| {
        eprintln!("config.toml: {e}; using defaults");
        Config::default()
    })
}

pub fn save(cfg: &Config) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = config_path()?;
    let txt = toml::to_string_pretty(cfg)?;
    fs::write(path, txt)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_color_with_and_without_hash() {
        let with: HexColor = toml::from_str("c = \"#0000aa\"\n")
            .map(|v: toml::Value| v["c"].clone().try_into::<HexColor>().unwrap())
            .unwrap();
        assert_eq!((with.r, with.g, with.b), (0, 0, 0xaa));
    }

    #[test]
    fn parses_keybind_strings() {
        assert_eq!(
            parse_key("q").unwrap(),
            KeyBind { code: KeyCode::Char('q'), modifiers: KeyModifiers::empty() }
        );
        assert_eq!(
            parse_key("Ctrl-d").unwrap(),
            KeyBind { code: KeyCode::Char('d'), modifiers: KeyModifiers::CONTROL }
        );
        assert_eq!(parse_key("F5").unwrap().code, KeyCode::F(5));
        assert_eq!(parse_key("Esc").unwrap().code, KeyCode::Esc);
        assert_eq!(parse_key("Space").unwrap().code, KeyCode::Char(' '));
    }

    #[test]
    fn round_trips_through_toml() {
        let cfg = Config {
            default_translation: Some("en-kjv".into()),
            theme: ThemeConfig {
                blue: HexColor::new(1, 2, 3),
                ..ThemeConfig::default()
            },
            keys: KeysConfig {
                quit: vec![parse_key("Ctrl-q").unwrap()],
                ..KeysConfig::default()
            },
            ..Config::default()
        };
        let txt = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&txt).unwrap();
        assert_eq!(back.theme.blue.r, 1);
        assert_eq!(back.keys.quit.len(), 1);
        assert_eq!(back.default_translation.as_deref(), Some("en-kjv"));
    }

    #[test]
    fn dump_default_config() {
        let cfg = Config::default();
        let txt = toml::to_string_pretty(&cfg).unwrap();
        // For visual inspection during development.
        eprintln!("---\n{txt}\n---");
        assert!(txt.contains("[theme]"));
        assert!(txt.contains("[reading]"));
        assert!(txt.contains("[keys]"));
    }
}
