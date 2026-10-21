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
    pub state: &'static str,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub ratio: f32,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: &'static str,
    pub source: String,
    pub status: String,
    pub rows: Vec<Row>,
    pub range: String,
    pub empty_hint: bool,
}

#[derive(Debug, Default)]
pub struct Renderer {
    last_head: String,
    last_progress: String,
    last_tail: String,
    drawn: bool,
}

impl Renderer {
    pub fn draw(&mut self, view: &View) -> String {
        let (head, progress, tail) = render_parts(view);
        if self.drawn && head == self.last_head && tail == self.last_tail {
            if progress == self.last_progress {
                return String::new();
            }
            self.last_progress = progress.clone();
            if !view.style.ansi {
                return format!("{progress}\n");
            }
            let row = head.lines().count() + 1;
            return format!("\x1b[s\x1b[{row};1H\x1b[2K{progress}\x1b[u");
        }
        self.drawn = true;
        self.last_head = head.clone();
        self.last_progress = progress.clone();
        self.last_tail = tail.clone();
        if view.style.ansi {
            format!("\x1b[2J\x1b[H{head}{progress}\n{tail}")
        } else {
            format!("{head}{progress}\n{tail}")
        }
    }

    pub fn reset(&mut self) {
        self.drawn = false;
        self.last_head.clear();
        self.last_progress.clear();
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
            pad(key, 12)
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

    let progress = progress_line(view);

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
        "状态 {}  随机 {}  循环 {}",
        view.state,
        onoff(view.shuffle),
        view.repeat
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
    (head, progress, tail)
}

pub fn render(view: &View) -> String {
    let (head, progress, tail) = render_parts(view);
    format!("{head}{progress}\n{tail}")
}

pub fn progress_line(view: &View) -> String {
    let bar_width = view.width.saturating_sub(28).clamp(10, 60);
    let filled = (view.ratio * bar_width as f32).round() as usize;
    let bar = format!(
        "{}{}",
        view.style.paint(view.theme.bar(), &"█".repeat(filled)),
        view.style
            .paint(DIM, &"░".repeat(bar_width.saturating_sub(filled)))
    );
    format!(
        "   {bar} {} / {}",
        fmt_time(view.position_secs),
        fmt_time(view.duration_secs)
    )
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
    fn the_progress_bar_reflects_the_ratio() {
        let view = View {
            app_name: "x".into(),
            theme: Theme::Dark,
            style: Style::new(true),
            width: 60,
            height: 24,
            backend: "静音模式",
            title: "t".into(),
            artist: "a".into(),
            state: "playing",
            position_secs: 30,
            duration_secs: 60,
            ratio: 0.5,
            volume: 70,
            shuffle: false,
            repeat: "off",
            source: "s".into(),
            status: String::new(),
            rows: Vec::new(),
            range: String::new(),
            empty_hint: true,
        };
        let line = progress_line(&view);
        assert!(line.contains('█'));
        assert!(line.contains('░'));
        assert!(line.contains("0:30 / 1:00"));
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
            state: "playing",
            position_secs: 61,
            duration_secs: 454,
            ratio: 0.13,
            volume: 70,
            shuffle: true,
            repeat: "one",
            source: "/很长的音乐库路径/with/a/very/long/path/子目录".into(),
            status: "已扫描 12345 首曲目".into(),
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
            state: "playing",
            position_secs: 0,
            duration_secs: 0,
            ratio: 0.0,
            volume: 70,
            shuffle: false,
            repeat: "off",
            source: "src".into(),
            status,
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
