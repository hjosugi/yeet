use crate::i18n::Language;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

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
    pub autostart: bool,
    pub strip_size: i32,
    pub theme: Theme,
    pub language: Language,
    pub reduced_motion: bool,
    pub edge: ScreenEdge,
    pub disabled_outputs: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_hide: true,
            restore_shelf: true,
            autostart: false,
            strip_size: 6,
            theme: Theme::System,
            language: Language::System,
            reduced_motion: false,
            edge: ScreenEdge::Right,
            disabled_outputs: Vec::new(),
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
        settings.strip_size = settings.strip_size.clamp(3, 16);
        settings
    }

    pub fn save(&self) -> io::Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
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
        assert_eq!(settings.strip_size, 6);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.language, Language::System);
        assert!(!settings.reduced_motion);
        assert_eq!(settings.edge, ScreenEdge::Right);
        assert!(settings.disabled_outputs.is_empty());
    }

    #[test]
    fn missing_fields_use_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"auto_hide":false}"#).unwrap();
        assert!(!settings.auto_hide);
        assert!(settings.restore_shelf);
        assert_eq!(settings.strip_size, 6);
    }
}
