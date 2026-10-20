use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};

use music_shell::config::{self, Config, Repeat, Theme};
use music_shell::player::{Player, State};
use music_shell::playlist::{self, Library, Playlist};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[38;5;240m";
const TICK: Duration = Duration::from_millis(200);

const HELP: &[(&str, &str)] = &[
    ("<数字>", "跳转并播放该曲目"),
    ("p / 回车", "播放 / 暂停 / 继续"),
    ("n", "下一首"),
    ("b", "上一首"),
    ("j / k", "光标下移 / 上移（不切换播放）"),
    ("+ / -", "音量 ±5"),
    ("l", "重新扫描音乐库"),
    ("pl", "列出所有播放列表"),
    ("pl <名字>", "加载播放列表"),
    ("sv <名字>", "把当前列表存成播放列表"),
    ("s", "随机播放开关"),
    ("r", "循环模式 off → one → all"),
    ("c", "重新载入 config.json"),
    ("w", "把当前状态写回 config.json"),
    ("?", "显示这份帮助"),
    ("q", "退出"),
];

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
        last_render: String::new(),
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
    last_render: String,
}

impl App {
    fn relabel(&mut self) {
        if self.source.is_empty() {
            self.source = format!(
                "{} · {} 首 · 随机 {} · 循环 {}",
                self.cfg.library_path.display(),
                self.lib.len(),
                onoff(self.cfg.shuffle),
                self.cfg.repeat.label()
            );
        }
    }

    fn scan_library(&mut self) {
        let path = self.cfg.library_path.clone();
        let tracks = playlist::scan(&path);
        let count = tracks.len();
        self.lib.set_tracks(tracks);
        self.source = format!("{} · {count} 首", path.display());
        self.status = if count == 0 {
            format!("{} 下没有找到音频文件", path.display())
        } else {
            format!("已扫描 {count} 首曲目")
        };
    }

    fn play_cursor(&mut self) {
        let Some(track) = self.lib.current().cloned() else {
            self.status = "播放列表为空".into();
            return;
        };
        if let Err(err) = self.player.play(&track) {
            self.status = format!("播放失败: {err}");
            if self.lib.len() > 1 {
                self.lib.advance();
                self.play_cursor();
            }
            return;
        }
        self.status = format!("正在播放 {}", track.title);
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
        self.status = format!("随机播放 {}", onoff(self.cfg.shuffle));
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

    fn dispatch(&mut self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            self.toggle_play();
            return true;
        }
        if let Ok(index) = line.parse::<usize>() {
            if (1..=self.lib.len()).contains(&index) {
                if self.lib.select(index - 1).is_some() {
                    self.play_cursor();
                }
            } else {
                self.status = format!("序号超出范围 (1-{})", self.lib.len());
            }
            return true;
        }

        let (cmd, arg) = split_arg(line);
        match cmd.as_str() {
            "q" | "quit" | "exit" => return false,
            "p" | "play" | "pause" | "space" | "enter" => self.toggle_play(),
            "n" | "next" => {
                if self.lib.advance().is_some() {
                    self.play_cursor();
                }
            }
            "b" | "prev" => {
                if self.lib.prev().is_some() {
                    self.play_cursor();
                }
            }
            "j" => self.lib.move_cursor(1),
            "k" => self.lib.move_cursor(-1),
            "+" | "=" | "up" => self.adjust_volume(5),
            "-" | "_" | "down" => self.adjust_volume(-5),
            "l" | "load" | "rescan" => self.scan_library(),
            "s" | "shuffle" => self.toggle_shuffle(),
            "r" | "repeat" => self.cycle_repeat(),
            "c" | "reload" => self.reload_config(),
            "w" | "write" => self.save_config(),
            "pl" | "playlist" => match arg {
                Some(name) => {
                    self.load_playlist(name);
                }
                None => self.show_playlists(),
            },
            "sv" | "save" => {
                self.save_playlist(arg);
            }
            "?" | "h" | "help" => self.status = help_text(self.cfg.theme),
            _ => self.status = format!("未知命令 `{cmd}`，输入 ? 看帮助"),
        }
        true
    }

    fn repl(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || feed_stdin(tx));
        loop {
            if self.player.finished() && !self.on_track_end() {
                break;
            }
            self.refresh();
            match rx.recv_timeout(TICK) {
                Ok(Some(line)) => {
                    let line = line.trim().to_string();
                    if !self.dispatch(&line) {
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

    fn signature(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}",
            self.player.state().label(),
            self.player.position().as_millis() / 200,
            (self.player.ratio() * 100.0) as u32,
            self.player.volume_percent(),
            self.lib.len(),
            self.lib.position(),
            self.source,
            self.status,
            self.cfg.repeat.label(),
            onoff(self.cfg.shuffle),
        )
    }

    fn refresh(&mut self) {
        let sig = self.signature();
        if sig != self.last_render {
            self.last_render = sig;
            self.render();
        }
    }

    fn render(&mut self) {
        let theme = self.cfg.theme;
        let width = self.cfg.window.width;
        let height = self.cfg.window.height;
        let mut out = String::from("\x1b[2J\x1b[H");
        let backend = if self.player.silent {
            "静音模式"
        } else {
            "音频已连接"
        };

        out.push_str(&format!(
            "{}{}{}{} {}{}\n",
            theme.accent(),
            self.cfg.app_name,
            RESET,
            DIM,
            backend,
            RESET
        ));
        out.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));
        out.push_str(&self.now_playing(theme, width));
        out.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));
        out.push_str(&self.track_view(theme, width, height.saturating_sub(10)));
        out.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));
        out.push_str(&format!(
            "{}状态 {}{RESET}  {DIM}随机 {}{RESET}  {DIM}循环 {}{RESET}\n",
            theme.strong(),
            self.player.state().label(),
            onoff(self.cfg.shuffle),
            self.cfg.repeat.label(),
        ));
        out.push_str(&format!(
            "{DIM}{}{RESET}  {}»{RESET} {}\n",
            self.source,
            theme.accent(),
            self.status
        ));
        out.push_str(&format!("{DIM}输入命令 (? 看帮助, q 退出) ›{RESET} "));

        print!("{out}");
        io::stdout().flush().ok();
    }

    fn now_playing(&self, theme: Theme, width: usize) -> String {
        let bar_width = width.saturating_sub(28).clamp(10, 60);
        let filled = (self.player.ratio() * bar_width as f32).round() as usize;
        let bar = format!(
            "{}{}{}{}{}",
            theme.bar(),
            "█".repeat(filled),
            RESET,
            DIM,
            "░".repeat(bar_width.saturating_sub(filled))
        );
        let (title, artist) = match self.lib.current() {
            Some(t) => (t.title.clone(), t.artist.clone()),
            None => ("—".to_string(), String::new()),
        };
        format!(
            "{}{title}{RESET} {DIM}— {artist}\n   {bar} {} / {}{RESET}\n",
            theme.strong(),
            fmt_time(self.player.position()),
            fmt_time(self.player.duration()),
        )
    }

    fn track_view(&self, theme: Theme, width: usize, rows: usize) -> String {
        let len = self.lib.len();
        if len == 0 {
            return format!("{DIM}(音乐库为空，输入 l 重新扫描){RESET}\n");
        }
        let rows = rows.clamp(3, 20);
        let cursor = self.lib.position().unwrap_or(0);
        let start = cursor
            .saturating_sub(rows / 2)
            .min(len.saturating_sub(rows));
        let end = (start + rows).min(len);

        let mut out = String::new();
        for pos in start..end {
            let Some(track) = self.lib.track_at(pos) else {
                continue;
            };
            let (icon, color) = if pos == cursor {
                (">", theme.accent())
            } else {
                (" ", DIM)
            };
            let line = format!("{icon}{:>4}  {}", pos + 1, track.title);
            let fill = width.saturating_sub(display_width(&line) + 25);
            out.push_str(&format!(
                "{color}{line}{}{RESET}{DIM}{}{RESET}\n",
                " ".repeat(fill),
                truncate(&track.artist, 24)
            ));
        }
        out.push_str(&format!(
            "{DIM}第 {}-{} / {len} 首{RESET}\n",
            start + 1,
            end
        ));
        out
    }
}

fn split_arg(line: &str) -> (String, Option<&str>) {
    match line.split_once(char::is_whitespace) {
        Some((cmd, rest)) => {
            let rest = rest.trim();
            (cmd.to_lowercase(), (!rest.is_empty()).then_some(rest))
        }
        None => (line.to_lowercase(), None),
    }
}

fn onoff(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn rule(width: usize) -> String {
    "─".repeat(width)
}

fn char_width(c: char) -> usize {
    match c {
        '\u{1100}'..='\u{115f}'
        | '\u{2e80}'..='\u{a4cf}'
        | '\u{ac00}'..='\u{d7a3}'
        | '\u{f900}'..='\u{faff}'
        | '\u{fe30}'..='\u{fe6f}'
        | '\u{ff00}'..='\u{ff60}'
        | '\u{ffe0}'..='\u{ffe6}' => 2,
        _ => 1,
    }
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let fill = width.saturating_sub(display_width(&text));
    format!("{text}{}", " ".repeat(fill))
}

fn truncate(text: &str, max: usize) -> String {
    if display_width(text) <= max {
        return text.to_string();
    }
    let head: String = text
        .chars()
        .scan(0usize, |acc, c| {
            *acc += char_width(c);
            if *acc > max.saturating_sub(1) {
                None
            } else {
                Some(*acc)
            }
        })
        .map(|_| ())
        .zip(text.chars())
        .take_while(|((), c)| *c != '\u{2026}')
        .map(|(_, c)| c)
        .collect();
    format!("{head}…")
}

fn fmt_time(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
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

fn help_text(theme: Theme) -> String {
    let mut out = String::from("\n");
    for (key, desc) in HELP {
        out.push_str(&format!(
            "   {}{}{RESET}{desc}\n",
            theme.accent(),
            pad(key, 12)
        ));
    }
    out.trim_end().to_string()
}
