#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit,
    TogglePlay,
    Next,
    Prev,
    CursorDown,
    CursorUp,
    CursorHome,
    CursorEnd,
    PageUp,
    PageDown,
    VolumeUp,
    VolumeDown,
    PlayIndex(usize),
    Rescan,
    ToggleShuffle,
    CycleRepeat,
    ReloadConfig,
    SaveConfig,
    ListPlaylists,
    LoadPlaylist(String),
    SavePlaylist(String),
    Help,
    Unknown(String),
}

impl Command {
    pub fn parse(input: &str) -> Self {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Self::TogglePlay;
        }
        if let Ok(index) = trimmed.parse::<usize>() {
            return Self::PlayIndex(index);
        }

        let (verb, arg) = match trimmed.split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_lowercase(), Some(rest.trim().to_string())),
            None => (trimmed.to_lowercase(), None),
        };

        match verb.as_str() {
            "q" | "quit" | "exit" => Self::Quit,
            "p" | "play" | "pause" | "space" | "enter" => Self::TogglePlay,
            "n" | "next" => Self::Next,
            "b" | "prev" => Self::Prev,
            "j" | "down" => Self::CursorDown,
            "k" | "up" => Self::CursorUp,
            "g" | "home" => Self::CursorHome,
            "e" | "end" => Self::CursorEnd,
            "+" | "=" | "vol+" => Self::VolumeUp,
            "-" | "_" | "vol-" => Self::VolumeDown,
            "l" | "load" | "rescan" => Self::Rescan,
            "s" | "shuffle" => Self::ToggleShuffle,
            "r" | "repeat" => Self::CycleRepeat,
            "c" | "reload" => Self::ReloadConfig,
            "w" | "write" => Self::SaveConfig,
            "pl" | "playlist" => match arg {
                Some(name) if !name.is_empty() => Self::LoadPlaylist(name),
                _ => Self::ListPlaylists,
            },
            "sv" | "save" => match arg {
                Some(name) if !name.is_empty() => Self::SavePlaylist(name),
                _ => Self::ListPlaylists,
            },
            "?" | "h" | "help" => Self::Help,
            _ => Self::Unknown(verb),
        }
    }
}

/// 这些命令的动词后面可以带参数。
/// 打这几个字的时候先不急着执行："p" 可能是 "pl <名字>"，"s" 可能是 "sv <名字>"。
const VERBS_WITH_ARG: &[&str] = &["pl", "sv"];

/// 这段文字敲完可以直接执行吗？
///
/// 规则：
/// - 带空格的命令要按回车（"pl work" 这种）
/// - 纯数字要按回车，否则 "12" 会被当成第 1 首
/// - "p" / "s" / "pl" / "sv" 要按回车，因为后面可能还跟着参数
/// - 其余单键命令敲一下就生效
pub fn is_instant(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.contains(|c: char| c.is_whitespace()) {
        return false;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if VERBS_WITH_ARG.iter().any(|verb| verb.starts_with(t)) {
        return false;
    }
    !matches!(Command::parse(t), Command::Unknown(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_become_index_commands() {
        assert_eq!(Command::parse("7"), Command::PlayIndex(7));
        assert_eq!(Command::parse(" 12 "), Command::PlayIndex(12));
    }

    #[test]
    fn an_empty_line_toggles_playback() {
        assert_eq!(Command::parse(""), Command::TogglePlay);
        assert_eq!(Command::parse("   "), Command::TogglePlay);
    }

    #[test]
    fn navigation_shortcuts_exist() {
        assert_eq!(Command::parse("q"), Command::Quit);
        assert_eq!(Command::parse("quit"), Command::Quit);
        assert_eq!(Command::parse("p"), Command::TogglePlay);
        assert_eq!(Command::parse("n"), Command::Next);
        assert_eq!(Command::parse("b"), Command::Prev);
        assert_eq!(Command::parse("j"), Command::CursorDown);
        assert_eq!(Command::parse("k"), Command::CursorUp);
        assert_eq!(Command::parse("g"), Command::CursorHome);
        assert_eq!(Command::parse("e"), Command::CursorEnd);
        assert_eq!(Command::parse("+"), Command::VolumeUp);
        assert_eq!(Command::parse("-"), Command::VolumeDown);
        assert_eq!(Command::parse("l"), Command::Rescan);
        assert_eq!(Command::parse("s"), Command::ToggleShuffle);
        assert_eq!(Command::parse("r"), Command::CycleRepeat);
        assert_eq!(Command::parse("c"), Command::ReloadConfig);
        assert_eq!(Command::parse("w"), Command::SaveConfig);
        assert_eq!(Command::parse("?"), Command::Help);
    }

    #[test]
    fn commands_are_case_insensitive() {
        assert_eq!(Command::parse("Q"), Command::Quit);
        assert_eq!(Command::parse("Next"), Command::Next);
        assert_eq!(Command::parse("PL"), Command::ListPlaylists);
    }

    #[test]
    fn playlist_commands_carry_their_argument() {
        assert_eq!(
            Command::parse("pl work"),
            Command::LoadPlaylist("work".into())
        );
        assert_eq!(
            Command::parse("sv 夜里的歌"),
            Command::SavePlaylist("夜里的歌".into())
        );
        assert_eq!(Command::parse("pl"), Command::ListPlaylists);
        assert_eq!(Command::parse("sv"), Command::ListPlaylists);
    }

    #[test]
    fn a_blank_argument_falls_back_to_listing() {
        assert_eq!(Command::parse("pl   "), Command::ListPlaylists);
    }

    #[test]
    fn unknown_input_is_preserved_for_the_error_message() {
        assert_eq!(Command::parse("wat"), Command::Unknown("wat".into()));
        assert_eq!(Command::parse("wat now"), Command::Unknown("wat".into()));
    }

    #[test]
    fn single_key_commands_fire_without_enter() {
        for key in ["q", "n", "b", "l", "r", "c", "w", "?", "+", "-"] {
            assert!(is_instant(key), "{key} 应该敲一下就生效");
        }
    }

    #[test]
    fn numbers_wait_for_enter() {
        assert!(!is_instant("5"));
        assert!(!is_instant("12"));
    }

    #[test]
    fn verbs_that_take_an_argument_wait_for_enter() {
        for text in ["p", "s", "pl", "sv"] {
            assert!(!is_instant(text), "{text} 后面可能还跟着参数");
        }
    }

    #[test]
    fn commands_with_a_space_wait_for_enter() {
        assert!(!is_instant("pl work"));
        assert!(!is_instant("sv work"));
    }

    #[test]
    fn nonsense_is_not_instant() {
        assert!(!is_instant("zzz"));
        assert!(!is_instant(""));
    }
}
