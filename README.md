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

## 安装

**直接下载 Release**（推荐，四个平台预编译）：

| 平台 | 产物 |
| --- | --- |
| Linux x86_64 | `music-shell-x86_64-unknown-linux-gnu.tar.gz` |
| Linux x86_64 静态 | 无（ALSA 是 glibc 专属的，见「已知限制」） |
| macOS Apple Silicon | `music-shell-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `music-shell-x86_64-apple-darwin.tar.gz` |
| Windows x64 | `music-shell-x86_64-pc-windows-msvc.zip` |

```bash
tar xzf music-shell-x86_64-unknown-linux-gnu.tar.gz
sudo install -m755 music-shell ~/.local/bin/
```

**从源码装**：

```bash
cargo install --path .
```

装到 `~/.cargo/bin`，需要 ALSA 运行库（`libasound2`，Debian/Ubuntu 一般已预装）。

**手动编译**：

Debian 需要先装 ALSA 头文件，rodio 靠它链接 `libasound`：

```bash
sudo apt install libasound2-dev pkg-config build-essential
cargo build --release
./target/release/music-shell
```

没声卡（服务器 / SSH）也能跑，加 `--no-audio` 走静音模式，状态机和进度照常工作。

## 自更新

```bash
music-shell update            # 检查并安装新版本
music-shell update --check    # 只看有没有新版本，不下载
music-shell update --retries 5   # 网络不好时多试几次
```

流程：查 GitHub 最新 Release → 按当前平台挑产物 → 下载 → **校验 SHA256**
→ 解压 → 写临时文件 → 原子替换。校验和不匹配会直接中止，不会装上。

Release 必须带 `checksums.txt`，没有的话命令会拒绝自动安装并提示手动下载。

Windows 上无法覆盖正在运行的 exe，会提示手动下载。

**走代理或镜像**

```bash
export HTTPS_PROXY=socks5://192.168.6.1:1070   # 或 ALL_PROXY
music-shell update

export MUSIC_SHELL_UPDATE_URL=https://mirror.example.com   # 自定义 API
music-shell update
```

没设代理就是直连。GitHub 在部分网络下不稳，命令默认重试 3 次。

## 命令

方向键是主力，命令是补充。

| 按键 | 作用 |
| --- | --- |
| `↑` / `↓` | 上一首 / 下一首，选中哪首就播哪首 |
| `←` / `→` | 上一页 / 下一页，每页 10 首 |
| `空格` | 播放 / 暂停 / 继续 |
| `n` / `b` | 下一首 / 上一首 |
| `p` + 回车 | 播放 / 暂停 / 继续 |
| `s` + 回车 | 随机播放开关 |
| `<数字>` + 回车 | 跳到第 N 首并播放 |
| `+` / `-` | 音量 ±5 |
| `l` | 重新扫描音乐库 |
| `pl` / `pl <名字>` | 列出 / 加载播放列表 |
| `sv <名字>` | 把当前列表存成播放列表 |
| `r` | 循环模式 off → one（单曲循环）→ all（列表循环） |
| `c` | 重新载入 `config.json` |
| `w` | 把当前音量 / 状态写回 `config.json` |
| `?` | 帮助 |
| `q` | 退出 |

单键命令（`q` `n` `b` `l` `r` `c` `w` `?` `+` `-`）敲一下就生效。
数字和带参数的命令要按回车，否则 `12` 会被当成第 1 首。
正在输入的文字会显示在提示符后面，看得见自己敲了什么。

## 启动参数

| 参数 | 说明 |
| --- | --- |
| `--config <路径>` | 指定配置文件，默认 `~/.config/music-shell/config.json` |
| `--playlist <名字>` | 启动时加载某个播放列表（也可用 `--playlist=名字`） |
| `--no-audio` | 静音模式，不开音频设备 |
| `--print-config` | 打印当前配置后退出 |
| `-V, --version` | 显示版本号 |

## 配置

首次运行会在 `~/.config/music-shell/config.json` 生成一份默认配置，之后用 `w` 命令回写。

首次运行生成的默认配置长这样（和 `--print-config` 的输出逐字段一致）：

```json
{
  "app_name": "music-shell",
  "theme": "dark",
  "volume": 70,
  "library_path": "/home/you/Music",
  "playlists_dir": "/home/you/.config/music-shell/playlists",
  "auto_play": false,
  "shuffle": false,
  "repeat": "off",
  "recent_playlists": [],
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
  lib.rs        crate 出口，导出下面这些模块
  main.rs       只负责启动和退出：解析参数、装配 App、跑 REPL
  app.rs        状态流转：应用命令、推进播放状态机、REPL 循环
  config.rs     只负责 JSON 读取、校验、写回
  command.rs    只负责命令解析：输入字符串 → Command 枚举
  renderer.rs   只负责终端渲染：整屏绘制 + 增量刷新
  library.rs    只负责音乐库：扫描、播放列表、shuffle/next/prev
  player.rs     只负责 rodio 播放状态：设备、解码、状态机、逻辑时钟
  capability.rs 格式能力判定：可播放 / 需转码 / 不支持
```

职责划分：

| 模块 | 职责 | 不知道的东西 |
| --- | --- | --- |
| `main.rs` | 启动和退出 | 一切业务细节 |
| `App`（app.rs） | 状态流转 | 怎么画、怎么解析 |
| `Renderer`（renderer.rs） | 画 | 播放器状态怎么来的 |
| `Command`（command.rs） | 解析 | 命令会造成什么后果 |
| `Library`（library.rs） | 音乐库和队列 | 音频怎么放、界面怎么画 |
| `Player`（player.rs） | 播放状态机 | 界面长什么样 |
| `Capability`（capability.rs） | 格式判定 | 什么都不关心 |

依赖方向是单向的：`main → app → {command, renderer, library, player, config}`，下层不回头引用上层。

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

**主循环是轮询的，不是阻塞读。** `app.rs` 把 stdin 丢进一个后台线程，用 `mpsc` 传回主线程，主循环 `recv_timeout(200ms)`。这样你在提示符前发呆时，时间照样走、歌曲播完照样自动切下一首。渲染前先比对渲染器的上一帧，状态没变就不重画，避免终端闪烁。

**终端里不画进度条。** 进度条是图形界面的东西，挤在终端里既占地方又多余。曲名下面一行直接给状态和时间（`▶ 正在播放 1:23 / 4:56`），看不看进度一眼就够。

**方向键需要把终端切到非规范模式。** 默认情况下内核走 ICANON，会一直缓冲到换行才把数据交给应用——而方向键 `[B` 不带换行，于是永远读不到，还会被当成 `^[[B` 回显到命令行里。`input.rs` 因此做了三件事：

- 启动时 `tcsetattr` 去掉 ICANON 和 ECHO，退出时恢复（否则关掉程序后 shell 会没有回显）
- 按字节而不是按行读取，用状态机识别 CSI / SS3 序列，识别不出方向键的其它转义序列会被丢弃而不是污染命令行
- 序列可能被拆在两次 `read` 之间，缓冲区末尾残缺的前缀会保留到下一轮

**一个按键一次 read，文字不能自己变成命令。** 终端是一个按键一次 `read`，如果每读到一个字节就把这一行交出去，`pl work` 会被拆成 `pl` 和 `work` 分别执行，带空格的命令根本没法输入。现在普通文字先攒在缓冲区里，攒成一个完整命令（`q` `n` `l` …）就立刻执行，数字和带参数的命令攒到回车。正在输入的内容会回显在提示符后面。

**光标就是播放头。** 上下键移动选中行，选中哪首就播哪首，不用再去输序号按回车。左右键按页翻，每页 10 首。


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

## 终端适配

- **宽度取 `config.window.width` 和真实终端宽度的较小值**，终端变窄时自动收窄、变宽时不会超出配置上限。终端尺寸变化会被实时感知（渲染循环每 200ms 查一次）。
- **所有输出行都按显示宽度裁剪**，CJK 算两列，长路径、长标题、长状态文字都会截断并加省略号，不会折行破坏版面。
- **`TERM=dumb` 或输出被重定向到文件时自动关闭 ANSI**，终端里看到的是纯文本，不会出现裸转义符。
- 无音频设备时降级为静音模式，界面照常工作，状态栏会说明原因。

## 已知限制

- **Opus 不支持。** rodio 0.22 完全没有 Opus 支持（它的 `lewton` 特性是 Vorbis 不是 Opus，symphonia 也不带 Opus codec）。要支持得引入 `symphonia-adapter-libopus`（依赖 C 库 libopus）。
- **m4a/mp4 要求 `moov` 在文件前部。** ffmpeg 默认把 `moov` 写在文件末尾，symphonia 找不到。`ffmpeg -i in.m4a -c copy -movflags +faststart out.m4a` 转一下即可（`-c copy` 不重编码）。
- **时长的探测靠完整解码。** `probe_duration` 把文件解码一遍数采样，长文件会卡一下。更大的音乐库应该建库时批量探测并缓存。
- **格式判定基于扩展名。** 一个 `.mp3` 扩展名里装的是 AAC 流这种情况，扫描时发现不了，只在播放时报解码错误。
- **没有音频标签。** 曲名取自文件名，艺术家取自父目录名，不读 ID3/Vorbis 标签。
- **SSH 断开进程即终止。** 长期跑请用 tmux 或 systemd --user。
- **自更新只支持 Linux/macOS。** Windows 不能覆盖正在运行的 exe。
- **纯终端界面。** 没有鼠标支持，没有 TUI 组件库依赖。

## 未来计划

- 读 ID3 / Vorbis / MP4 标签，取代文件名和目录名
- 建库时批量探测时长并落盘缓存
- 引入 `symphonia-adapter-libopus` 支持 Opus
- 扫描选项（`ignore_hidden` / `max_depth` / `sort_by`）接到配置文件里
- 快捷键绑定，以及可脚本化的 `--exec` 批处理模式
- 播放列表支持通配符和自动更新

## 扩展到更多格式

解码能力完全取决于 rodio 启用的 symphonia feature。改 `Cargo.toml` 里的 feature 列表即可：

```toml
rodio = { version = "0.22", default-features = false, features = [
    "playback", "mp3", "flac", "wav", "vorbis", "mp4", "symphonia-aiff",
] }
```

想加 Opus 还需要引入 `symphonia-adapter-libopus` 并在 `src/player.rs` 里接上它的解码器。改完记得同步 `src/capability.rs` 的 `Capability::of`，让界面能给出正确提示。

用这个探针快速验证某个格式能不能解：

```bash
cargo run --release --example probe -- ~/Music/*
```

## 单元测试最小用例

跑全量：

```bash
cargo test
```

只想测某一个模块：

```bash
cargo test --lib playlist          # 只跑 playlist 相关
cargo test --lib shuffle_tests     # 只跑随机播放那组
cargo test --lib config::tests::volume_converts_both_ways   # 精确到单个用例
```

加新用例的套路是：临时目录里造数据 → 调 API → 断言 → 清理。目录用 `std::env::temp_dir()` 拼一个专属名字，别和别的测试撞：

```rust
#[test]
fn 例子() {
    let dir = std::env::temp_dir().join("music_shell_test_例子");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // ... 造数据、断言 ...
    let _ = std::fs::remove_dir_all(&dir);
}
```

## 开发

```bash
cargo test              # 152 个单元测试
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

`testdata/` 里是几个用脚本生成的短 WAV，可以直接拿来试：

```bash
./target/release/music-shell --config /dev/stdin <<'EOF'
{ "library_path": "/data/music_rust/testdata", "volume": 70 }
EOF
```

## 发布说明

每个 Release 的说明模板见 [RELEASE_NOTES.md](RELEASE_NOTES.md)，覆盖支持格式、Linux 依赖、运行方式和已知限制。

## CI 与发布

三个 workflow 串成一条全自动的发布链，**不需要手工打 tag 或改版本号**：

```
bump.yml  ──手动触发，选 patch/minor/major──▶ 改 Cargo.toml → 提交推送
    │
    ▼
auto-tag.yml ──push 到 main──▶ 读 Cargo.toml 版本 → 自动打 v<版本> tag
    │
    ▼
release.yml ──tag 推送触发──▶ 构建 5 个目标 → 校验产物版本 → 挂到 Release
```

- `bump.yml` —— 手动触发，填 `patch` / `minor` / `major`，自动改 `Cargo.toml` 里的版本号并提交推送。填 `dry-run` 可以只看结果不提交。
- `auto-tag.yml` —— 每次 push 到 main 都跑，读 `Cargo.toml` 的版本号，**没有对应 tag 就自动创建并推送**（已有则跳过）。tag 推送会触发发布流程。
- `release.yml` —— tag 推送触发，构建 5 个目标并挂到 Release。
- `ci.yml` —— push / PR 触发：格式检查、clippy（`-D warnings`）、测试，以及 Linux / macOS / Windows 三平台构建 + `--help` 冒烟。

**版本号以谁为准**

发布产物的版本号**永远等于 tag**。CI 构建时通过 `MS_VERSION` 环境变量注入，`build.rs` 把它编译进二进制（`--version` 读的就是它）。如果 `Cargo.toml` 和 tag 对不上，CI 只会给个 warning 提示，产物仍按 tag 走——不会出现"装完发现版本号不对"。

本地构建时 `MS_VERSION` 没设，`build.rs` 退回用 `Cargo.toml` 里的版本号，所以本地 `cargo build` 的 `--version` 和 `Cargo.toml` 一致。

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