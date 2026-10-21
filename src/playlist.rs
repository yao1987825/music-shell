use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "aiff", "aif",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
}

impl Track {
    pub fn from_path(path: PathBuf) -> Self {
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let artist = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path,
            title,
            artist,
        }
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }
}

const MP4_EXTS: &[&str] = &["m4a", "mp4", "m4b", "m4v", "mov", "aac_in_mp4"];

pub fn is_mp4_container(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| MP4_EXTS.contains(&e.as_str()))
}

pub fn moov_at_end(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(total) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    let mut offset = 0u64;
    let mut last_was_moov = false;
    while offset + 8 <= total {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return false;
        }
        let mut header = [0u8; 16];
        let read = file.read(&mut header).unwrap_or(0);
        if read < 8 {
            break;
        }
        let raw = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let atype = &header[4..8];
        let (header_len, size) = match raw {
            0 => break,
            1 => {
                if read < 16 {
                    break;
                }
                let mut ext = [0u8; 8];
                ext.copy_from_slice(&header[8..16]);
                (16u64, u64::from_be_bytes(ext))
            }
            n => (8u64, u64::from(n)),
        };
        if size < header_len {
            break;
        }
        last_was_moov = atype == b"moov";
        offset += size;
    }
    last_was_moov
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| AUDIO_EXTS.contains(&e.as_str()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Path,
    Title,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    pub ignore_hidden: bool,
    pub max_depth: Option<usize>,
    pub sort_by: SortBy,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            ignore_hidden: true,
            max_depth: None,
            sort_by: SortBy::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub tracks: Vec<Track>,
    pub unsupported: Vec<Track>,
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

pub fn scan(dir: &Path) -> Vec<Track> {
    scan_with_options(dir, ScanOptions::default()).tracks
}

pub fn scan_with_options(dir: &Path, opts: ScanOptions) -> ScanResult {
    if !dir.is_dir() {
        return ScanResult::default();
    }
    let mut walker = WalkDir::new(dir).follow_links(false);
    if let Some(depth) = opts.max_depth {
        walker = walker.max_depth(depth);
    }

    let mut tracks: Vec<Track> = Vec::new();
    let mut unsupported: Vec<Track> = Vec::new();
    let mut modified: Vec<(std::time::SystemTime, usize)> = Vec::new();

    for entry in walker.into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if opts.ignore_hidden && is_hidden(path) {
            continue;
        }
        if !is_audio(path) {
            continue;
        }
        let track = Track::from_path(path.to_path_buf());
        if crate::capability::Capability::of(path).is_playable() {
            if opts.sort_by == SortBy::Modified {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                modified.push((mtime, tracks.len()));
                tracks.push(track);
            } else {
                tracks.push(track);
            }
        } else {
            unsupported.push(track);
        }
    }

    match opts.sort_by {
        SortBy::Path => tracks.sort_by(|a, b| a.path.cmp(&b.path)),
        SortBy::Title => tracks.sort_by(|a, b| a.title.cmp(&b.title).then(a.path.cmp(&b.path))),
        SortBy::Modified => {
            modified.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
            let reordered: Vec<Track> = modified
                .iter()
                .filter_map(|(_, i)| tracks.get(*i).cloned())
                .collect();
            tracks = reordered;
        }
    }
    unsupported.sort_by(|a, b| a.path.cmp(&b.path));

    ScanResult {
        tracks,
        unsupported,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub fn file_in(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{}.json", name.replace(['/', '\\'], "_")))
    }

    pub fn load(dir: &Path, name: &str) -> Result<Self> {
        let path = Self::file_in(dir, name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取播放列表 {} 失败: {e}", path.display()))?;
        let mut pl: Self = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("解析播放列表 {} 失败: {e}", path.display()))?;
        pl.tracks.retain(|t| t.exists());
        pl.name = name.to_string();
        Ok(pl)
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("创建播放列表目录失败: {e}"))?;
        let path = Self::file_in(dir, &self.name);
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|e| anyhow::anyhow!("写入 {} 失败: {e}", path.display()))?;
        Ok(path)
    }

    pub fn list(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        names
    }
}

#[derive(Debug, Default)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub order: Vec<usize>,
    pub cursor: Option<usize>,
    pub history: Vec<usize>,
    pub shuffle: bool,
}

impl Library {
    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = tracks;
        self.rebuild_order();
        self.cursor = if self.order.is_empty() { None } else { Some(0) };
        self.history.clear();
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn position(&self) -> Option<usize> {
        self.cursor
    }

    pub fn current(&self) -> Option<&Track> {
        self.cursor
            .and_then(|c| self.order.get(c))
            .map(|i| &self.tracks[*i])
    }

    pub fn track_at(&self, pos: usize) -> Option<&Track> {
        self.order.get(pos).map(|i| &self.tracks[*i])
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.order.is_empty() {
            return;
        }
        let len = self.order.len() as isize;
        let cur = self.cursor.unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.cursor = Some(next);
    }

    pub fn select(&mut self, pos: usize) -> Option<&Track> {
        if pos >= self.order.len() {
            return None;
        }
        if let Some(cur) = self.cursor
            && cur != pos
        {
            self.history.push(cur);
        }
        self.cursor = Some(pos);
        self.current()
    }

    pub fn advance(&mut self) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        let len = self.order.len();
        let cur = self.cursor.unwrap_or(0);
        if cur + 1 < len {
            self.history.push(cur);
            self.cursor = Some(cur + 1);
        } else {
            self.cursor = Some(0);
            self.history.clear();
        }
        self.current()
    }

    pub fn prev(&mut self) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        if let Some(prev) = self.history.pop() {
            self.cursor = Some(prev);
        } else {
            let len = self.order.len();
            self.cursor = Some(self.cursor.unwrap_or(0).saturating_sub(1) % len);
        }
        self.current()
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if self.shuffle == on {
            return;
        }
        let cur = self.current().map(|t| t.path.clone());
        self.shuffle = on;
        self.rebuild_order();
        self.history.clear();
        self.cursor = cur.and_then(|p| self.order.iter().position(|i| self.tracks[*i].path == p));
        if self.cursor.is_none() && !self.order.is_empty() {
            self.cursor = Some(0);
        }
    }

    fn rebuild_order(&mut self) {
        self.order = (0..self.tracks.len()).collect();
        if !self.shuffle {
            return;
        }
        self.order.shuffle(&mut rand::rng());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> Track {
        Track::from_path(PathBuf::from(format!("/music/{name}.mp3")))
    }

    fn library(names: &[&str]) -> Library {
        let mut lib = Library::default();
        lib.set_tracks(names.iter().copied().map(track).collect());
        lib
    }

    #[test]
    fn audio_detection_ignores_case_and_other_extensions() {
        assert!(is_audio(Path::new("a.MP3")));
        assert!(is_audio(Path::new("a.flac")));
        assert!(is_audio(Path::new("a.Ogg")));
        assert!(!is_audio(Path::new("a.txt")));
        assert!(!is_audio(Path::new("a")));
        assert!(!is_audio(Path::new("mp3")));
    }

    #[test]
    fn track_metadata_comes_from_path() {
        let t = Track::from_path(PathBuf::from("/music/Album1/稻香.mp3"));
        assert_eq!(t.title, "稻香");
        assert_eq!(t.artist, "Album1");
    }

    #[test]
    fn scan_ignores_missing_directory() {
        assert!(scan(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn sequential_navigation_walks_the_list() {
        let mut lib = library(&["a", "b", "c"]);
        assert_eq!(lib.len(), 3);
        assert!(!lib.is_empty());
        assert_eq!(lib.position(), Some(0));
        assert_eq!(lib.current().unwrap().title, "a");
        assert_eq!(lib.advance().unwrap().title, "b");
        assert_eq!(lib.advance().unwrap().title, "c");
        assert_eq!(lib.advance().unwrap().title, "a");
    }

    #[test]
    fn an_empty_library_navigates_to_nowhere() {
        let mut lib = Library::default();
        assert!(lib.is_empty());
        assert!(lib.current().is_none());
        assert!(lib.advance().is_none());
        assert!(lib.prev().is_none());
    }

    #[test]
    fn prev_uses_history_before_walking_backwards() {
        let mut lib = library(&["a", "b", "c"]);
        lib.advance();
        lib.advance();
        assert_eq!(lib.current().unwrap().title, "c");
        assert_eq!(lib.prev().unwrap().title, "b");
        assert_eq!(lib.prev().unwrap().title, "a");
        assert_eq!(lib.prev().unwrap().title, "a");
    }

    #[test]
    fn cursor_wraps_in_both_directions() {
        let mut lib = library(&["a", "b", "c"]);
        lib.move_cursor(-1);
        assert_eq!(lib.position(), Some(2));
        lib.move_cursor(1);
        assert_eq!(lib.position(), Some(0));
    }

    #[test]
    fn out_of_range_select_is_rejected() {
        let mut lib = library(&["a", "b"]);
        assert!(lib.select(5).is_none());
        assert_eq!(lib.position(), Some(0));
    }

    #[test]
    fn shuffle_is_a_permutation_and_keeps_the_current_track() {
        let mut lib = library(&["a", "b", "c", "d", "e"]);
        let all: Vec<String> = lib.tracks.iter().map(|t| t.title.clone()).collect();
        let current = lib.current().unwrap().title.clone();
        lib.set_shuffle(true);
        assert_eq!(lib.len(), all.len());
        let shuffled: Vec<&str> = lib
            .order
            .iter()
            .map(|i| lib.tracks[*i].title.as_str())
            .collect();
        let mut sorted_shuffled = shuffled.clone();
        sorted_shuffled.sort_unstable();
        let mut sorted_all = all.iter().map(String::as_str).collect::<Vec<_>>();
        sorted_all.sort_unstable();
        assert_eq!(sorted_shuffled, sorted_all);
        assert_eq!(lib.current().unwrap().title, current);
    }

    #[test]
    fn shuffle_navigation_stays_in_bounds() {
        let mut lib = library(&["a", "b", "c", "d"]);
        lib.set_shuffle(true);
        for _ in 0..12 {
            assert!(lib.advance().is_some());
        }
        for _ in 0..12 {
            assert!(lib.prev().is_some());
        }
    }

    #[test]
    fn playlist_round_trips_through_disk() {
        let root = std::env::temp_dir().join("music_shell_test_pl");
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("playlists");
        let pl = Playlist {
            name: "work".into(),
            tracks: vec![touch(&root, "a"), touch(&root, "b")],
        };
        let path = pl.save(&dir).unwrap();
        assert!(path.exists());
        let back = Playlist::load(&dir, "work").unwrap();
        assert_eq!(back.tracks.len(), 2);
        assert_eq!(back.name, "work");
        assert_eq!(Playlist::list(&dir), vec!["work".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn playlist_loading_drops_vanished_tracks() {
        let root = std::env::temp_dir().join("music_shell_test_missing");
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("playlists");
        let gone = touch(&root, "gone");
        let pl = Playlist {
            name: "stale".into(),
            tracks: vec![touch(&root, "here"), gone.clone()],
        };
        pl.save(&dir).unwrap();
        std::fs::remove_file(&gone.path).unwrap();
        let back = Playlist::load(&dir, "stale").unwrap();
        assert_eq!(back.tracks.len(), 1);
        assert_eq!(back.tracks[0].title, "here");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn touch(root: &Path, name: &str) -> Track {
        std::fs::create_dir_all(root).unwrap();
        let path = root.join(format!("{name}.mp3"));
        std::fs::write(&path, b"").unwrap();
        Track::from_path(path)
    }

    #[test]
    fn missing_playlist_reports_an_error() {
        let dir = std::env::temp_dir().join("music_shell_test_absent");
        assert!(Playlist::load(&dir, "nothing").is_err());
    }
}

#[cfg(test)]
mod mp4_tests {
    use super::*;

    fn boxes(entries: &[(&[u8; 4], u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (typ, size) in entries {
            let total = if *size == 0 {
                out.len() as u64 + 8
            } else {
                *size
            };
            out.extend_from_slice(&(total as u32).to_be_bytes());
            out.extend_from_slice(*typ);
            out.resize(out.len() + (total as usize - 8), 0);
        }
        out
    }

    fn write(name: &str, data: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join("music_shell_test_mp4");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn moov_at_end_is_detected() {
        let data = boxes(&[(b"ftyp", 28), (b"free", 8), (b"mdat", 4096), (b"moov", 512)]);
        let path = write("tail.m4a", &data);
        assert!(moov_at_end(&path));
    }

    #[test]
    fn moov_at_front_is_not_flagged() {
        let data = boxes(&[(b"ftyp", 32), (b"moov", 512), (b"free", 8), (b"mdat", 4096)]);
        let path = write("head.m4a", &data);
        assert!(!moov_at_end(&path));
    }

    #[test]
    fn a_truncated_or_bogus_file_is_not_flagged() {
        let path = write("junk.m4a", b"short");
        assert!(!moov_at_end(&path));
        assert!(!moov_at_end(Path::new("/nope/none.m4a")));
    }

    #[test]
    fn mp4_container_extensions_are_recognised() {
        assert!(is_mp4_container(Path::new("a.m4a")));
        assert!(is_mp4_container(Path::new("a.MP4")));
        assert!(is_mp4_container(Path::new("a.mov")));
        assert!(!is_mp4_container(Path::new("a.mp3")));
        assert!(!is_mp4_container(Path::new("a.aac")));
    }
}

#[cfg(test)]
mod shuffle_tests {
    use super::*;

    fn library(n: usize) -> Library {
        let mut lib = Library::default();
        lib.set_tracks(
            (0..n)
                .map(|i| Track::from_path(PathBuf::from(format!("/m/{i:03}.mp3"))))
                .collect(),
        );
        lib
    }

    fn permutation(lib: &Library) -> Vec<usize> {
        lib.order.clone()
    }

    #[test]
    fn shuffle_is_not_reproducible() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..40 {
            let mut lib = library(12);
            lib.set_shuffle(true);
            seen.insert(permutation(&lib));
        }
        assert!(
            seen.len() > 1,
            "40 次洗牌得到了完全相同的顺序，随机源没生效"
        );
        assert!(seen.len() > 5, "洗牌结果过于集中：只有 {} 种", seen.len());
    }

    #[test]
    fn shuffle_is_a_permutation_of_all_tracks() {
        let mut lib = library(30);
        lib.set_shuffle(true);
        let mut got = permutation(&lib);
        got.sort_unstable();
        assert_eq!(got, (0..30).collect::<Vec<_>>());
    }

    #[test]
    fn turning_shuffle_off_restores_the_natural_order() {
        let mut lib = library(10);
        lib.set_shuffle(true);
        lib.set_shuffle(false);
        assert_eq!(permutation(&lib), (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn toggling_shuffle_keeps_the_current_track_playing() {
        let mut lib = library(15);
        let current = lib.current().unwrap().path.clone();
        lib.set_shuffle(true);
        assert_eq!(lib.current().unwrap().path, current);
    }

    #[test]
    fn every_track_appears_exactly_once_per_cycle() {
        let mut lib = library(20);
        lib.set_shuffle(true);
        let mut visited = std::collections::HashSet::new();
        for _ in 0..20 {
            let t = lib.advance().unwrap();
            assert!(visited.insert(t.path.clone()), "有曲目被重复播放");
        }
        assert_eq!(visited.len(), 20);
    }
}
