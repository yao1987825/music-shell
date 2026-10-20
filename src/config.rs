use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const APP_DIR: &str = "music-shell";

pub fn app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

pub fn default_path() -> PathBuf {
    app_dir().join("config.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn accent(self) -> &'static str {
        match self {
            Self::Dark => "\x1b[38;5;44m",
            Self::Light => "\x1b[38;5;25m",
        }
    }

    pub fn strong(self) -> &'static str {
        match self {
            Self::Dark => "\x1b[38;5;252m",
            Self::Light => "\x1b[38;5;238m",
        }
    }

    pub fn bar(self) -> &'static str {
        match self {
            Self::Dark => "\x1b[38;5;42m",
            Self::Light => "\x1b[38;5;28m",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::One => "one",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Window {
    pub width: usize,
    pub height: usize,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub app_name: String,
    pub theme: Theme,
    pub volume: u8,
    pub library_path: PathBuf,
    pub playlists_dir: PathBuf,
    pub auto_play: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub recent_playlists: Vec<String>,
    pub window: Window,
}

impl Default for Config {
    fn default() -> Self {
        let base = app_dir();
        Self {
            app_name: APP_DIR.to_string(),
            theme: Theme::default(),
            volume: 70,
            library_path: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Music"),
            playlists_dir: base.join("playlists"),
            auto_play: false,
            shuffle: false,
            repeat: Repeat::Off,
            recent_playlists: Vec::new(),
            window: Window::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let cfg = Self::default();
            cfg.save(path)?;
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let cfg: Self = serde_json::from_str(&text)
            .with_context(|| format!("解析配置失败: {}", path.display()))?;
        Ok(cfg.normalized())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).context("序列化配置失败")?;
        std::fs::write(path, format!("{text}\n"))
            .with_context(|| format!("写入配置失败: {}", path.display()))?;
        Ok(())
    }

    pub fn volume_f32(&self) -> f32 {
        f32::from(self.volume) / 100.0
    }

    pub fn set_volume_f32(&mut self, value: f32) {
        self.volume = (value.clamp(0.0, 1.0) * 100.0).round() as u8;
    }

    pub fn normalized(mut self) -> Self {
        self.volume = self.volume.min(100);
        self.window.width = self.window.width.clamp(40, 200);
        self.window.height = self.window.height.clamp(10, 100);
        if self.app_name.trim().is_empty() {
            self.app_name = APP_DIR.to_string();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let cfg = Config::default();
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.app_name, cfg.app_name);
        assert_eq!(back.volume, cfg.volume);
        assert_eq!(back.theme, cfg.theme);
        assert_eq!(back.repeat, cfg.repeat);
        assert_eq!(back.window.width, cfg.window.width);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg: Config = serde_json::from_str(r#"{"volume": 30}"#).unwrap();
        assert_eq!(cfg.volume, 30);
        assert_eq!(cfg.app_name, APP_DIR);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = serde_json::from_str::<Config>(r#"{"volum": 30}"#).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let cfg = Config {
            volume: 250,
            app_name: "   ".into(),
            window: Window {
                width: 1,
                height: 999,
            },
            ..Config::default()
        }
        .normalized();
        assert_eq!(cfg.volume, 100);
        assert_eq!(cfg.window.width, 40);
        assert_eq!(cfg.window.height, 100);
        assert_eq!(cfg.app_name, APP_DIR);
    }

    #[test]
    fn volume_converts_both_ways() {
        let mut cfg = Config::default();
        cfg.set_volume_f32(0.42);
        assert_eq!(cfg.volume, 42);
        assert!((cfg.volume_f32() - 0.42).abs() < 0.005);
        cfg.set_volume_f32(1.9);
        assert_eq!(cfg.volume, 100);
    }

    #[test]
    fn repeat_cycles_and_labels() {
        assert_eq!(Repeat::Off.next(), Repeat::One);
        assert_eq!(Repeat::One.next(), Repeat::All);
        assert_eq!(Repeat::All.next(), Repeat::Off);
        assert_eq!(Repeat::One.label(), "one");
    }

    #[test]
    fn themes_use_distinct_accent_colors() {
        assert_ne!(Theme::Dark.accent(), Theme::Light.accent());
        assert_ne!(Theme::Dark.bar(), Theme::Light.bar());
    }
}
