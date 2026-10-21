# music-shell v0.4.2

终端音乐播放器。Rust 写的，纯 JSON 配置驱动，菜单式交互，rodio 播放本地音频。

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
# SSH 场景
ssh -t user@host 'export PATH=$HOME/bin:$PATH; exec music-shell'
```

配置文件默认在 `~/.config/music-shell/config.json`，首次运行自动生成。
用 `MUSIC_SHELL_CONFIG` 环境变量可以覆盖路径。

常用命令：`p` 播放/暂停、`n`/`b` 上下曲、`+/-` 音量、`s` 随机、`r` 循环、
`pl <名字>` 加载播放列表、`w` 把当前状态写回配置、`?` 帮助、`q` 退出。

## 已知限制

- **不支持 Opus。** 需要引入 `symphonia-adapter-libopus`（依赖 C 库 libopus）。
- **m4a 要求 `moov` 在文件前部。** 见上。
- **时长探测靠完整解码。** 每次切歌要把文件解码一遍数采样，长文件会卡一下。
- **不读音频标签。** 曲名取自文件名，艺术家取自父目录名。
- **格式判定基于扩展名。** 扩展名对但内容不对的文件，扫描时发现不了，播放时才报错。
- **SSH 断开进程即终止。** 长期跑请用 tmux 或 systemd --user。

## 校验

- 97 个单元测试，`cargo test` 全绿
- `cargo clippy --all-targets -- -D warnings` 零告警
- CI 在 Linux / macOS / Windows 三平台构建并冒烟
- 发布前会校验 Cargo.toml 版本号与 tag 一致，不一致直接失败
