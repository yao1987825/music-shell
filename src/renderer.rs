use crate::config::Theme;

/// 是否输出 ANSI 转义序列。管道重定向、TERM=dumb 时关掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub ansi: bool,
}

impl Style {
    pub fn new(ansi: bool) -> Self {
        Self { ansi }
    }

    /// 终端不支持 ANSI 时，TERM=dumb 或输出被重定向到文件
    pub fn detect() -> Self {
        use std::io::IsTerminal;
        let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        Self::new(!dumb && std::io::stdout().is_terminal())
    }

    pub fn paint(&self, code: &str, text: &str) -> String {
        if self.ansi {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }
}

pub const RESET: &str = "\x1b[0m";
pub const DIM: &str = "\x1b[38;5;240m";

const HELP: &[(&str, &str)] = &[
    ("↑ / ↓", "上一首 / 下一首（选中即播放）"),
    ("← / →", "上一页 / 下一页（每页 10 首）"),
    ("空格", "播放 / 暂停 / 继续"),
    ("n / b", "下一首 / 上一首"),
    ("p+回车", "播放 / 暂停 / 继续"),
    ("s+回车", "随机播放开关"),
    ("<数字>+回车", "跳到第 N 首并播放"),
    ("+ / -", "音量 ±5"),
    ("l", "重新扫描音乐库"),
    ("pl", "列出所有播放列表"),
    ("pl <名字>", "加载播放列表"),
    ("sv <名字>", "把当前列表存成播放列表"),
    ("r", "循环模式 off → one → all"),
    ("c", "重新载入 config.json"),
    ("w", "把当前状态写回 config.json"),
    ("?", "显示这份帮助"),
    ("q", "退出"),
];

/// 播放状态。渲染层只认这三个状态，不关心播放器内部怎么转的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

impl PlayState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "未播放",
            Self::Playing => "播放中",
            Self::Paused => "已暂停",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub app_name: String,
    pub theme: Theme,
    pub style: Style,
    pub width: usize,
    pub height: usize,
    pub backend: &'static str,
    pub title: String,
    pub artist: String,
    pub state: PlayState,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: &'static str,
    pub source: String,
    pub status: String,
    /// 正在输入、还没回车的那行命令
    pub input: String,
    pub rows: Vec<Row>,
    pub range: String,
    pub empty_hint: bool,
}

#[derive(Debug, Default)]
pub struct Renderer {
    last_head: String,
    last_middle: String,
    last_tail: String,
    drawn: bool,
}

impl Renderer {
    pub fn draw(&mut self, view: &View) -> String {
        let (head, middle, tail) = render_parts(view);
        if self.drawn && head == self.last_head && tail == self.last_tail {
            if middle == self.last_middle {
                return String::new();
            }
            self.last_middle = middle.clone();
            if !view.style.ansi {
                return format!("{middle}\n");
            }
            let row = head.lines().count() + 1;
            return format!("\x1b[s\x1b[{row};1H\x1b[2K{middle}\x1b[u");
        }
        self.drawn = true;
        self.last_head = head.clone();
        self.last_middle = middle.clone();
        self.last_tail = tail.clone();
        if view.style.ansi {
            format!("\x1b[2J\x1b[H{head}{middle}\n{tail}")
        } else {
            format!("{head}{middle}\n{tail}")
        }
    }

    pub fn reset(&mut self) {
        self.drawn = false;
        self.last_head.clear();
        self.last_middle.clear();
        self.last_tail.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub index: usize,
    pub title: String,
    pub artist: String,
    pub active: bool,
}

pub fn help_text(theme: Theme) -> String {
    let mut out = String::from("\n");
    for (key, desc) in HELP {
        out.push_str(&format!(
            "   {}{}{RESET}{desc}\n",
            theme.accent(),
            pad(key, 14)
        ));
    }
    out.trim_end().to_string()
}

pub fn render_parts(view: &View) -> (String, String, String) {
    let theme = view.theme;
    let st = view.style;
    let width = view.width;
    let mut head = String::new();

    head.push_str(&format!(
        "{}{}{}\n",
        st.paint(theme.accent(), &view.app_name),
        "",
        ""
    ));
    head.push_str(&st.paint(DIM, &format!("{} {}", view.backend, "")));
    head.push('\n');
    head.push_str(&st.paint(DIM, &rule(width)));
    head.push('\n');
    let now_playing = format!("{} — {}", view.title, view.artist);
    head.push_str(&st.paint(theme.strong(), &fit(&now_playing, width)));
    head.push('\n');

    let middle = status_line(view);

    let mut tail = String::new();
    tail.push_str(&st.paint(DIM, &rule(width)));
    tail.push('\n');

    if view.empty_hint {
        tail.push_str(&st.paint(DIM, "(音乐库为空，输入 l 重新扫描)"));
        tail.push('\n');
    } else {
        for row in &view.rows {
            let (icon, color) = if row.active {
                (">", theme.accent())
            } else {
                (" ", DIM)
            };
            let artist = truncate(&row.artist, 24);
            let budget = width.saturating_sub(display_width(&artist) + 2);
            let line = fit(&format!("{icon}{:>4}  {}", row.index, row.title), budget);
            let fill = width.saturating_sub(display_width(&line) + display_width(&artist) + 1);
            tail.push_str(&st.paint(color, &line));
            tail.push_str(&" ".repeat(fill));
            tail.push_str(&st.paint(DIM, &artist));
            tail.push('\n');
        }
        tail.push_str(&st.paint(DIM, &view.range));
        tail.push('\n');
    }

    tail.push_str(&st.paint(DIM, &rule(width)));
    tail.push('\n');
    let state_line = format!(
        "随机 {}  循环 {}  音量 {}%",
        onoff(view.shuffle),
        view.repeat,
        view.volume
    );
    tail.push_str(&st.paint(theme.strong(), &fit(&state_line, width)));
    tail.push('\n');
    let mut status_lines = view.status.split('\n');
    let first = status_lines.next().unwrap_or_default();
    tail.push_str(&st.paint(DIM, &fit(&format!("{} » {}", view.source, first), width)));
    tail.push('\n');
    for extra in status_lines {
        tail.push_str(&st.paint(DIM, &fit(extra, width)));
        tail.push('\n');
    }
    tail.push_str(&st.paint(DIM, "输入命令 (? 看帮助, q 退出) › "));
    // 正在输入的那行字跟在提示符后面，敲了什么一眼就能看见
    if !view.input.is_empty() {
        tail.push_str(&st.paint(theme.strong(), &view.input));
    }
    (head, middle, tail)
}

pub fn render(view: &View) -> String {
    let (head, middle, tail) = render_parts(view);
    format!("{head}{middle}\n{tail}")
}

/// 曲名下面那一行：播放状态 + 时间。
/// 终端里画进度条纯属多此一举，直接给时间和状态更贴切。
pub fn status_line(view: &View) -> String {
    let st = view.style;
    let (icon, text) = match view.state {
        PlayState::Playing => ("▶", "正在播放"),
        PlayState::Paused => ("‖", "已暂停"),
        PlayState::Stopped => ("○", "未播放"),
    };
    let time = format!(
        "{} / {}",
        fmt_time(view.position_secs),
        fmt_time(view.duration_secs)
    );
    let plain = format!("   {icon}  {text}   {time}");
    if display_width(&plain) <= view.width {
        format!(
            "   {}  {}   {}",
            st.paint(view.theme.accent(), icon),
            st.paint(view.theme.strong(), text),
            st.paint(DIM, &time)
        )
    } else {
        format!(
            "   {}  {}",
            st.paint(view.theme.strong(), text),
            st.paint(DIM, &time)
        )
    }
}

pub fn onoff(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub fn rule(width: usize) -> String {
    "─".repeat(width)
}

pub fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn char_width(c: char) -> usize {
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

pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

pub fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let fill = width.saturating_sub(display_width(&text));
    format!("{text}{}", " ".repeat(fill))
}

/// 按显示宽度裁剪，末尾补 "…"（留给调用方已有的颜色转义不算在内）
pub fn fit(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    truncate(text, width)
}

pub fn truncate(text: &str, max: usize) -> String {
    if display_width(text) <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = char_width(c);
        if used + w > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    format!("{out}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_characters_count_as_two_columns() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("稻香"), 4);
        assert_eq!(display_width("a稻"), 3);
    }

    #[test]
    fn padding_aligns_mixed_width_text() {
        let a = pad("pl <名字>", 12);
        let b = pad("l", 12);
        assert_eq!(display_width(&a), 12);
        assert_eq!(display_width(&b), 12);
    }

    #[test]
    fn truncation_respects_display_width() {
        assert_eq!(truncate("稻香很好听", 5), "稻香…");
        assert_eq!(display_width(&truncate("稻香很好听", 5)), 5);
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn time_formatting_is_mm_ss() {
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(59), "0:59");
        assert_eq!(fmt_time(454), "7:34");
    }

    #[test]
    fn the_status_line_shows_the_time_and_the_state() {
        let view = View {
            app_name: "x".into(),
            theme: Theme::Dark,
            style: Style::new(false),
            width: 60,
            height: 24,
            backend: "静音模式",
            title: "t".into(),
            artist: "a".into(),
            state: PlayState::Playing,
            position_secs: 30,
            duration_secs: 60,
            volume: 70,
            shuffle: false,
            repeat: "off",
            source: "s".into(),
            status: String::new(),
            input: String::new(),
            rows: Vec::new(),
            range: String::new(),
            empty_hint: true,
        };
        let line = status_line(&view);
        assert!(line.contains("正在播放"), "{line}");
        assert!(line.contains("0:30 / 1:00"), "{line}");
        assert!(!line.contains('█'), "不该再有进度条: {line}");
        assert!(!line.contains('░'), "不该再有进度条: {line}");
    }

    #[test]
    fn a_paused_player_says_it_is_paused() {
        let view = View {
            app_name: "x".into(),
            theme: Theme::Dark,
            style: Style::new(false),
            width: 60,
            height: 24,
            backend: "静音模式",
            title: "t".into(),
            artist: "a".into(),
            state: PlayState::Paused,
            position_secs: 30,
            duration_secs: 60,
            volume: 70,
            shuffle: false,
            repeat: "off",
            source: "s".into(),
            status: String::new(),
            input: String::new(),
            rows: Vec::new(),
            range: String::new(),
            empty_hint: true,
        };
        assert!(status_line(&view).contains("已暂停"));
        assert_eq!(PlayState::Paused.label(), "已暂停");
        assert_eq!(PlayState::Playing.label(), "播放中");
        assert_eq!(PlayState::Stopped.label(), "未播放");
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn view_with(width: usize, ansi: bool) -> View {
        View {
            app_name: "music-shell".into(),
            theme: Theme::Dark,
            style: Style::new(ansi),
            width,
            height: 24,
            backend: "静音模式",
            title: "一首很长的曲子名字".into(),
            artist: "某个很长的艺术家名字".into(),
            state: PlayState::Playing,
            position_secs: 61,
            duration_secs: 454,
            volume: 70,
            shuffle: true,
            repeat: "one",
            source: "/很长的音乐库路径/with/a/very/long/path/子目录".into(),
            status: "已扫描 12345 首曲目".into(),
            input: String::new(),
            rows: vec![Row {
                index: 1,
                title: "曲目标题".into(),
                artist: "艺术家".into(),
                active: true,
            }],
            range: "第 1-1 / 1 首".into(),
            empty_hint: false,
        }
    }

    #[test]
    fn no_line_exceeds_the_configured_width() {
        let view = view_with(40, true);
        let frame = render(&view);
        for line in frame.lines() {
            let visible = strip_ansi(line);
            assert!(
                display_width(&visible) <= 40,
                "超宽了 ({}): {visible}",
                display_width(&visible)
            );
        }
    }

    #[test]
    fn a_narrow_terminal_truncates_long_content_instead_of_wrapping() {
        let view = view_with(30, true);
        let frame = render(&view);
        for line in frame.lines() {
            let visible = strip_ansi(line);
            assert!(display_width(&visible) <= 30, "超宽了: {visible}");
        }
        assert!(frame.contains('…'), "长内容应该被截断并加省略号");
    }

    #[test]
    fn ansi_disabled_output_contains_no_escape_sequences() {
        let view = view_with(80, false);
        let frame = render(&view);
        assert!(!frame.contains('\x1b'), "关闭 ANSI 后不应有转义序列");
    }

    #[test]
    fn ansi_enabled_output_does_contain_escape_sequences() {
        let view = view_with(80, true);
        let frame = render(&view);
        assert!(frame.contains('\x1b'));
    }

    #[test]
    fn style_detects_a_pipe_as_non_ansi() {
        let style = Style::new(false);
        assert_eq!(style.paint("\x1b[31m", "x"), "x");
        let style = Style::new(true);
        assert_eq!(style.paint("\x1b[31m", "x"), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn fit_respects_display_width() {
        assert_eq!(display_width(&fit("稻香很好听", 5)), 5);
        assert_eq!(fit("短", 10), "短");
    }

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn base_view(status: String) -> View {
        View {
            app_name: "x".into(),
            theme: Theme::Dark,
            style: Style::new(true),
            width: 60,
            height: 24,
            backend: "静音模式",
            title: "t".into(),
            artist: "a".into(),
            state: PlayState::Playing,
            position_secs: 0,
            duration_secs: 0,
            volume: 70,
            shuffle: false,
            repeat: "off",
            source: "src".into(),
            status,
            input: String::new(),
            rows: Vec::new(),
            range: String::new(),
            empty_hint: true,
        }
    }

    #[test]
    fn a_multi_line_status_keeps_all_its_lines() {
        let view = base_view("第一行\n第二行\n第三行".into());
        let frame = render(&view);
        for line in ["第一行", "第二行", "第三行"] {
            assert!(frame.contains(line), "多行状态丢了 {line}:\n{frame}");
        }
    }

    #[test]
    fn a_multi_line_status_keeps_every_line_within_the_width() {
        let long = "很长的帮助文字".repeat(12);
        let view = base_view(long);
        let frame = render(&view);
        for line in frame.lines() {
            let visible = strip_ansi(line);
            assert!(display_width(&visible) <= 60, "超宽: {visible}");
        }
    }

    #[test]
    fn a_single_line_status_still_renders_inline() {
        let view = base_view("正在播放某首歌".into());
        let frame = render(&view);
        assert!(frame.contains("src » 正在播放某首歌"), "{frame}");
    }

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
