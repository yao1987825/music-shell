use crate::config::Theme;

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
            let row = head.lines().count() + 1;
            self.last_progress = progress.clone();
            return format!("\x1b[s\x1b[{row};1H\x1b[2K{progress}\x1b[u");
        }
        self.drawn = true;
        self.last_head = head.clone();
        self.last_progress = progress.clone();
        self.last_tail = tail.clone();
        format!("\x1b[2J\x1b[H{head}{progress}\n{tail}")
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
    let width = view.width;
    let mut head = String::new();

    head.push_str(&format!(
        "{}{}{}{} {}{}\n",
        theme.accent(),
        view.app_name,
        RESET,
        DIM,
        view.backend,
        RESET
    ));
    head.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));
    head.push_str(&format!(
        "{}{}{RESET} {DIM}— {}\n",
        theme.strong(),
        view.title,
        view.artist
    ));

    let progress = progress_line(view);

    let mut tail = String::new();
    tail.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));

    if view.empty_hint {
        tail.push_str(&format!("{DIM}(音乐库为空，输入 l 重新扫描){RESET}\n"));
    } else {
        for row in &view.rows {
            let (icon, color) = if row.active {
                (">", theme.accent())
            } else {
                (" ", DIM)
            };
            let line = format!("{icon}{:>4}  {}", row.index, row.title);
            let fill = width.saturating_sub(display_width(&line) + 25);
            tail.push_str(&format!(
                "{color}{line}{}{RESET}{DIM}{}{RESET}\n",
                " ".repeat(fill),
                truncate(&row.artist, 24)
            ));
        }
        tail.push_str(&format!("{DIM}{}{RESET}\n", view.range));
    }

    tail.push_str(&format!("{DIM}{}{RESET}\n", rule(width)));
    tail.push_str(&format!(
        "{}状态 {}{RESET}  {DIM}随机 {}{RESET}  {DIM}循环 {}{RESET}\n",
        theme.strong(),
        view.state,
        onoff(view.shuffle),
        view.repeat,
    ));
    tail.push_str(&format!(
        "{DIM}{}{RESET}{}»{RESET} {}\n",
        view.source,
        theme.accent(),
        view.status
    ));
    tail.push_str(&format!("{DIM}输入命令 (? 看帮助, q 退出) ›{RESET} "));
    (head, progress, tail)
}

pub fn render(view: &View) -> String {
    let (head, progress, tail) = render_parts(view);
    format!("{head}{progress}\n{tail}")
}

/// 进度条单独一行，供播放中原地刷新用
pub fn progress_line(view: &View) -> String {
    let bar_width = view.width.saturating_sub(28).clamp(10, 60);
    let filled = (view.ratio * bar_width as f32).round() as usize;
    format!(
        "   {}{}{}{}{} {} / {}{RESET}",
        view.theme.bar(),
        "█".repeat(filled),
        RESET,
        DIM,
        "░".repeat(bar_width.saturating_sub(filled)),
        fmt_time(view.position_secs),
        fmt_time(view.duration_secs),
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
