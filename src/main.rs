use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};

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
