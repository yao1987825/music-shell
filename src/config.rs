use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const APP_DIR: &str = "music-shell";
pub const CONFIG_ENV: &str = "MUSIC_SHELL_CONFIG";

pub fn app_dir() -> PathBuf {
    config_root().join(APP_DIR)
}

pub fn config_root() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir;
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return dir;
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
}

pub fn default_path() -> PathBuf {
    match std::env::var_os(CONFIG_ENV) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => app_dir().join("config.json"),
    }
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
        let cfg = cfg.normalized();
        cfg.check_shape()?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let checked = self.validated()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&checked).context("序列化配置失败")?;
        std::fs::write(path, format!("{text}\n"))
            .with_context(|| format!("写入配置失败: {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validated().map(|_| ())
    }

    /// 只校验配置本身的形状，不碰文件系统。加载和写回都该过这一关。
    pub fn check_shape(&self) -> Result<()> {
        if self.library_path.as_os_str().is_empty() {
            anyhow::bail!("library_path 不能为空");
        }
        if self.playlists_dir.as_os_str().is_empty() {
            anyhow::bail!("playlists_dir 不能为空");
        }
        Ok(())
    }

    /// 写回前的完整校验：形状 + 文件系统状态。返回规范化后的副本。
    pub fn validated(&self) -> Result<Self> {
        let cfg = self.normalized();
        cfg.check_shape()?;
        if !cfg.library_path.exists() {
            std::fs::create_dir_all(&cfg.library_path).with_context(|| {
                format!(
                    "library_path 不存在且无法创建: {}",
                    cfg.library_path.display()
                )
            })?;
        }
        if !cfg.library_path.is_dir() {
            anyhow::bail!("library_path 不是目录: {}", cfg.library_path.display());
        }
        if !cfg.playlists_dir.exists() {
            std::fs::create_dir_all(&cfg.playlists_dir).with_context(|| {
                format!(
                    "playlists_dir 不存在且无法创建: {}",
                    cfg.playlists_dir.display()
                )
            })?;
        }
        if !cfg.playlists_dir.is_dir() {
            anyhow::bail!("playlists_dir 不是目录: {}", cfg.playlists_dir.display());
        }
        Ok(cfg)
    }

    pub fn volume_f32(&self) -> f32 {
        f32::from(self.volume) / 100.0
    }

    pub fn set_volume_f32(&mut self, value: f32) {
        self.volume = (value.clamp(0.0, 1.0) * 100.0).round() as u8;
    }

    pub fn normalized(&self) -> Self {
        let mut cfg = self.clone();
        cfg.volume = cfg.volume.min(100);
        cfg.window.width = cfg.window.width.clamp(40, 200);
        cfg.window.height = cfg.window.height.clamp(10, 100);
        if cfg.app_name.trim().is_empty() {
            cfg.app_name = APP_DIR.to_string();
        }
        cfg
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

#[cfg(test)]
mod validation_tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("music_shell_cfg_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validation_creates_missing_directories() {
        let root = temp_dir("create");
        let lib = root.join("Music");
        let pls = root.join("playlists");
        let cfg = Config {
            library_path: lib.clone(),
            playlists_dir: pls.clone(),
            ..Config::default()
        };
        cfg.validate().unwrap();
        assert!(lib.is_dir());
        assert!(pls.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_a_path_that_is_not_a_directory() {
        let root = temp_dir("notdir");
        let file = root.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let cfg = Config {
            library_path: file,
            playlists_dir: root.join("pls"),
            ..Config::default()
        };
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("不是目录"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_empty_paths() {
        let cfg = Config {
            library_path: PathBuf::new(),
            ..Config::default()
        };
        assert!(
            cfg.validate()
                .unwrap_err()
                .to_string()
                .contains("library_path")
        );
    }

    #[test]
    fn saving_normalizes_before_writing() {
        let root = temp_dir("save");
        let path = root.join("config.json");
        let cfg = Config {
            app_name: "  ".into(),
            volume: 250,
            window: Window {
                width: 3,
                height: 9999,
            },
            library_path: root.join("Music"),
            playlists_dir: root.join("pls"),
            ..Config::default()
        };
        cfg.save(&path).unwrap();
        let back: Config = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.volume, 100);
        assert_eq!(back.app_name, APP_DIR);
        assert_eq!(back.window.width, 40);
        assert_eq!(back.window.height, 100);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn env_override_changes_the_default_config_path() {
        let dir = temp_dir("env");
        let custom = dir.join("elsewhere.json");
        unsafe { std::env::set_var(CONFIG_ENV, &custom) };
        assert_eq!(default_path(), custom);
        unsafe { std::env::remove_var(CONFIG_ENV) };
        assert!(default_path().ends_with("config.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod shape_tests {
    use super::*;

    #[test]
    fn an_empty_library_path_is_rejected_on_load() {
        let dir = std::env::temp_dir().join("music_shell_shape");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let cfg = Config {
            library_path: PathBuf::new(),
            playlists_dir: dir.join("pls"),
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        std::fs::write(&path, text).unwrap();
        let err = Config::load(&path).unwrap_err().to_string();
        assert!(err.contains("library_path"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_playlists_dir_is_rejected_on_load() {
        let cfg = Config {
            library_path: PathBuf::from("/tmp"),
            playlists_dir: PathBuf::new(),
            ..Config::default()
        };
        let err = cfg.check_shape().unwrap_err().to_string();
        assert!(err.contains("playlists_dir"), "{err}");
    }

    #[test]
    fn a_valid_config_passes_the_shape_check() {
        let cfg = Config {
            library_path: PathBuf::from("/tmp"),
            playlists_dir: PathBuf::from("/tmp"),
            ..Config::default()
        };
        cfg.check_shape().unwrap();
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        let cfg = Config {
            volume: 250,
            window: Window {
                width: 0,
                height: 999_999,
            },
            ..Config::default()
        }
        .normalized();
        assert_eq!(cfg.volume, 100);
        assert_eq!(cfg.window.width, 40);
        assert_eq!(cfg.window.height, 100);
    }
}
