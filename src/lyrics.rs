use std::path::{Path, PathBuf};
use std::time::Duration;

/// 一行歌词及其时间点
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    pub at: Duration,
    pub text: String,
}

/// 一首歌的歌词
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// 歌词文件来源，给用户一个交代
    pub source: Option<PathBuf>,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 播放到 pos 时应该显示的那一行
    pub fn line_at(&self, pos: Duration) -> Option<&LyricLine> {
        if self.lines.is_empty() {
            return None;
        }
        match self.lines.binary_search_by(|l| l.at.cmp(&pos)) {
            Ok(i) => self.lines.get(i),
            Err(0) => self.lines.first(),
            Err(i) => self.lines.get(i - 1),
        }
    }

    /// 当前行在整首歌里的序号，用来算滚动窗口
    pub fn index_at(&self, pos: Duration) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        match self.lines.binary_search_by(|l| l.at.cmp(&pos)) {
            Ok(i) => Some(i),
            Err(0) => Some(0),
            Err(i) => Some(i - 1),
        }
    }

    /// 取当前行前后各若干行，用于面板展示
    pub fn window(&self, pos: Duration, before: usize, after: usize) -> Vec<&LyricLine> {
        let Some(idx) = self.index_at(pos) else {
            return Vec::new();
        };
        let start = idx.saturating_sub(before);
        let end = (idx + after + 1).min(self.lines.len());
        self.lines[start..end].iter().collect()
    }

    /// 当前行在 window() 结果里的下标。
    ///
    /// 窗口开头会被边界截断，所以不能简单用 全局下标 - before，
    /// 得按窗口实际起点算。
    pub fn current_in_window(&self, pos: Duration, before: usize, after: usize) -> Option<usize> {
        let idx = self.index_at(pos)?;
        let start = idx.saturating_sub(before);
        let end = (idx + after + 1).min(self.lines.len());
        if idx < start || idx >= end {
            return None;
        }
        Some(idx - start)
    }
}

/// 解析 LRC 格式。
///
/// 形如：
/// ```text
/// [ti:稻香]
/// [ar:周杰伦]
/// [00:12.34]窗外的麻雀
/// [00:15.60][01:20.00]在电线杆上摇摇晃晃
/// ```
/// 一行可以有多个时间标签，对应同一句歌词的多个时间点。
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some((stamps, content)) = split_tags(line) else {
            continue;
        };
        if stamps.is_empty() {
            continue;
        }
        let text = content.trim();
        for at in stamps {
            lines.push(LyricLine {
                at,
                text: text.to_string(),
            });
        }
    }
    lines.sort_by_key(|l| l.at);
    Lyrics {
        lines,
        source: None,
    }
}

/// 拆出所有 [mm:ss.xx] 时间标签和剩余正文
fn split_tags(line: &str) -> Option<(Vec<Duration>, &str)> {
    let mut stamps = Vec::new();
    let mut rest = line;
    loop {
        if !rest.starts_with('[') {
            break;
        }
        let Some(end) = rest.find(']') else {
            break;
        };
        let inner = &rest[1..end];
        // [ti:xxx] 这类元信息标签解析不出时间，跳过但不当作歌词
        if let Some(d) = parse_timestamp(inner) {
            stamps.push(d);
        }
        rest = &rest[end + 1..];
    }
    Some((stamps, rest))
}

/// 解析 mm:ss.xx / mm:ss / mm:ss:xx
fn parse_timestamp(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (main, frac) = match text.split_once('.') {
        Some((m, f)) => (m, Some(f)),
        None => (text, None),
    };
    let mut parts = main.split(':');
    let minutes: u64 = parts.next()?.trim().parse().ok()?;
    let seconds: u64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if seconds >= 60 {
        return None;
    }
    let mut millis = 0u64;
    if let Some(frac) = frac {
        // 取前三位作为毫秒，兼容 .5 / .50 / .500
        let digits: String = frac
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(3)
            .collect();
        if !digits.is_empty() {
            let scaled = format!("{digits:0<3}");
            millis = scaled.parse().unwrap_or(0);
        }
    }
    Some(Duration::new(minutes * 60 + seconds, 0) + Duration::from_millis(millis))
}

/// 从音频文件里读内嵌歌词
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    use lofty::file::TaggedFileExt;
    use lofty::probe::Probe;

    let tagged = Probe::open(path).ok()?;
    let file = tagged.read().ok()?;
    for tag in file.tags() {
        for item in tag.items() {
            let key = format!("{:?}", item.key());
            let is_lyrics = key.contains("Lyrics") || key.contains("LYRICS");
            if !is_lyrics {
                continue;
            }
            if let Some(text) = item.value().text() {
                let text = text.trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

/// 读音频文件里的标签（曲名、艺术家）
pub fn read_tags(path: &Path) -> Option<(String, String)> {
    use lofty::file::TaggedFileExt;
    use lofty::probe::Probe;

    let tagged = Probe::open(path).ok()?;
    let file = tagged.read().ok()?;
    let mut title = None;
    let mut artist = None;
    for tag in file.tags() {
        for item in tag.items() {
            let key = format!("{:?}", item.key());
            let Some(text) = item.value().text() else {
                continue;
            };
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            match key.as_str() {
                "TrackTitle" => {
                    title.get_or_insert_with(|| text.to_string());
                }
                "TrackArtist" => {
                    artist.get_or_insert_with(|| text.to_string());
                }
                _ => {}
            }
        }
    }
    match (title, artist) {
        (Some(t), Some(a)) => Some((t, a)),
        (Some(t), None) => Some((t, String::new())),
        _ => None,
    }
}

/// 寻找歌词文件。
///
/// 依次尝试：
/// 1. 同目录同名的 .lrc
/// 2. 同目录同名的 .txt
/// 3. lyrics_dir 下同名的 .lrc（歌词集中存放的情况）
pub fn find_lyric_file(track_path: &Path, lyrics_dir: Option<&Path>) -> Option<PathBuf> {
    let stem = track_path.file_stem()?;
    let dir = track_path.parent()?;

    for ext in ["lrc", "txt"] {
        let candidate = dir.join(format!("{}.{}", stem.to_string_lossy(), ext));
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(lyrics_dir) = lyrics_dir {
        for ext in ["lrc", "txt"] {
            let candidate = lyrics_dir.join(format!("{}.{}", stem.to_string_lossy(), ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 按优先级取歌词：旁挂 .lrc → 内嵌标签
pub fn load_lyrics(track_path: &Path, lyrics_dir: Option<&Path>) -> Lyrics {
    if let Some(file) = find_lyric_file(track_path, lyrics_dir)
        && let Ok(text) = std::fs::read_to_string(&file)
    {
        let mut lyrics = parse_lrc(&text);
        if !lyrics.is_empty() {
            lyrics.source = Some(file);
            return lyrics;
        }
    }
    if let Some(text) = embedded_lyrics(track_path) {
        let mut lyrics = parse_lrc(&text);
        if !lyrics.is_empty() {
            lyrics.source = Some(track_path.to_path_buf());
            return lyrics;
        }
    }
    Lyrics::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_parse_in_all_common_forms() {
        assert_eq!(
            parse_timestamp("00:12.34"),
            Some(Duration::from_millis(12340))
        );
        assert_eq!(parse_timestamp("01:05"), Some(Duration::from_secs(65)));
        assert_eq!(
            parse_timestamp("00:12.5"),
            Some(Duration::from_millis(12500))
        );
        assert_eq!(
            parse_timestamp("00:12.500"),
            Some(Duration::from_millis(12500))
        );
        assert_eq!(parse_timestamp("10:00.00"), Some(Duration::from_secs(600)));
    }

    #[test]
    fn invalid_timestamps_are_rejected() {
        assert_eq!(parse_timestamp("ti:稻香"), None);
        assert_eq!(parse_timestamp("ab:cd"), None);
        assert_eq!(parse_timestamp("00:99.00"), None, "秒数不能到 60 以上");
        assert_eq!(parse_timestamp("00"), None);
    }

    #[test]
    fn a_typical_lrc_file_parses() {
        let text = "[ti:稻香]\n[ar:周杰伦]\n[00:00.00]窗外的麻雀\n[00:05.20]在电线杆上摇摇晃晃\n";
        let lyrics = parse_lrc(text);
        assert_eq!(lyrics.lines.len(), 2, "元信息标签不算歌词");
        assert_eq!(lyrics.lines[0].text, "窗外的麻雀");
        assert_eq!(lyrics.lines[0].at, Duration::ZERO);
        assert_eq!(lyrics.lines[1].at, Duration::from_millis(5200));
    }

    #[test]
    fn one_line_can_have_several_timestamps() {
        let text = "[00:10.00][01:20.00]重复的句子\n";
        let lyrics = parse_lrc(text);
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].at, Duration::from_secs(10));
        assert_eq!(lyrics.lines[1].at, Duration::from_secs(80));
        assert_eq!(lyrics.lines[0].text, "重复的句子");
    }

    #[test]
    fn lines_are_sorted_by_time() {
        let text = "[00:30.00]第三\n[00:10.00]第一\n[00:20.00]第二\n";
        let lyrics = parse_lrc(text);
        assert_eq!(lyrics.lines[0].text, "第一");
        assert_eq!(lyrics.lines[1].text, "第二");
        assert_eq!(lyrics.lines[2].text, "第三");
    }

    #[test]
    fn the_current_line_is_found_by_time() {
        let text = "[00:00.00]一\n[00:10.00]二\n[00:20.00]三\n";
        let lyrics = parse_lrc(text);
        assert_eq!(lyrics.line_at(Duration::from_secs(5)).unwrap().text, "一");
        assert_eq!(lyrics.line_at(Duration::from_secs(10)).unwrap().text, "二");
        assert_eq!(lyrics.line_at(Duration::from_secs(15)).unwrap().text, "二");
        assert_eq!(lyrics.line_at(Duration::from_secs(25)).unwrap().text, "三");
    }

    #[test]
    fn before_the_first_line_it_shows_the_first() {
        let text = "[00:10.00]晚一点才开始\n";
        let lyrics = parse_lrc(text);
        assert_eq!(lyrics.line_at(Duration::ZERO).unwrap().text, "晚一点才开始");
    }

    #[test]
    fn the_window_centres_on_the_current_line() {
        let text = "[00:00.00]1\n[00:01.00]2\n[00:02.00]3\n[00:03.00]4\n[00:04.00]5\n";
        let lyrics = parse_lrc(text);
        let w = lyrics.window(Duration::from_secs(2), 1, 1);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].text, "2");
        assert_eq!(w[1].text, "3");
        assert_eq!(w[2].text, "4");
    }

    #[test]
    fn the_current_line_index_inside_the_window_is_right() {
        let text = "[00:00.00]1\n[00:01.00]2\n[00:02.00]3\n[00:03.00]4\n[00:04.00]5\n";
        let lyrics = parse_lrc(text);
        // before=2 after=2，第三行(idx=2)是当前行，窗口从 0 开始
        assert_eq!(
            lyrics.current_in_window(Duration::from_secs(2), 2, 2),
            Some(2)
        );
        // 第一行(idx=0)时窗口被开头截断，当前行下标是 0 而不是 0-2
        assert_eq!(lyrics.current_in_window(Duration::ZERO, 2, 2), Some(0));
        // 最后一行的窗口被结尾截断
        assert_eq!(
            lyrics.current_in_window(Duration::from_secs(4), 2, 2),
            Some(2)
        );
    }

    #[test]
    fn the_window_is_clamped_at_the_edges() {
        let text = "[00:00.00]1\n[00:01.00]2\n[00:02.00]3\n";
        let lyrics = parse_lrc(text);
        let w = lyrics.window(Duration::ZERO, 2, 2);
        assert_eq!(w.len(), 3, "开头不该越界");
        assert_eq!(w[0].text, "1");
    }

    #[test]
    fn empty_lyrics_are_handled() {
        let lyrics = parse_lrc("");
        assert!(lyrics.is_empty());
        assert!(lyrics.line_at(Duration::from_secs(5)).is_none());
        assert!(lyrics.window(Duration::ZERO, 2, 2).is_empty());
    }

    #[test]
    fn lines_without_timestamps_are_skipped() {
        let lyrics = parse_lrc("这是一行没有时间戳的文字\n[00:01.00]有时间戳\n");
        assert_eq!(lyrics.lines.len(), 1);
        assert_eq!(lyrics.lines[0].text, "有时间戳");
    }

    #[test]
    fn the_lyric_file_is_found_next_to_the_track() {
        let dir = std::env::temp_dir().join("music_shell_test_lyr");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let track = dir.join("song.mp3");
        std::fs::write(&track, b"x").unwrap();
        let lrc = dir.join("song.lrc");
        std::fs::write(&lrc, "[00:01.00]hi\n").unwrap();

        let found = find_lyric_file(&track, None).unwrap();
        assert_eq!(found, lrc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_central_lyrics_directory_is_searched_too() {
        let dir = std::env::temp_dir().join("music_shell_test_lyr2");
        let lrc_dir = dir.join("lyrics");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&lrc_dir).unwrap();
        let track = dir.join("song.mp3");
        std::fs::write(&track, b"x").unwrap();
        std::fs::write(lrc_dir.join("song.lrc"), "[00:01.00]hi\n").unwrap();

        let found = find_lyric_file(&track, Some(&lrc_dir)).unwrap();
        assert_eq!(found, lrc_dir.join("song.lrc"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_lyric_file_returns_none() {
        let dir = std::env::temp_dir().join("music_shell_test_lyr3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let track = dir.join("nothing.mp3");
        std::fs::write(&track, b"x").unwrap();
        assert!(find_lyric_file(&track, None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
