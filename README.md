# music-shell

Rust 写的终端音乐播放器：JSON 配置驱动，菜单式交互，rodio 播放本地音频。

```
music-shell 静音模式
──────────────────────────────────────────────────────────────────────────────
稻香 — 周杰伦
   ████████████████████░░░░░░░░░░░░░░░░░░░░░░ 1:23 / 4:59
──────────────────────────────────────────────────────────────────────────────
>  12  稻香                                     Album1
   13  晴天                                     Album1
   14  English Song Name                        Album2
第 12-14 / 137 首
──────────────────────────────────────────────────────────────────────────────
状态 playing  随机 off  循环 off
~/Music · 137 首  » 正在播放 稻香
输入命令 (? 看帮助, q 退出) ›
```

## 编译

Debian 需要先装 ALSA 头文件，rodio 靠它链接 `libasound`：

```bash
sudo apt install libasound2-dev pkg-config build-essential
cargo build --release
./target/release/music-shell
```

没声卡（服务器 / SSH）也能跑，加 `--no-audio` 走静音模式，状态机和进度照常工作。

## 命令

| 命令 | 作用 |
| --- | --- |
| `<数字>` | 跳转并播放该曲目 |
| `p` / 回车 | 播放 / 暂停 / 继续 |
| `n` / `b` | 下一首 / 上一首 |
| `j` / `k` | 光标下移 / 上移（只移动，不切换播放） |
| `+` / `-` | 音量 ±5 |
| `l` | 重新扫描音乐库 |
| `pl` / `pl <名字>` | 列出 / 加载播放列表 |
| `sv <名字>` | 把当前列表存成播放列表 |
| `s` | 随机播放开关 |
| `r` | 循环模式 off → one → all |
| `c` | 重新载入 `config.json` |
| `w` | 把当前音量 / 状态写回 `config.json` |
| `?` | 帮助 |
| `q` | 退出 |

## 启动参数

| 参数 | 说明 |
| --- | --- |
| `--config <路径>` | 指定配置文件，默认 `~/.config/music-shell/config.json` |
| `--playlist <名字>` | 启动时加载某个播放列表 |
| `--no-audio` | 静音模式，不开音频设备 |
| `--print-config` | 打印当前配置后退出 |

## 配置

首次运行会在 `~/.config/music-shell/config.json` 生成一份默认配置，之后用 `w` 命令回写。

```json
{
  "app_name": "music-shell",
  "theme": "dark",
  "volume": 70,
  "library_path": "/home/you/Music",
  "playlists_dir": "/home/you/.config/music-shell/playlists",
  "auto_play": true,
  "shuffle": false,
  "repeat": "off",
  "recent_playlists": ["favorites", "work"],
  "window": { "width": 80, "height": 24 }
}
```

| 字段 | 取值 | 说明 |
| --- | --- | --- |
| `theme` | `dark` / `light` | 配色 |
| `volume` | `0`–`100` | 音量百分比，超范围会被夹紧 |
| `library_path` | 路径 | 递归扫描的音频目录 |
| `auto_play` | bool | 启动后立刻播第一首 |
| `shuffle` | bool | 随机播放 |
| `repeat` | `off` / `one` / `all` | 循环模式 |
| `window` | `width` / `height` | 渲染宽度与列表行数 |

所有字段都有默认值，且开了 `deny_unknown_fields` —— 写错字段名会直接报错而不是被静默忽略。

播放列表本身也是 JSON，存在 `playlists_dir/<名字>.json`，形如：

```json
{
  "name": "work",
  "tracks": [
    { "path": "/home/you/Music/a.mp3", "title": "a", "artist": "Album1" }
  ]
}
```

## 代码结构

```
src/
  lib.rs        crate 出口，导出下面三个模块
  config.rs     配置结构体、加载/写回/夹紧、主题配色
  playlist.rs   Track / Playlist / Library：目录扫描、随机顺序、前进后退历史
  player.rs     rodio 封装：打开设备、解码、播放/暂停、音量、时长探测
  main.rs       CLI 参数、REPL 主循环、菜单渲染、命令分发
```

`lib` + `bin` 拆分是为了能跑单元测试（`cargo test`）并让别的 crate 复用这几块逻辑。

### 几个值得注意的点

**格式支持有边界，踩到了会明确告诉你原因。** 实测结果：

| 扩展名 | 状态 |
| --- | --- |
| `mp3` / `flac` / `wav` / `ogg` / `aac` / `aiff` | 正常 |
| `m4a` / `mp4` / `mov` | 正常，**但要求 `moov` 在文件前部** |
| `opus` | **不支持** |

两个坑：

1. **m4a 的 `moov` 必须在文件开头。** rodio 0.22 底层的 symphonia 解析不了 `moov` 在末尾的 MP4。`ffmpeg` 默认就是这么存的，所以它转出来的 m4a 一律放不了，需要 `ffmpeg -i in.m4a -c copy -movflags +faststart out.m4a` 转一下（`-c copy` 不重编码，无损且很快）。iTunes/手机录的通常本来就是前置的，不受影响。

2. **Opus 无解。** rodio 0.22 没有任何 Opus 支持——它的 `lewton` 特性是 Vorbis 不是 Opus，symphonia 也不带 Opus codec。想支持得引入 `symphonia-adapter-libopus`（依赖 C 库 libopus，构建复杂度上一个台阶），termusic 就是这么做的。

遇到解不了的曲目不会静默跳过，会在状态栏报出来并给出上面这些可操作的原因。

**主循环是轮询的，不是阻塞读。** `main.rs` 把 stdin 丢进一个后台线程，用 `mpsc` 传回主线程，主循环 `recv_timeout(200ms)`。这样你在提示符前发呆时，进度条照样走、歌曲播完照样自动切下一首。渲染前先比对 `App::signature()`，状态没变就不重画，避免终端闪烁。

**没有音频设备不会崩。** `Player::new` 拿不到默认设备时降级成静音模式，用自己的时钟走完时长。`--no-audio` 强制走这条路径，无头服务器上照样能开发和调试。

**时长靠解码采样数算。** `probe_duration` 把解码器 `count()` 一遍，用 `样本数 / 采样率` 得到时长。代价是每次切歌要把整个文件解码一次（只计数不混音），大文件上会有停顿。真要优化就在建库时批量探测并缓存。

**rodio 0.22 的 API 已经不是老样子了。** 老教程里的 `OutputStream::try_default()` / `Sink` 都没了，现在是 `DeviceSinkBuilder::open_sink_or_fallback()` 拿到 `MixerDeviceSink`，再 `Player::connect_new(sink.mixer())`。`MixerDeviceSink` 必须活到 `Player` 结束，`Drop` 会停流。

**中文宽度。** `{:<12}` 按字符数补位，CJK 实际占两列，所以 help 和曲目列表要自己算显示宽度，见 `main.rs` 的 `char_width` / `display_width`。

## 依赖

`rodio` 0.22（音频）、`serde` + `serde_json`（配置）、`walkdir`（递归扫描）、`dirs`（配置目录）、`anyhow`（错误）。

另外有个 `examples/probe.rs`，直接调 rodio 解码器逐个文件探测采样率/声道/时长，遇到放不了的格式用它快速定位：

```bash
cargo run --release --example probe -- ~/Music/*
```

## 开发

```bash
cargo test              # 33 个单元测试
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

`testdata/` 里是几个用脚本生成的短 WAV，可以直接拿来试：

```bash
./target/release/music-shell --config /dev/stdin <<'EOF'
{ "library_path": "/data/music_rust/testdata", "volume": 70 }
EOF
```

## CI 与发布

- `.github/workflows/ci.yml` —— push / PR 触发：格式检查、clippy（`-D warnings`）、测试，以及 Linux / macOS / Windows 三平台构建 + `--help` 冒烟。
- `.github/workflows/release.yml` —— 推 `v*` tag 触发：构建 5 个目标并挂到 Release。

  | 目标 | 产物 |
  | --- | --- |
  | `x86_64-unknown-linux-gnu` | `music-shell-x86_64-unknown-linux-gnu.tar.gz` |
  | `aarch64-apple-darwin` | `music-shell-aarch64-apple-darwin.tar.gz` |
  | `x86_64-apple-darwin` | `music-shell-x86_64-apple-darwin.tar.gz` |
  | `x86_64-pc-windows-msvc` | `music-shell-x86_64-pc-windows-msvc.zip` |

没有 musl 静态版：rodio 走 cpal → `alsa-sys`，ALSA 是 glibc 专属的，交叉编译到 musl 需要自建 ALSA sysroot。与其发一个静态但发不出声的二进制，不如只发动态链接版（目标机器 `apt install libasound2` 即可）。

触发一次发布：

```bash
git tag v0.1.0 && git push origin v0.1.0
```

也可以在 Actions 页面手动跑 `Release`，填入 tag 名。