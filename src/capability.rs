use std::path::Path;

use crate::library::moov_at_end;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Playable,
    NeedsTranscode,
    Unsupported,
}

impl Capability {
    pub fn of(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "mp3" | "flac" | "wav" | "ogg" | "oga" | "aac" | "aiff" | "aif" => Self::Playable,
            "m4a" | "mp4" | "m4b" | "m4v" | "mov" => {
                if moov_at_end(path) {
                    Self::NeedsTranscode
                } else {
                    Self::Playable
                }
            }
            "opus" => Self::Unsupported,
            _ => Self::Unsupported,
        }
    }

    pub fn is_playable(self) -> bool {
        matches!(self, Self::Playable)
    }

    pub fn hint(self, path: &Path) -> Option<String> {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        match self {
            Self::Playable => None,
            Self::NeedsTranscode => Some(format!(
                "{name} 的 moov 在文件末尾，rodio 解析不了；\
                 用 ffmpeg -i in.m4a -c copy -movflags +faststart out.m4a 转换一下"
            )),
            Self::Unsupported => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("?")
                    .to_ascii_lowercase();
                let reason = if ext == "opus" {
                    "rodio 0.22 不支持 Opus"
                } else {
                    "rodio 0.22 不支持这种格式"
                };
                Some(format!("{name}（{reason}）"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(name: &str, data: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join("music_shell_cap");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

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

    #[test]
    fn common_formats_are_playable() {
        for ext in ["mp3", "flac", "wav", "ogg", "aac", "aiff", "MP3", "Flac"] {
            let path = PathBuf::from(format!("/m/song.{ext}"));
            assert_eq!(Capability::of(&path), Capability::Playable, "{ext}");
            assert!(Capability::of(&path).hint(&path).is_none());
        }
    }

    #[test]
    fn opus_is_unsupported_and_says_why() {
        let path = PathBuf::from("/m/song.opus");
        assert_eq!(Capability::of(&path), Capability::Unsupported);
        let hint = Capability::of(&path).hint(&path).unwrap();
        assert!(hint.contains("Opus"), "{hint}");
    }

    #[test]
    fn unknown_extensions_are_unsupported() {
        let path = PathBuf::from("/m/song.wma");
        assert_eq!(Capability::of(&path), Capability::Unsupported);
    }

    #[test]
    fn mp4_with_trailing_moov_needs_transcoding() {
        let data = boxes(&[(b"ftyp", 28), (b"free", 8), (b"mdat", 4096), (b"moov", 512)]);
        let path = write("tail.m4a", &data);
        assert_eq!(Capability::of(&path), Capability::NeedsTranscode);
        let hint = Capability::of(&path).hint(&path).unwrap();
        assert!(hint.contains("faststart"), "{hint}");
    }

    #[test]
    fn mp4_with_leading_moov_is_playable() {
        let data = boxes(&[(b"ftyp", 32), (b"moov", 512), (b"free", 8), (b"mdat", 4096)]);
        let path = write("head.m4a", &data);
        assert_eq!(Capability::of(&path), Capability::Playable);
    }
}
