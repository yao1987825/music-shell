use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::capability::Capability;
use crate::command::Command;
use crate::config::{Config, Repeat};
use crate::library::{self, Library, Playlist, Track};
use crate::player::{Player, State};
use crate::renderer::{self, Renderer, Style, View};

pub const TICK: Duration = Duration::from_millis(200);

pub struct App {
    cfg: Config,
    config_path: PathBuf,
    lib: Library,
    player: Player,
    source: String,
    status: String,
    renderer: Renderer,
    style: Style,
}

impl App {
    pub fn new(cfg: Config, config_path: PathBuf, player: Player) -> Self {
        let mut app = Self {
            cfg,
            config_path,
            lib: Library::default(),
            player,
            source: String::new(),
            status: String::new(),
            renderer: Renderer::default(),
            style: Style::detect(),
        };
        app.lib.set_shuffle(app.cfg.shuffle);
        app.relabel();
        app
    }

    fn relabel(&mut self) {
        if self.source.is_empty() {
            self.source = format!(
                "{} · {} 首 · 随机 {} · 循环 {}",
                self.cfg.library_path.display(),
                self.lib.len(),
                renderer::onoff(self.cfg.shuffle),
                self.cfg.repeat.label()
            );
        }
    }

    pub fn scan_library(&mut self) {
        let path = self.cfg.library_path.clone();
        let result = library::scan_with_options(&path, library::ScanOptions::default());
        let count = result.tracks.len();
        let unsupported = result.unsupported.len();
        self.lib.set_tracks(result.tracks);
        self.source = format!("{} · {count} 首", path.display());
        self.status = match (count, unsupported) {
            (0, 0) => format!("{} 下没有找到音频文件", path.display()),
            (0, n) => format!("{} 下的 {n} 个文件都放不了（格式不支持）", path.display()),
            (c, 0) => format!("已扫描 {c} 首曲目"),
            (c, n) => format!("已扫描 {c} 首曲目，{n} 个文件格式不支持"),
        };
    }

    pub fn play_cursor(&mut self) {
        let mut skipped: Vec<String> = Vec::new();
        let total = self.lib.len().max(1);
        for _ in 0..total {
            let Some(track) = self.lib.current().cloned() else {
                break;
            };
            match self.player.play(&track) {
                Ok(()) => {
                    self.status = if skipped.is_empty() {
                        format!("正在播放 {}", track.title)
                    } else {
                        format!(
                            "正在播放 {}（跳过 {} 首无法解码：{}）",
                            track.title,
                            skipped.len(),
                            skipped.join("、")
                        )
                    };
                    return;
                }
                Err(err) => {
                    skipped.push(describe_failure(&track, &err));
                    if self.lib.len() <= 1 || self.lib.advance().is_none() {
                        break;
                    }
                }
            }
        }
        self.status = if skipped.is_empty() {
            "播放列表为空".into()
        } else {
            format!("全部无法播放：{}", skipped.join("、"))
        };
    }

    pub fn toggle_play(&mut self) {
        match self.player.state() {
            State::Stopped => self.play_cursor(),
            _ => {
                self.player.toggle_pause();
                self.status = format!("状态 {}", self.player.state().label());
            }
        }
    }

    pub fn on_track_end(&mut self) -> bool {
        if self.cfg.repeat == Repeat::One {
            self.play_cursor();
            return true;
        }
        let at_end = self
            .lib
            .position()
            .map(|p| p + 1 >= self.lib.len())
            .unwrap_or(true);
        if at_end && !self.cfg.shuffle && self.cfg.repeat == Repeat::Off {
            self.player.stop();
            self.status = "播放结束".into();
            return false;
        }
        if self.lib.advance().is_some() {
            self.play_cursor();
            true
        } else {
            self.player.stop();
            self.status = "播放结束".into();
            false
        }
    }

    pub fn adjust_volume(&mut self, delta: i32) {
        let next = i32::from(self.player.volume_percent()) + delta;
        self.player
            .set_volume(f32::from(next.clamp(0, 100) as u8) / 100.0);
        self.cfg.set_volume_f32(self.player.volume());
        self.status = format!("音量 {}%", self.player.volume_percent());
    }

    pub fn toggle_shuffle(&mut self) {
        self.cfg.shuffle = !self.cfg.shuffle;
        self.lib.set_shuffle(self.cfg.shuffle);
        self.source = format!("{} · {} 首", self.source_base(), self.lib.len());
        self.status = format!("随机播放 {}", renderer::onoff(self.cfg.shuffle));
    }

    pub fn cycle_repeat(&mut self) {
        self.cfg.repeat = self.cfg.repeat.next();
        self.source = format!("{} · {} 首", self.source_base(), self.lib.len());
        self.status = format!("循环模式 {}", self.cfg.repeat.label());
    }

    pub fn source_base(&self) -> String {
        self.source
            .split(" · ")
            .next()
            .unwrap_or(&self.source)
            .to_string()
    }

    pub fn load_playlist(&mut self, name: &str) -> bool {
        match Playlist::load(&self.cfg.playlists_dir, name) {
            Ok(pl) => {
                let count = pl.tracks.len();
                self.lib.set_tracks(pl.tracks);
                self.source = format!("播放列表 {name} · {count} 首");
                self.status = format!("已加载播放列表 {name}（{count} 首）");
                self.remember_playlist(name);
                true
            }
            Err(err) => {
                self.status = format!("{err}");
                false
            }
        }
    }

    pub fn save_playlist(&mut self, name: Option<&str>) -> bool {
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
            let known = Playlist::list(&self.cfg.playlists_dir);
            let hint = if known.is_empty() {
                "还没有".to_string()
            } else {
                known.join(", ")
            };
            self.status = format!("请给播放列表起名字: sv <名字>（已有: {hint}）");
            return false;
        };
        let pl = Playlist {
            name: name.to_string(),
            tracks: self.lib.tracks.clone(),
        };
        match pl.save(&self.cfg.playlists_dir) {
            Ok(path) => {
                self.status = format!("已保存 {} 首到 {}", pl.tracks.len(), path.display());
                self.remember_playlist(name);
                true
            }
            Err(err) => {
                self.status = format!("{err}");
                false
            }
        }
    }

    pub fn show_playlists(&mut self) {
        let names = Playlist::list(&self.cfg.playlists_dir);
        if names.is_empty() {
            self.status = format!(
                "{} 下还没有播放列表，用 `sv <名字>` 存一个",
                self.cfg.playlists_dir.display()
            );
            return;
        }
        let recent = self.cfg.recent_playlists.join(", ");
        self.status = format!("播放列表: {}｜最近: {}", names.join(", "), recent);
    }

    pub fn remember_playlist(&mut self, name: &str) {
        self.cfg.recent_playlists.retain(|n| n != name);
        self.cfg.recent_playlists.insert(0, name.to_string());
        self.cfg.recent_playlists.truncate(10);
    }

    pub fn reload_config(&mut self) {
        match Config::load(&self.config_path) {
            Ok(cfg) => {
                let volume = cfg.volume_f32();
                self.cfg = cfg;
                self.player.set_volume(volume);
                self.lib.set_shuffle(self.cfg.shuffle);
                self.scan_library();
                self.status = "已重新载入配置".into();
            }
            Err(err) => self.status = format!("载入配置失败: {err}"),
        }
    }

    pub fn save_config(&mut self) {
        self.cfg.set_volume_f32(self.player.volume());
        self.cfg.shuffle = self.lib.shuffle;
        let before = self.cfg.clone();
        match self.cfg.save(&self.config_path) {
            Ok(()) => {
                let after = self.cfg.normalized();
                let mut notes = Vec::new();
                if before.volume != after.volume {
                    notes.push(format!("音量 {}→{}", before.volume, after.volume));
                }
                if before.window.width != after.window.width {
                    notes.push(format!(
                        "宽度 {}→{}",
                        before.window.width, after.window.width
                    ));
                }
                if before.window.height != after.window.height {
                    notes.push(format!(
                        "高度 {}→{}",
                        before.window.height, after.window.height
                    ));
                }
                if before.app_name != after.app_name {
                    notes.push(format!(
                        "名称 \"{}\"→\"{}\"",
                        before.app_name, after.app_name
                    ));
                }
                self.status = if notes.is_empty() {
                    format!("配置已写入 {}", self.config_path.display())
                } else {
                    format!(
                        "配置已写入 {}（已修正：{}）",
                        self.config_path.display(),
                        notes.join("、")
                    )
                };
            }
            Err(err) => self.status = format!("写配置失败: {err}"),
        }
    }

    pub fn dispatch(&mut self, command: Command) -> bool {
        match command {
            Command::Quit => return false,
            Command::TogglePlay => self.toggle_play(),
            Command::Next => {
                if self.lib.advance().is_some() {
                    self.play_cursor();
                }
            }
            Command::Prev => {
                if self.lib.prev().is_some() {
                    self.play_cursor();
                }
            }
            Command::CursorDown => self.lib.move_cursor(1),
            Command::CursorUp => self.lib.move_cursor(-1),
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::PlayIndex(index) => {
                if (1..=self.lib.len()).contains(&index) {
                    if self.lib.select(index - 1).is_some() {
                        self.play_cursor();
                    }
                } else {
                    self.status = format!("序号超出范围 (1-{})", self.lib.len());
                }
            }
            Command::Rescan => self.scan_library(),
            Command::ToggleShuffle => self.toggle_shuffle(),
            Command::CycleRepeat => self.cycle_repeat(),
            Command::ReloadConfig => self.reload_config(),
            Command::SaveConfig => self.save_config(),
            Command::ListPlaylists => self.show_playlists(),
            Command::LoadPlaylist(name) => {
                self.load_playlist(&name);
            }
            Command::SavePlaylist(name) => {
                self.save_playlist(Some(&name));
            }
            Command::Help => self.status = renderer::help_text(self.cfg.theme),
            Command::Unknown(word) => {
                self.status = format!("未知命令 `{word}`，输入 ? 看帮助");
            }
        }
        true
    }

    pub fn repl(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || feed_stdin(tx));
        loop {
            self.player.tick();
            if self.player.finished() && !self.on_track_end() {
                break;
            }
            self.refresh();
            match rx.recv_timeout(TICK) {
                Ok(Some(line)) => {
                    let command = Command::parse(line.trim());
                    if !self.dispatch(command) {
                        break;
                    }
                }
                Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        self.player.stop();
        Ok(())
    }

    pub fn auto_play(&self) -> bool {
        self.cfg.auto_play
    }

    /// 渲染宽度取 config 与真实终端宽度的较小值，避免在窄终端里折行
    pub fn effective_width(&self) -> usize {
        let configured = self.cfg.window.width;
        match terminal_size::terminal_size() {
            Some((w, _)) => configured.min(usize::from(w.0)).max(20),
            None => configured,
        }
    }

    pub fn effective_height(&self) -> usize {
        let configured = self.cfg.window.height;
        match terminal_size::terminal_size() {
            Some((_, h)) => configured.min(usize::from(h.0)).max(10),
            None => configured,
        }
    }

    pub fn view(&self) -> View {
        let (title, artist) = match self.lib.current() {
            Some(t) => (t.title.clone(), t.artist.clone()),
            None => ("—".to_string(), String::new()),
        };
        let len = self.lib.len();
        let cursor = self.lib.position().unwrap_or(0);
        let rows = self.cfg.window.height.saturating_sub(11).clamp(3, 20);
        let start = cursor
            .saturating_sub(rows / 2)
            .min(len.saturating_sub(rows));
        let end = (start + rows).min(len);

        let mut list = Vec::new();
        for pos in start..end {
            if let Some(track) = self.lib.track_at(pos) {
                list.push(renderer::Row {
                    index: pos + 1,
                    title: track.title.clone(),
                    artist: track.artist.clone(),
                    active: pos == cursor,
                });
            }
        }

        View {
            app_name: self.cfg.app_name.clone(),
            theme: self.cfg.theme,
            style: self.style,
            width: self.effective_width(),
            height: self.effective_height(),
            backend: if self.player.silent {
                "静音模式"
            } else {
                "音频已连接"
            },
            title,
            artist,
            state: self.player.state().label(),
            position_secs: self.player.position().as_secs(),
            duration_secs: self.player.duration().as_secs(),
            ratio: self.player.ratio(),
            volume: self.player.volume_percent(),
            shuffle: self.cfg.shuffle,
            repeat: self.cfg.repeat.label(),
            source: self.source.clone(),
            status: self.status.clone(),
            empty_hint: len == 0,
            range: format!("第 {}-{} / {len} 首", start + 1, end),
            rows: list,
        }
    }

    pub fn refresh(&mut self) {
        let view = self.view();
        let frame = self.renderer.draw(&view);
        if !frame.is_empty() {
            print!("{frame}");
            io::stdout().flush().ok();
        }
    }
}

fn describe_failure(track: &Track, err: &anyhow::Error) -> String {
    match Capability::of(&track.path).hint(&track.path) {
        Some(hint) => hint,
        None => format!("{}（{err}）", track.title),
    }
}

fn feed_stdin(tx: Sender<Option<String>>) {
    let stdin = io::stdin();
    let mut locked = stdin.lock();
    loop {
        let mut buf = String::new();
        match locked.read_line(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = tx.send(None);
                return;
            }
            Ok(_) => {
                if tx.send(Some(buf)).is_err() {
                    return;
                }
            }
        }
    }
}
