#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit,
    TogglePlay,
    Next,
    Prev,
    CursorDown,
    CursorUp,
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
    fn every_documented_alias_maps_to_its_command() {
        assert_eq!(Command::parse("q"), Command::Quit);
        assert_eq!(Command::parse("quit"), Command::Quit);
        assert_eq!(Command::parse("p"), Command::TogglePlay);
        assert_eq!(Command::parse("n"), Command::Next);
        assert_eq!(Command::parse("b"), Command::Prev);
        assert_eq!(Command::parse("j"), Command::CursorDown);
        assert_eq!(Command::parse("k"), Command::CursorUp);
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
}
