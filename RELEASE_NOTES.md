# music-shell v0.6.0

终端音乐播放器。Rust 写的，纯 JSON 配置驱动，菜单式交互，rodio 播放本地音频。

## 这一版改了什么

播放中按一个键，把当前这首歌收进自己的歌单；想专门听这个歌单时一条命令切过去。

| 改动 | 说明 |
| --- | --- |
| `Alt+A` 收歌 | 播放中按 Alt+A，把当前这首歌追加进自定义歌单（默认《我的》），按路径去重，重复按只提示"已经有了" |
| `fav` 文本命令 | 同一件事的文字版，敲 `fav` 回车即可，效果和 Alt+A 一样 |
| `favorite_playlist` 配置 | `config.json` 新增该字段改歌单名，缺省是 `我的`；老配置文件不加也能跑 |
| 状态栏反馈 | 加成功显示 `已加入《我的》(第 N 首): 歌名`，歌单第一次用会自动创建 |
| `--playlist` 不再"歌单不存在就退出" | 歌单还没有（或者还是空的）时退回全库开张，状态栏提示按 Alt+A 收歌；歌单文件损坏仍然启动失败（不覆盖用户数据） |

歌单文件在 `~/.config/music-shell/playlists/`，和 `sv` / `pl` 存的是同一套格式。
Debian 镜像里配套的 `music my` 命令会直接进这个歌单播放。

按键速查：

| 按键 | 作用 |
| --- | --- |
| `↑` / `↓` | 上一首 / 下一首（选中即播放） |
| `←` / `→` | 上一页 / 下一页，每页 10 首 |
| `空格` | 播放 / 暂停 / 继续 |
| `Alt+A` | 当前这首歌加入自定义歌单 |
| `n` / `b` | 下一首 / 上一首 |
| `<数字>` + 回车 | 跳到第 N 首并播放 |
| `q` `l` `r` `c` `w` `?` `+` `-` `fav` | 单键命令，敲一下就生效 |
| `p` / `s` + 回车 | 播放暂停 / 随机开关（后面可能跟参数，所以要回车） |

## 支持格式

| 扩展名 | 状态 |
| --- | --- |
| mp3 / flac / wav / ogg / aac / aiff | 正常 |
| m4a / mp4 / mov | 正常，但要求 `moov` 在文件前部 |
| opus | 不支持（rodio 0.22 无 Opus 解码器） |

m4a 报"格式不识别"多半是 `moov` 在文件末尾，转换一下即可：
`ffmpeg -i in.m4a -c copy -movflags +faststart out.m4a`

## Linux 依赖

运行时需要 ALSA（`libasound2`，Debian/Ubuntu 一般已预装）。
从源码编译还需要 ALSA 头文件：
`sudo apt install libasound2-dev pkg-config build-essential`

没有声卡的机器加 `--no-audio`，走静音模式，界面和状态机照常工作。

## 运行方式

```bash
# 交互式使用（需要真终端）
music-shell
# 只听某个歌单
music-shell --playlist 我的
# SSH 场景
ssh -t user@host 'export PATH=$HOME/bin:$PATH; exec music-shell'
```

配置文件默认在 `~/.config/music-shell/config.json`，首次运行自动生成。
用 `MUSIC_SHELL_CONFIG` 环境变量可以覆盖路径。

常用命令：`空格` 播放/暂停、`↑`/`↓` 上下曲、`←`/`→` 翻页、`+/-` 音量、
`Alt+A` 收歌、`r` 循环、`pl <名字>` 加载播放列表、`w` 把当前状态写回配置、`?` 帮助、`q` 退出。

## 已知限制

- **不支持 Opus。** 需要引入 `symphonia-adapter-libopus`（依赖 C 库 libopus）。
- **m4a 要求 `moov` 在文件前部。** 见上。
- **时长探测靠完整解码。** 每次切歌要把文件解码一遍数采样，长文件会卡一下。
- **不读音频标签。** 曲名取自文件名，艺术家取自父目录名。
- **格式判定基于扩展名。** 扩展名对但内容不对的文件，扫描时发现不了，播放时才报错。
- **SSH 断开进程即终止。** 长期跑请用 tmux 或 systemd --user。
- **跨 read 的 ESC 不算 Alt。** 终端把 Alt+A 发成一次 `ESC a`；若极端情况拆成两次 read，会按"先 Esc 后 a"处理，这次收歌会丢——重按一次即可。

## 校验

- 161 个单元测试，`cargo test` 全绿
- `cargo clippy --all-targets -- -D warnings` 零告警
- CI 在 Linux / macOS / Windows 三平台构建并冒烟
- 发布前会校验 Cargo.toml 版本号与 tag 一致，不一致直接失败
