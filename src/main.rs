use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};

use music_shell::capability::Capability;
use music_shell::command::Command;
use music_shell::config::{self, Config, Repeat};
use music_shell::player::{Player, State};
use music_shell::playlist::{self, Library, Playlist, Track};
use music_shell::render::{self, Renderer, View};

const TICK: Duration = Duration::from_millis(200);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("错误: {err:#}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    config: PathBuf,
    playlist: Option<String>,
    no_audio: bool,
    print_config: bool,
    help: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        config: config::default_path(),
        playlist: None,
        no_audio: false,
        print_config: false,
        help: false,
    };
    let mut raw = std::env::args().skip(1);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => args.help = true,
            "--no-audio" => args.no_audio = true,
            "--print-config" => args.print_config = true,
            "--config" => {
                let Some(value) = raw.next() else {
                    bail!("--config 需要一个路径参数");
                };
                args.config = PathBuf::from(value);
            }
            other if other.starts_with("--config=") => {
                args.config = PathBuf::from(&other["--config=".len()..]);
            }
            other => bail!("未知参数: {other}（用 --help 查看用法）"),
        }
    }
    Ok(args)
}

fn usage() {
    println!(
        "music-shell —— 终端音乐播放器

用法:
  music-shell [选项]

选项:
  --config <路径>       指定配置文件（默认 {}）
  --playlist <名字>     启动时加载某个播放列表
  --no-audio            静音模式：只走状态机，不开音频设备
  --print-config        打印当前配置后退出
  -h, --help            显示用法",
        config::default_path().display()
    );
}

fn run() -> Result<()> {
    let args = parse_args()?;
    if args.help {
        usage();
        return Ok(());
    }

    let cfg = Config::load(&args.config)?;
    if args.print_config {
        println!("{}", serde_json::to_string_pretty(&cfg)?);
        return Ok(());
    }

    let volume = cfg.volume_f32();
    let mut app = App {
        cfg,
        config_path: args.config,
        lib: Library::default(),
        player: Player::new(volume, args.no_audio),
        source: String::new(),
        status: String::new(),
        renderer: Renderer::default(),
    };
    app.lib.set_shuffle(app.cfg.shuffle);
    app.relabel();

    if let Some(name) = args.playlist {
        if !app.load_playlist(&name) {
            bail!("播放列表 {name} 加载失败");
        }
    } else {
        app.scan_library();
    }

    if app.cfg.auto_play {
        app.play_cursor();
    }

    app.repl()
}

struct App {
    cfg: Config,
    config_path: PathBuf,
    lib: Library,
    player: Player,
    source: String,
    status: String,
    renderer: Renderer,
}

impl App {
    fn relabel(&mut self) {
        if self.source.is_empty() {
            self.source = format!(
                "{} · {} 首 · 随机 {} · 循环 {}",
                self.cfg.library_path.display(),
                self.lib.len(),
                render::onoff(self.cfg.shuffle),
                self.cfg.repeat.label()
            );
        }
    }

    fn scan_library(&mut self) {
        let path = self.cfg.library_path.clone();
        let result = playlist::scan_with_options(&path, playlist::ScanOptions::default());
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

    fn play_cursor(&mut self) {
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

    fn toggle_play(&mut self) {
        match self.player.state() {
            State::Stopped => self.play_cursor(),
            _ => {
                self.player.toggle_pause();
                self.status = format!("状态 {}", self.player.state().label());
            }
        }
    }

    fn on_track_end(&mut self) -> bool {
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

    fn adjust_volume(&mut self, delta: i32) {
        let next = i32::from(self.player.volume_percent()) + delta;
        self.player
            .set_volume(f32::from(next.clamp(0, 100) as u8) / 100.0);
        self.cfg.set_volume_f32(self.player.volume());
        self.status = format!("音量 {}%", self.player.volume_percent());
    }

    fn toggle_shuffle(&mut self) {
        self.cfg.shuffle = !self.cfg.shuffle;
        self.lib.set_shuffle(self.cfg.shuffle);
        self.source = format!("{} · {} 首", self.source_base(), self.lib.len());
        self.status = format!("随机播放 {}", render::onoff(self.cfg.shuffle));
    }

    fn cycle_repeat(&mut self) {
        self.cfg.repeat = self.cfg.repeat.next();
        self.source = format!("{} · {} 首", self.source_base(), self.lib.len());
        self.status = format!("循环模式 {}", self.cfg.repeat.label());
    }

    fn source_base(&self) -> String {
        self.source
            .split(" · ")
            .next()
            .unwrap_or(&self.source)
            .to_string()
    }

    fn load_playlist(&mut self, name: &str) -> bool {
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

    fn save_playlist(&mut self, name: Option<&str>) -> bool {
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

    fn show_playlists(&mut self) {
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

    fn remember_playlist(&mut self, name: &str) {
        self.cfg.recent_playlists.retain(|n| n != name);
        self.cfg.recent_playlists.insert(0, name.to_string());
        self.cfg.recent_playlists.truncate(10);
    }

    fn reload_config(&mut self) {
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

    fn save_config(&mut self) {
        self.cfg.set_volume_f32(self.player.volume());
        self.cfg.shuffle = self.lib.shuffle;
        match self.cfg.save(&self.config_path) {
            Ok(()) => self.status = format!("配置已写入 {}", self.config_path.display()),
            Err(err) => self.status = format!("写配置失败: {err}"),
        }
    }

    fn dispatch(&mut self, command: Command) -> bool {
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
            Command::Help => self.status = render::help_text(self.cfg.theme),
            Command::Unknown(word) => {
                self.status = format!("未知命令 `{word}`，输入 ? 看帮助");
            }
        }
        true
    }

    fn repl(&mut self) -> Result<()> {
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

    fn view(&self) -> View {
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
                list.push(render::Row {
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
            width: self.cfg.window.width,
            height: self.cfg.window.height,
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

    fn refresh(&mut self) {
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
