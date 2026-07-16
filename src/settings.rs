use crate::i18n::Language;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

pub const DEFAULT_GLOBAL_HOTKEY: &str = "Ctrl+Alt+Y";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotkeyBinding {
    normalized: String,
    modifiers: u32,
    virtual_key: u32,
}

impl HotkeyBinding {
    const ALT: u32 = 0x0001;
    const CONTROL: u32 = 0x0002;
    const SHIFT: u32 = 0x0004;
    const WIN: u32 = 0x0008;

    pub fn parse(input: &str) -> Result<Self, HotkeyParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(HotkeyParseError::Empty);
        }

        let mut modifiers = 0;
        let mut key = None;
        for part in input.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err(HotkeyParseError::EmptySegment);
            }
            let modifier = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(Self::CONTROL),
                "alt" => Some(Self::ALT),
                "shift" => Some(Self::SHIFT),
                "win" | "super" | "meta" => Some(Self::WIN),
                _ => None,
            };
            if let Some(modifier) = modifier {
                if modifiers & modifier != 0 {
                    return Err(HotkeyParseError::DuplicateModifier(part.to_owned()));
                }
                modifiers |= modifier;
                continue;
            }

            if key.is_some() {
                return Err(HotkeyParseError::MultipleKeys);
            }
            key = Some(parse_key(part)?);
        }

        if modifiers == 0 {
            return Err(HotkeyParseError::MissingModifier);
        }
        let (key_name, virtual_key) = key.ok_or(HotkeyParseError::MissingKey)?;
        let mut names = Vec::with_capacity(5);
        if modifiers & Self::CONTROL != 0 {
            names.push("Ctrl");
        }
        if modifiers & Self::ALT != 0 {
            names.push("Alt");
        }
        if modifiers & Self::SHIFT != 0 {
            names.push("Shift");
        }
        if modifiers & Self::WIN != 0 {
            names.push("Win");
        }
        names.push(&key_name);

        Ok(Self {
            normalized: names.join("+"),
            modifiers,
            virtual_key,
        })
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    pub fn modifier_mask(&self) -> u32 {
        self.modifiers
    }

    pub fn virtual_key(&self) -> u32 {
        self.virtual_key
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HotkeyParseError {
    Empty,
    EmptySegment,
    MissingModifier,
    MissingKey,
    MultipleKeys,
    DuplicateModifier(String),
    UnsupportedKey(String),
}

impl std::fmt::Display for HotkeyParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("enter a shortcut"),
            Self::EmptySegment => formatter.write_str("a shortcut part is empty"),
            Self::MissingModifier => formatter.write_str("include Ctrl, Alt, Shift, or Win"),
            Self::MissingKey => formatter.write_str("include one non-modifier key"),
            Self::MultipleKeys => formatter.write_str("use exactly one non-modifier key"),
            Self::DuplicateModifier(modifier) => {
                write!(formatter, "modifier {modifier} is repeated")
            }
            Self::UnsupportedKey(key) => write!(formatter, "key {key} is not supported"),
        }
    }
}

fn parse_key(input: &str) -> Result<(String, u32), HotkeyParseError> {
    if input.len() == 1 {
        let key = input.as_bytes()[0];
        if key.is_ascii_alphanumeric() {
            let key = key.to_ascii_uppercase();
            return Ok(((key as char).to_string(), u32::from(key)));
        }
    }

    let uppercase = input.to_ascii_uppercase();
    if let Some(number) = uppercase.strip_prefix('F')
        && let Ok(number) = number.parse::<u32>()
        && (1..=24).contains(&number)
    {
        return Ok((format!("F{number}"), 0x70 + number - 1));
    }

    let (name, virtual_key) = match uppercase.as_str() {
        "SPACE" => ("Space", 0x20),
        "TAB" => ("Tab", 0x09),
        "ENTER" | "RETURN" => ("Enter", 0x0D),
        "ESC" | "ESCAPE" => ("Escape", 0x1B),
        "LEFT" => ("Left", 0x25),
        "UP" => ("Up", 0x26),
        "RIGHT" => ("Right", 0x27),
        "DOWN" => ("Down", 0x28),
        "HOME" => ("Home", 0x24),
        "END" => ("End", 0x23),
        "PAGEUP" => ("PageUp", 0x21),
        "PAGEDOWN" => ("PageDown", 0x22),
        "INSERT" => ("Insert", 0x2D),
        "DELETE" | "DEL" => ("Delete", 0x2E),
        _ => return Err(HotkeyParseError::UnsupportedKey(input.to_owned())),
    };
    Ok((name.to_owned(), virtual_key))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenEdge {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub auto_hide: bool,
    pub restore_shelf: bool,
    pub deduplicate_items: bool,
    pub stack_multi_drop: bool,
    pub autostart: bool,
    pub strip_size: i32,
    pub theme: Theme,
    pub language: Language,
    pub reduced_motion: bool,
    pub edge: ScreenEdge,
    pub disabled_outputs: Vec<String>,
    pub global_hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_hide: true,
            restore_shelf: true,
            deduplicate_items: true,
            stack_multi_drop: false,
            autostart: false,
            strip_size: 6,
            theme: Theme::System,
            language: Language::System,
            reduced_motion: false,
            edge: ScreenEdge::Right,
            disabled_outputs: Vec::new(),
            global_hotkey: DEFAULT_GLOBAL_HOTKEY.to_owned(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        let Ok(data) = fs::read(path) else {
            return Self::default();
        };
        let mut settings: Self = serde_json::from_slice(&data).unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn normalize(&mut self) {
        self.strip_size = self.strip_size.clamp(3, 16);
        self.global_hotkey = HotkeyBinding::parse(&self.global_hotkey)
            .map(|binding| binding.normalized().to_owned())
            .unwrap_or_else(|_| DEFAULT_GLOBAL_HOTKEY.to_owned());

        let mut seen = HashSet::new();
        self.disabled_outputs = self
            .disabled_outputs
            .drain(..)
            .map(|output| output.trim().to_owned())
            .filter(|output| !output.is_empty() && seen.insert(output.clone()))
            .collect();
    }

    pub fn save(&self) -> io::Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let mut normalized = self.clone();
        normalized.normalize();
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&normalized).map_err(io::Error::other)?,
        )?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::rename(temporary, path)
    }
}

fn settings_path() -> PathBuf {
    ProjectDirs::from("io", "hjosugi", "Yeet")
        .map(|dirs| dirs.config_dir().join("settings.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("yeet/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let settings = Settings::default();
        assert!(settings.auto_hide);
        assert!(settings.restore_shelf);
        assert!(settings.deduplicate_items);
        assert!(!settings.stack_multi_drop);
        assert_eq!(settings.strip_size, 6);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.language, Language::System);
        assert!(!settings.reduced_motion);
        assert_eq!(settings.edge, ScreenEdge::Right);
        assert!(settings.disabled_outputs.is_empty());
        assert_eq!(settings.global_hotkey, DEFAULT_GLOBAL_HOTKEY);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"auto_hide":false}"#).unwrap();
        assert!(!settings.auto_hide);
        assert!(settings.restore_shelf);
        assert!(settings.deduplicate_items);
        assert!(!settings.stack_multi_drop);
        assert_eq!(settings.strip_size, 6);
        assert_eq!(settings.global_hotkey, DEFAULT_GLOBAL_HOTKEY);
    }

    #[test]
    fn normalize_clamps_and_cleans_user_editable_values() {
        let mut settings = Settings {
            strip_size: 200,
            global_hotkey: " shift + control + f12 ".to_owned(),
            disabled_outputs: vec![
                " DP-1 ".to_owned(),
                String::new(),
                "HDMI-A-1".to_owned(),
                "DP-1".to_owned(),
                "   ".to_owned(),
            ],
            ..Settings::default()
        };

        settings.normalize();

        assert_eq!(settings.strip_size, 16);
        assert_eq!(settings.global_hotkey, "Ctrl+Shift+F12");
        assert_eq!(settings.disabled_outputs, ["DP-1", "HDMI-A-1"]);
    }

    #[test]
    fn normalize_falls_back_from_an_invalid_hotkey() {
        let mut settings = Settings {
            strip_size: -40,
            global_hotkey: "Y".to_owned(),
            ..Settings::default()
        };

        settings.normalize();

        assert_eq!(settings.strip_size, 3);
        assert_eq!(settings.global_hotkey, DEFAULT_GLOBAL_HOTKEY);
    }

    #[test]
    fn legacy_schema_migrates_to_new_defaults_without_losing_values() {
        let legacy = r#"{
            "auto_hide": false,
            "restore_shelf": false,
            "strip_size": 99,
            "edge": "left",
            "disabled_outputs": [" DP-1 ", "DP-1"]
        }"#;
        let mut settings: Settings = serde_json::from_str(legacy).unwrap();

        settings.normalize();

        assert!(!settings.auto_hide);
        assert!(!settings.restore_shelf);
        assert!(settings.deduplicate_items);
        assert!(!settings.stack_multi_drop);
        assert_eq!(settings.edge, ScreenEdge::Left);
        assert_eq!(settings.strip_size, 16);
        assert_eq!(settings.disabled_outputs, ["DP-1"]);
        assert_eq!(settings.global_hotkey, DEFAULT_GLOBAL_HOTKEY);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.language, Language::System);
    }

    #[test]
    fn hotkeys_are_validated_and_normalized() {
        let binding = HotkeyBinding::parse(" shift + control + f12 ").unwrap();
        assert_eq!(binding.normalized(), "Ctrl+Shift+F12");
        assert_eq!(binding.modifier_mask(), 0x0002 | 0x0004);
        assert_eq!(binding.virtual_key(), 0x7B);
        assert_eq!(
            HotkeyBinding::parse("Super+PageDown").unwrap().normalized(),
            "Win+PageDown"
        );
    }

    #[test]
    fn hotkeys_reject_unsafe_or_ambiguous_input() {
        assert_eq!(
            HotkeyBinding::parse("Y").unwrap_err(),
            HotkeyParseError::MissingModifier
        );
        assert_eq!(
            HotkeyBinding::parse("Ctrl+Alt").unwrap_err(),
            HotkeyParseError::MissingKey
        );
        assert_eq!(
            HotkeyBinding::parse("Ctrl+Y+K").unwrap_err(),
            HotkeyParseError::MultipleKeys
        );
        assert_eq!(
            HotkeyBinding::parse("Ctrl++Y").unwrap_err(),
            HotkeyParseError::EmptySegment
        );
    }
}
