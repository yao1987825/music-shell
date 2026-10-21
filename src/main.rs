use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use music_shell::app::App;
use music_shell::config::{self, Config};
use music_shell::player::Player;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("错误: {err:#}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct Args {
    config: PathBuf,
    update: bool,
    update_check: bool,
    retries: usize,
    playlist: Option<String>,
    no_audio: bool,
    print_config: bool,
    help: bool,
    version: bool,
}

fn parse_args() -> Result<Args> {
    parse_from(std::env::args().skip(1))
}

fn parse_from<I, S>(raw: I) -> Result<Args>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = Args {
        config: config::default_path(),
        update: false,
        update_check: false,
        retries: 3,
        playlist: None,
        no_audio: false,
        print_config: false,
        help: false,
        version: false,
    };
    let mut raw = raw.into_iter().map(Into::into);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "update" => args.update = true,
            "--check" => args.update_check = true,
            "--retries" => {
                let Some(value) = raw.next() else {
                    bail!("--retries 需要一个数字");
                };
                args.retries = value
                    .parse()
                    .with_context(|| format!("--retries 的值不合法: {value}"))?;
            }
            "--no-audio" => args.no_audio = true,
            "--print-config" => args.print_config = true,
            "--config" => {
                let Some(value) = raw.next() else {
                    bail!("--config 需要一个路径参数");
                };
                args.config = PathBuf::from(value);
            }
            "--playlist" => {
                let Some(value) = raw.next() else {
                    bail!("--playlist 需要一个播放列表名字");
                };
                args.playlist = Some(value);
            }
            other if other.starts_with("--config=") => {
                let value = &other["--config=".len()..];
                if value.is_empty() {
                    bail!("--config= 需要一个路径参数");
                }
                args.config = PathBuf::from(value);
            }
            other if other.starts_with("--playlist=") => {
                let value = &other["--playlist=".len()..];
                if value.is_empty() {
                    bail!("--playlist= 需要一个播放列表名字");
                }
                args.playlist = Some(value.to_string());
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
  -h, --help            显示用法
  -V, --version         显示版本号

自更新:
  update                检查并安装新版本（自动校验 SHA256）
  update --check        只检查有没有新版本，不下载
  --retries <n>         网络失败重试次数（默认 3）

可用环境变量:
  HTTPS_PROXY / ALL_PROXY   访问 GitHub 走代理时设置
  MUSIC_SHELL_UPDATE_URL    自定义 API 地址（走镜像时用）",
        config::default_path().display()
    );
}

fn run() -> Result<()> {
    let args = parse_args()?;
    if args.help {
        usage();
        return Ok(());
    }
    if args.version {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("MS_VERSION"));
        return Ok(());
    }

    if args.update {
        music_shell::update_cmd::run(args.update_check, args.retries)?;
        return Ok(());
    }

    let cfg = Config::load(&args.config)?;
    if args.print_config {
        println!("{}", serde_json::to_string_pretty(&cfg)?);
        return Ok(());
    }

    let volume = cfg.volume_f32();
    let mut app = App::new(cfg, args.config, Player::new(volume, args.no_audio));

    if let Some(name) = args.playlist {
        if !app.load_playlist(&name) {
            bail!("播放列表 {name} 加载失败");
        }
    } else {
        app.scan_library();
    }

    if app.auto_play() {
        app.play_cursor();
    }

    app.repl()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args> {
        parse_from(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn playlist_can_be_given_as_two_arguments() {
        let args = parse(&["--playlist", "work"]).unwrap();
        assert_eq!(args.playlist.as_deref(), Some("work"));
    }

    #[test]
    fn playlist_can_be_given_with_an_equals_sign() {
        let args = parse(&["--playlist=work"]).unwrap();
        assert_eq!(args.playlist.as_deref(), Some("work"));
    }

    #[test]
    fn playlist_names_with_spaces_are_accepted() {
        let args = parse(&["--playlist=夜里的歌"]).unwrap();
        assert_eq!(args.playlist.as_deref(), Some("夜里的歌"));
        let args = parse(&["--playlist", "夜里的歌"]).unwrap();
        assert_eq!(args.playlist.as_deref(), Some("夜里的歌"));
    }

    #[test]
    fn a_missing_playlist_value_is_an_error() {
        let err = parse(&["--playlist"]).unwrap_err().to_string();
        assert!(err.contains("--playlist"), "{err}");
        let err = parse(&["--playlist="]).unwrap_err().to_string();
        assert!(err.contains("--playlist"), "{err}");
    }

    #[test]
    fn config_can_be_given_in_both_forms() {
        let a = parse(&["--config", "/tmp/a.json"]).unwrap();
        let b = parse(&["--config=/tmp/a.json"]).unwrap();
        assert_eq!(a.config, b.config);
        assert_eq!(a.config, PathBuf::from("/tmp/a.json"));
    }

    #[test]
    fn a_missing_config_value_is_an_error() {
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--config="]).is_err());
    }

    #[test]
    fn flags_can_be_combined() {
        let args = parse(&["--no-audio", "--print-config", "--playlist", "x", "-h"]).unwrap();
        assert!(args.no_audio);
        assert!(args.print_config);
        assert!(args.help);
        assert_eq!(args.playlist.as_deref(), Some("x"));
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let err = parse(&["--nope"]).unwrap_err().to_string();
        assert!(err.contains("未知参数"), "{err}");
    }

    #[test]
    fn version_flag_is_recognised() {
        assert!(parse(&["--version"]).unwrap().version);
        assert!(parse(&["-V"]).unwrap().version);
        assert!(!parse(&[]).unwrap().version);
    }

    #[test]
    fn no_arguments_yields_the_defaults() {
        let args = parse(&[]).unwrap();
        assert!(!args.help);
        assert!(!args.no_audio);
        assert!(!args.print_config);
        assert!(args.playlist.is_none());
        assert_eq!(args.config, config::default_path());
    }
}
