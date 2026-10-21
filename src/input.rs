use std::io::{self, Write};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::command::Command;

/// 主循环的心跳。状态变化、进度条、按键都在这个节奏上处理。
pub const TICK: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// 一整行文本输入
    Line(String),
    /// 单个功能键
    Key(Key),
    /// 输入结束（EOF 或 Ctrl-D）
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Tab,
    Backspace,
    CtrlC,
    PageUp,
    PageDown,
    Home,
    End,
    /// 其他无法识别的转义序列，只用来清掉终端回显的内容
    Unknown,
}

impl Input {
    pub fn to_command(self) -> Option<Command> {
        match self {
            Self::Line(line) => Some(Command::parse(&line)),
            Self::Key(Key::Up) => Some(Command::CursorUp),
            Self::Key(Key::Down) => Some(Command::CursorDown),
            Self::Key(Key::PageUp) => Some(Command::PageUp),
            Self::Key(Key::PageDown) => Some(Command::PageDown),
            Self::Key(Key::Home) => Some(Command::CursorHome),
            Self::Key(Key::End) => Some(Command::CursorEnd),
            Self::Key(Key::Enter) => Some(Command::TogglePlay),
            Self::Key(Key::CtrlC) => Some(Command::Quit),
            Self::Key(_) => None,
            Self::Eof => Some(Command::Quit),
        }
    }
}

/// 把一段终端输入解析成若干个 Input。
/// 方向键之类不发换行的键必须在这里被识别出来，否则会被当成文本。
pub fn parse_stream(data: &[u8]) -> Vec<Input> {
    let mut out = Vec::new();
    let mut line = Vec::new();
    let mut i = 0;

    while i < data.len() {
        match data[i] {
            b'\n' => {
                // 回车总是提交一行，哪怕内容为空（空行 = 播放/暂停）
                let taken = std::mem::take(&mut line);
                out.push(Input::Line(String::from_utf8_lossy(&taken).into_owned()));
                i += 1;
            }
            b'\r' => {
                i += 1;
            }
            0x03 => {
                flush_line(&mut line, &mut out);
                out.push(Input::Key(Key::CtrlC));
                i += 1;
            }
            0x09 => {
                flush_line(&mut line, &mut out);
                out.push(Input::Key(Key::Tab));
                i += 1;
            }
            0x7f | 0x08 => {
                if line.pop().is_none() {
                    out.push(Input::Key(Key::Backspace));
                }
                i += 1;
            }
            0x1b => match parse_escape(&data[i..]) {
                Escaped::Key(key, len) => {
                    flush_line(&mut line, &mut out);
                    out.push(Input::Key(key));
                    i += len;
                }
                Escaped::Incomplete => {
                    // 序列还没收全，剩下的都当成普通字符处理
                    line.push(data[i]);
                    i += 1;
                }
                Escaped::NotEscape => {
                    flush_line(&mut line, &mut out);
                    out.push(Input::Key(Key::Esc));
                    i += 1;
                }
            },
            _ => {
                line.push(data[i]);
                i += 1;
            }
        }
    }
    flush_line(&mut line, &mut out);
    out
}

fn flush_line(line: &mut Vec<u8>, out: &mut Vec<Input>) {
    if line.is_empty() {
        return;
    }
    match String::from_utf8(std::mem::take(line)) {
        Ok(text) => out.push(Input::Line(text)),
        Err(err) => {
            let text = String::from_utf8_lossy(err.as_bytes()).into_owned();
            out.push(Input::Line(text));
        }
    }
}

enum Escaped {
    Key(Key, usize),
    NotEscape,
    Incomplete,
}

/// 解析 CSI / SS3 序列，返回键和它占用的字节数
fn parse_escape(data: &[u8]) -> Escaped {
    if data.len() < 2 {
        return Escaped::Incomplete;
    }
    match data[1] {
        b'[' => parse_csi(data),
        b'O' => {
            if data.len() < 3 {
                return Escaped::Incomplete;
            }
            let key = match data[2] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                _ => Key::Unknown,
            };
            Escaped::Key(key, 3)
        }
        _ => Escaped::NotEscape,
    }
}

fn parse_csi(data: &[u8]) -> Escaped {
    // CSI 形如 ESC [ <参数> <中间字符> <最终字符>
    let mut i = 2;
    while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
        i += 1;
    }
    if i >= data.len() {
        return Escaped::Incomplete;
    }
    let final_byte = data[i];
    let len = i + 1;

    // 带参数的功能键：ESC [ 5 ~ 之类是 PageUp/PageDown
    if final_byte == b'~' {
        let params: Vec<u8> = data[2..i].to_vec();
        let n = std::str::from_utf8(&params)
            .ok()
            .and_then(|s| s.parse::<u8>().ok());
        let key = match n {
            Some(1) | Some(7) => Key::Home,
            Some(3) => Key::Unknown,
            Some(4) | Some(8) => Key::End,
            Some(5) => Key::PageUp,
            Some(6) => Key::PageDown,
            _ => Key::Unknown,
        };
        return Escaped::Key(key, len);
    }

    let key = match final_byte {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        _ => Key::Unknown,
    };
    Escaped::Key(key, len)
}

/// 后台线程持续读 stdin，把解析好的 Input 送回主线程
pub fn spawn_reader() -> mpsc::Receiver<Input> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || reader_loop(tx));
    rx
}

fn reader_loop(tx: Sender<Input>) {
    use io::Read;
    let stdin = io::stdin();
    let mut locked = stdin.lock();
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 1024];

    loop {
        match locked.by_ref().read(&mut buf) {
            Ok(0) => {
                for input in parse_stream(&pending) {
                    if tx.send(input).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Input::Eof);
                return;
            }
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                // 转义序列可能被拆在两次 read 之间，先取出能取的部分，
                // 剩下的不完整前缀留在缓冲区里等下一次 read
                let drained = drain_complete(&mut pending);
                for input in drained {
                    if tx.send(input).is_err() {
                        return;
                    }
                }
            }
            Err(_) => {
                let _ = tx.send(Input::Eof);
                return;
            }
        }
    }
}

/// 解析缓冲区里已经完整的部分，末尾残缺的转义前缀保持不动
fn drain_complete(pending: &mut Vec<u8>) -> Vec<Input> {
    let keep = trailing_incomplete(pending);
    let ready = pending.len() - keep;
    let rest = pending.split_off(ready);
    let out = parse_stream(pending);
    *pending = rest;
    out
}

fn trailing_incomplete(data: &[u8]) -> usize {
    for start in (0..data.len()).rev() {
        if data[start] != 0x1b {
            continue;
        }
        return match parse_escape(&data[start..]) {
            Escaped::Incomplete => data.len() - start,
            _ => 0,
        };
    }
    0
}

// termios 的 lflag 位
const ECHO: u32 = 0o10;
const ICANON: u32 = 0o2;
const ECHOE: u32 = 0o20;
const ECHOK: u32 = 0o40;
const ECHONL: u32 = 0o100;
const IEXTEN: u32 = 0o100000;
const TCSANOW: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

unsafe extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, optional_actions: i32, termios: *const Termios) -> i32;
}

static SAVED: std::sync::Mutex<Option<Termios>> = std::sync::Mutex::new(None);

/// 把终端切到"逐字节立即交付"模式。
///
/// 必须去掉 ICANON：规范模式下内核会一直缓冲到换行才给应用，
/// 方向键这类不���换行的键就永远读不到。同时去掉 ECHO，
/// 免得 ^[ 之类被回显到命令行里。ISIG 保留，Ctrl-C 仍然有效。
pub fn enable_raw_input() {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return;
    }
    let mut current: Termios = unsafe { std::mem::zeroed() };
    if unsafe { tcgetattr(0, &mut current) } != 0 {
        return;
    }
    if SAVED.lock().map(|g| g.is_some()).unwrap_or(true) {
        return;
    }
    if let Ok(mut guard) = SAVED.lock() {
        *guard = Some(current);
    }
    let mut raw = current;
    raw.c_lflag &= !(ICANON | ECHO | ECHOE | ECHOK | ECHONL | IEXTEN);
    unsafe {
        tcsetattr(0, TCSANOW, &raw);
    }
}

/// 退出前恢复终端原状，否则关掉程序后 shell 会没有回显。
pub fn restore_terminal() {
    let original = match SAVED.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };
    if let Some(original) = original {
        unsafe {
            tcsetattr(0, TCSANOW, &original);
        }
    }
}

pub fn print_out(text: &str) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = write!(lock, "{text}");
    let _ = lock.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(data: &[u8]) -> Vec<Key> {
        parse_stream(data)
            .into_iter()
            .filter_map(|i| match i {
                Input::Key(k) => Some(k),
                _ => None,
            })
            .collect()
    }

    fn lines(data: &[u8]) -> Vec<String> {
        parse_stream(data)
            .into_iter()
            .filter_map(|i| match i {
                Input::Line(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn arrow_keys_are_recognised() {
        assert_eq!(keys(b"\x1b[A"), vec![Key::Up]);
        assert_eq!(keys(b"\x1b[B"), vec![Key::Down]);
        assert_eq!(keys(b"\x1b[C"), vec![Key::Right]);
        assert_eq!(keys(b"\x1b[D"), vec![Key::Left]);
    }

    #[test]
    fn three_arrows_produce_three_keys() {
        assert_eq!(
            keys(b"\x1b[B\x1b[B\x1b[B"),
            vec![Key::Down, Key::Down, Key::Down]
        );
    }

    #[test]
    fn arrow_bytes_split_across_reads_reassemble() {
        let mut pending: Vec<u8> = Vec::new();
        let mut all = Vec::new();
        for chunk in [&b"\x1b"[..], b"[", b"B"] {
            pending.extend_from_slice(chunk);
            for input in drain_complete(&mut pending) {
                if let Input::Key(k) = input {
                    all.push(k);
                }
            }
        }
        assert_eq!(all, vec![Key::Down]);
        assert!(pending.is_empty(), "完整序列不该有残留");
    }

    #[test]
    fn an_incomplete_sequence_stays_in_the_buffer() {
        let mut pending: Vec<u8> = Vec::new();
        pending.extend_from_slice(b"ab\x1b[");
        let got = drain_complete(&mut pending);
        assert_eq!(got, vec![Input::Line("ab".into())]);
        assert_eq!(pending, b"\x1b[".to_vec());
    }

    #[test]
    fn arrows_never_leak_into_the_command_line() {
        assert!(
            lines(b"\x1b[B\x1b[B").is_empty(),
            "方向键不能变成命令行文本"
        );
        assert!(lines(b"\x1b[A").is_empty());
    }

    #[test]
    fn home_end_and_page_keys_are_recognised() {
        assert_eq!(keys(b"\x1b[H"), vec![Key::Home]);
        assert_eq!(keys(b"\x1b[F"), vec![Key::End]);
        assert_eq!(keys(b"\x1bOH"), vec![Key::Home]);
        assert_eq!(keys(b"\x1b[5~"), vec![Key::PageUp]);
        assert_eq!(keys(b"\x1b[6~"), vec![Key::PageDown]);
    }

    #[test]
    fn arrows_and_text_can_be_mixed() {
        // 方向键会先提交当前已输入的行，再作为独立按键发出
        assert_eq!(
            parse_stream(b"abc\x1b[Bdef\n"),
            vec![
                Input::Line("abc".into()),
                Input::Key(Key::Down),
                Input::Line("def".into())
            ]
        );
        assert_eq!(
            parse_stream(b"\x1b[Bn\n"),
            vec![Input::Key(Key::Down), Input::Line("n".into())]
        );
    }

    #[test]
    fn a_lone_enter_submits_an_empty_line() {
        assert_eq!(parse_stream(b"\n"), vec![Input::Line(String::new())]);
    }

    #[test]
    fn ctrl_c_is_caught() {
        assert_eq!(keys(b"\x03"), vec![Key::CtrlC]);
    }

    #[test]
    fn carriage_return_does_not_create_extra_lines() {
        assert_eq!(lines(b"q\r\n"), vec!["q".to_string()]);
    }

    #[test]
    fn incomplete_escape_does_not_panic() {
        let _ = parse_stream(b"\x1b");
        let _ = parse_stream(b"\x1b[");
        let _ = parse_stream(b"\x1b[1");
    }

    #[test]
    fn non_utf8_bytes_do_not_panic() {
        let _ = parse_stream(&[0xff, 0xfe, b'\n']);
    }

    #[test]
    fn keys_map_to_commands() {
        assert_eq!(Input::Key(Key::Up).to_command(), Some(Command::CursorUp));
        assert_eq!(
            Input::Key(Key::Down).to_command(),
            Some(Command::CursorDown)
        );
        assert_eq!(Input::Key(Key::Esc).to_command(), None);
        assert_eq!(Input::Line("n".into()).to_command(), Some(Command::Next));
    }
}

#[cfg(test)]
mod raw_mode_tests {
    use super::*;

    #[test]
    fn raw_mode_helpers_are_safe_when_not_a_terminal() {
        enable_raw_input();
        restore_terminal();
    }

    #[test]
    fn termios_bit_constants_are_distinct() {
        let bits = [ECHO, ICANON, ECHOE, ECHOK, ECHONL, IEXTEN];
        for (i, a) in bits.iter().enumerate() {
            for b in &bits[i + 1..] {
                assert_eq!(a & b, 0, "位标志 {a:#o} 与 {b:#o} 有重叠");
            }
        }
    }
}
