use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use anyhow::Result;

use crate::capability::Capability;
use crate::command::Command;
use crate::config::{Config, Repeat};
use crate::input::{self, Input};
use crate::library::{self, Library, Playlist, Track};
use crate::player::{Player, State};
use crate::renderer::{self, Renderer, Style, View};

pub const TICK: Duration = Duration::from_millis(200);

/// 一页显示多少首曲目。左右键按页翻。
pub const PAGE_SIZE: usize = 10;

pub struct App {
    cfg: Config,
    config_path: PathBuf,
    lib: Library,
    player: Player,
    source: String,
    status: String,
    /// 正在输入、还没回车的那行命令，显示在提示符后面
    input: String,
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
            input: String::new(),
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
        if !path.exists()
            && let Err(err) = std::fs::create_dir_all(&path)
        {
            self.status = format!("无法创建音乐库目录 {}: {err}", path.display());
            return;
        }
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
                self.status = state_label(self.player.state()).to_string();
            }
        }
    }

    pub fn on_track_end(&mut self) -> bool {
        if self.cfg.repeat == Repeat::One {
            let title = self.lib.current().map(|t| t.title.clone());
            self.play_cursor();
            if let Some(title) = title {
                self.status = format!("单曲循环：{title}");
            }
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

    /// 执行命令。
    pub fn dispatch(&mut self, command: Command) {
        match command {
            Command::Quit => {}
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
            Command::CursorDown => self.move_selection(1),
            Command::CursorUp => self.move_selection(-1),
            Command::CursorHome => self.select_index(0),
            Command::CursorEnd => {
                let last = self.lib.len().saturating_sub(1);
                self.select_index(last);
            }
            Command::PageUp => {
                let page = self.page_index().saturating_sub(1);
                self.goto_page(page);
            }
            Command::PageDown => {
                let last = self.page_count().saturating_sub(1);
                let page = (self.page_index() + 1).min(last);
                self.goto_page(page);
            }
            Command::VolumeUp => self.adjust_volume(5),
            Command::VolumeDown => self.adjust_volume(-5),
            Command::PlayIndex(index) => {
                if (1..=self.lib.len()).contains(&index) {
                    self.select_index(index - 1);
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
    }

    /// 上下键：移动选中行，选中哪首就播哪首。
    /// 顶到头或底时什么都不做，不重新播放当前这首。
    fn move_selection(&mut self, delta: isize) {
        if self.lib.is_empty() {
            return;
        }
        let before = self.lib.position();
        self.lib.move_cursor(delta);
        if self.lib.position() == before {
            return;
        }
        self.play_cursor();
    }

    /// 选中第 index 首并播放
    fn select_index(&mut self, index: usize) {
        if self.lib.select(index).is_some() {
            self.play_cursor();
        }
    }

    /// 跳到第 page 页，并播放这一页的第一首
    fn goto_page(&mut self, page: usize) {
        if self.lib.is_empty() {
            return;
        }
        let index = page * PAGE_SIZE;
        if index >= self.lib.len() {
            return;
        }
        self.select_index(index);
    }

    /// 当前选中的曲目在第几页（从 0 开始）
    fn page_index(&self) -> usize {
        self.lib.position().unwrap_or(0) / PAGE_SIZE
    }

    /// 一共几页
    fn page_count(&self) -> usize {
        self.lib.len().div_ceil(PAGE_SIZE)
    }

    pub fn repl(&mut self) -> Result<()> {
        input::enable_raw_input();
        let rx = input::spawn_reader();
        loop {
            self.player.tick();
            if self.player.finished() && !self.on_track_end() {
                break;
            }
            self.refresh();

            match rx.recv_timeout(TICK) {
                Ok(Input::Eof) => break,
                Ok(item) => {
                    // 正在输入的那行文字不是命令，只是显示在提示符后面
                    let command = match item {
                        Input::Pending(text) => {
                            self.input = text;
                            continue;
                        }
                        other => other.to_command(),
                    };
                    let Some(command) = command else {
                        continue;
                    };
                    if command == Command::Quit {
                        break;
                    }
                    self.input.clear();
                    self.dispatch(command);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.player.stop();
        input::restore_terminal();
        Ok(())
    }

    /// 列表区最多显示多少行。终端不够高时少显示几行，但一页仍是 PAGE_SIZE 首。
    fn list_rows(&self) -> usize {
        self.effective_height()
            .saturating_sub(12)
            .clamp(3, PAGE_SIZE)
    }

    /// 当前页里实际要显示的那一段。
    /// 页内放不下时（比如终端很矮）窗口在页内滚动，保证选中项始终可见。
    fn visible_range(&self, cursor: usize, rows: usize) -> (usize, usize) {
        let len = self.lib.len();
        if len == 0 {
            return (0, 0);
        }
        let page_start = (cursor / PAGE_SIZE) * PAGE_SIZE;
        let page_end = (page_start + PAGE_SIZE).min(len);
        let page_len = page_end - page_start;
        let visible = rows.min(page_len);
        let offset = cursor - page_start;
        let scroll = offset.saturating_sub(visible.saturating_sub(1));
        let start = page_start + scroll.min(page_len - visible);
        (start, start + visible)
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
        let (start, end) = self.visible_range(cursor, self.list_rows());

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

        let range = if len == 0 {
            String::new()
        } else {
            format!(
                "第 {}/{} 页 · 第 {}-{} / {len} 首",
                self.page_index() + 1,
                self.page_count(),
                start + 1,
                end
            )
        };

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
            state: match self.player.state() {
                State::Playing => renderer::PlayState::Playing,
                State::Paused => renderer::PlayState::Paused,
                State::Stopped => renderer::PlayState::Stopped,
            },
            position_secs: self.player.position().as_secs(),
            duration_secs: self.player.duration().as_secs(),
            volume: self.player.volume_percent(),
            shuffle: self.cfg.shuffle,
            repeat: self.cfg.repeat.label(),
            source: self.source.clone(),
            status: self.status.clone(),
            input: self.input.clone(),
            empty_hint: len == 0,
            range,
            rows: list,
        }
    }

    pub fn refresh(&mut self) {
        let view = self.view();
        let frame = self.renderer.draw(&view);
        if frame.is_empty() {
            return;
        }
        print!("{frame}");
        io::stdout().flush().ok();
    }
}

fn state_label(state: State) -> &'static str {
    match state {
        State::Stopped => "未播放",
        State::Playing => "播放中",
        State::Paused => "已暂停",
    }
}

fn describe_failure(track: &Track, err: &anyhow::Error) -> String {
    match Capability::of(&track.path).hint(&track.path) {
        Some(hint) => hint,
        None => format!("{}（{err}）", track.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Window;

    /// 造一个一秒的静音 wav，够播放器认出来
    fn write_wav(path: &std::path::Path) {
        let rate: u32 = 8_000;
        let samples = rate;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples * 2).to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, samples as usize * 2));
        std::fs::write(path, out).unwrap();
    }

    fn app_with(name: &str, count: usize) -> App {
        let root = std::env::temp_dir().join(format!("music_shell_test_app_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let lib = root.join("Music");
        std::fs::create_dir_all(&lib).unwrap();
        for i in 0..count {
            write_wav(&lib.join(format!("song{i:02}.wav")));
        }
        let cfg = Config {
            library_path: lib,
            playlists_dir: root.join("playlists"),
            window: Window {
                width: 80,
                height: 24,
            },
            ..Config::default()
        };
        let mut app = App::new(cfg, root.join("config.json"), Player::new(0.5, true));
        app.scan_library();
        app
    }

    #[test]
    fn a_page_shows_ten_tracks() {
        let app = app_with("page", 25);
        let view = app.view();
        assert_eq!(view.rows.len(), PAGE_SIZE);
        assert!(view.range.contains("第 1/3 页"), "{}", view.range);
        assert_eq!(view.rows[0].index, 1);
        assert_eq!(view.rows[9].index, 10);
    }

    #[test]
    fn moving_down_selects_and_plays_the_track() {
        let mut app = app_with("down", 25);
        assert_eq!(app.player.state(), State::Stopped);
        app.dispatch(Command::CursorDown);
        assert_eq!(app.lib.position(), Some(1));
        assert_eq!(app.player.state(), State::Playing);
        assert!(app.status.contains("正在播放"), "{}", app.status);
    }

    #[test]
    fn moving_up_at_the_top_does_not_restart_the_track() {
        let mut app = app_with("up", 25);
        app.dispatch(Command::CursorDown);
        app.dispatch(Command::CursorUp);
        assert_eq!(app.lib.position(), Some(0));
        assert_eq!(app.player.state(), State::Playing);
    }

    #[test]
    fn the_right_key_flips_to_the_next_page() {
        let mut app = app_with("right", 25);
        app.dispatch(Command::PageDown);
        assert_eq!(app.lib.position(), Some(PAGE_SIZE));
        let view = app.view();
        assert_eq!(view.rows[0].index, PAGE_SIZE + 1);
        assert!(view.range.contains("第 2/3 页"), "{}", view.range);
    }

    #[test]
    fn the_left_key_flips_back() {
        let mut app = app_with("left", 25);
        app.dispatch(Command::PageDown);
        app.dispatch(Command::PageUp);
        assert_eq!(app.lib.position(), Some(0));
        assert!(app.view().range.contains("第 1/3 页"));
    }

    #[test]
    fn flipping_past_the_last_page_stays_in_the_list() {
        let mut app = app_with("past", 25);
        for _ in 0..10 {
            app.dispatch(Command::PageDown);
        }
        assert_eq!(app.lib.position(), Some(20));
        assert!(
            app.view().range.contains("第 3/3 页"),
            "{}",
            app.view().range
        );
    }

    #[test]
    fn the_last_page_shows_the_remaining_tracks() {
        let mut app = app_with("last", 25);
        app.dispatch(Command::PageDown);
        app.dispatch(Command::PageDown);
        let view = app.view();
        assert_eq!(view.rows.len(), 5);
        assert_eq!(view.rows[4].index, 25);
    }

    #[test]
    fn the_space_key_pauses_and_resumes() {
        let mut app = app_with("space", 25);
        app.dispatch(Command::TogglePlay);
        assert_eq!(app.player.state(), State::Playing);
        app.dispatch(Command::TogglePlay);
        assert_eq!(app.player.state(), State::Paused);
        assert_eq!(app.status, "已暂停");
        app.dispatch(Command::TogglePlay);
        assert_eq!(app.player.state(), State::Playing);
    }

    #[test]
    fn the_page_never_runs_past_the_end_of_the_library() {
        let app = app_with("short", 3);
        let view = app.view();
        assert_eq!(view.rows.len(), 3);
        assert!(view.range.contains("第 1/1 页"), "{}", view.range);
    }
}
