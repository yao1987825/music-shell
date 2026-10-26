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
    /// 正在输入、还没回车的那行文字。界面上把它显示在提示符后面，
    /// 不然用户敲了半天不知道敲了什么。
    Pending(String),
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
    /// 空格。没有正在输入命令时直接当暂停键
    Space,
    /// 其他无法识别的转义序列，只用来清掉终端回显的内容
    Unknown,
}

impl Input {
    pub fn to_command(self) -> Option<Command> {
        match self {
            Self::Line(line) => Some(Command::parse(&line)),
            Self::Key(Key::Up) => Some(Command::CursorUp),
            Self::Key(Key::Down) => Some(Command::CursorDown),
            Self::Key(Key::Left) => Some(Command::PageUp),
            Self::Key(Key::Right) => Some(Command::PageDown),
            Self::Key(Key::PageUp) => Some(Command::PageUp),
            Self::Key(Key::PageDown) => Some(Command::PageDown),
            Self::Key(Key::Home) => Some(Command::CursorHome),
            Self::Key(Key::End) => Some(Command::CursorEnd),
            Self::Key(Key::Enter) => Some(Command::TogglePlay),
            Self::Key(Key::Space) => Some(Command::TogglePlay),
            Self::Key(Key::CtrlC) => Some(Command::Quit),
            Self::Key(_) | Self::Pending(_) => None,
            Self::Eof => Some(Command::Quit),
        }
    }
}

/// 输入解析器：把终端字节流变成 Input。
///
/// 正在输入、还没回车提交的命令存在 line 里；
/// 没收全的转义序列存在 pending 里，等下一次读到的字节补齐。
///
/// 关键点：普通文字不会自己变成命令。终端一个按键一次 read，
/// 如果每读到一个字节就把行交出去，"pl work" 会被拆成 "pl" 和 "work"
/// 分别执行，带空格的命令根本没法输入。
#[derive(Debug, Default)]
pub struct Parser {
    line: Vec<u8>,
    pending: Vec<u8>,
}

impl Parser {
    /// 喂进新读到的字节，返回这次能确定的输入
    pub fn feed(&mut self, data: &[u8]) -> Vec<Input> {
        let mut merged = Vec::new();
        if !self.pending.is_empty() {
            merged.extend_from_slice(&self.pending);
            self.pending.clear();
        }
        merged.extend_from_slice(data);

        let before = self.line_text();
        let mut parsed = Vec::new();
        parse_bytes(&merged, &mut self.line, &mut self.pending, &mut parsed);

        let mut out = Vec::new();
        let after = self.line_text();
        if after != before {
            out.push(Input::Pending(after));
        }
        out.extend(parsed);
        out
    }

    /// 输入结束，把还没提交的命令交出去
    pub fn finish(&mut self) -> Vec<Input> {
        let mut out = Vec::new();
        flush_line(&mut self.line, &mut out);
        out
    }

    fn line_text(&self) -> String {
        String::from_utf8_lossy(&self.line).into_owned()
    }
}

/// 把一段终端输入解析成若干个 Input。
/// 方向键之类不发换行的键必须在这里被识别出来，否则会被当成文本。
pub fn parse_stream(data: &[u8]) -> Vec<Input> {
    let mut parser = Parser::default();
    let mut out = parser.feed(data);
    out.extend(parser.finish());
    out
}

fn parse_bytes(data: &[u8], line: &mut Vec<u8>, pending: &mut Vec<u8>, out: &mut Vec<Input>) {
    let mut i = 0;

    while i < data.len() {
        match data[i] {
            b'\n' => {
                // 回车总是提交一行，哪怕内容为空（空行 = 播放/暂停）
                submit_line(line, out);
                i += 1;
            }
            b'\r' => {
                i += 1;
            }
            0x03 => {
                flush_line(line, out);
                out.push(Input::Key(Key::CtrlC));
                i += 1;
            }
            0x09 => {
                flush_line(line, out);
                out.push(Input::Key(Key::Tab));
                i += 1;
            }
            0x7f | 0x08 => {
                if line.pop().is_none() {
                    out.push(Input::Key(Key::Backspace));
                }
                i += 1;
            }
            b' ' => {
                // 空格有两种身份：手头没有命令时是暂停键，
                // 正在输入 "pl work" 这种带空格命令时就是普通字符。
                if line.iter().all(u8::is_ascii_whitespace) {
                    line.clear();
                    out.push(Input::Key(Key::Space));
                } else {
                    line.push(b' ');
                }
                i += 1;
            }
            0x1b => {
                pending.clear();
                pending.extend_from_slice(&data[i..]);
                match parse_escape(pending) {
                    Escaped::Key(key, len) => {
                        pending.clear();
                        flush_line(line, out);
                        out.push(Input::Key(key));
                        i += len;
                    }
                    Escaped::Incomplete => {
                        // 序列还没收全，剩下的等下一次读
                        return;
                    }
                    Escaped::NotEscape => {
                        pending.clear();
                        flush_line(line, out);
                        out.push(Input::Key(Key::Esc));
                        i += 1;
                    }
                }
            }
            _ => {
                line.push(data[i]);
                i += 1;
                // 单键命令敲一下就生效，不用等回车。
                // 数字和带参数的命令攒到回车再说。
                let instant = crate::command::is_instant(&String::from_utf8_lossy(line));
                if instant {
                    submit_line(line, out);
                }
            }
        }
    }
}

/// 提交一行命令，空行也算（空行 = 播放/暂停）
fn submit_line(line: &mut Vec<u8>, out: &mut Vec<Input>) {
    match String::from_utf8(std::mem::take(line)) {
        Ok(text) => out.push(Input::Line(text)),
        Err(err) => {
            let text = String::from_utf8_lossy(err.as_bytes()).into_owned();
            out.push(Input::Line(text));
        }
    }
}

/// 提交一行命令，空行就什么都不交
fn flush_line(line: &mut Vec<u8>, out: &mut Vec<Input>) {
    if line.is_empty() {
        return;
    }
    submit_line(line, out);
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
    let mut parser = Parser::default();
    let mut buf = [0u8; 1024];

    loop {
        match locked.by_ref().read(&mut buf) {
            Ok(0) => {
                for input in parser.finish() {
                    if tx.send(input).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Input::Eof);
                return;
            }
            Ok(n) => {
                // 转义序列可能被拆在两次 read 之间，解析器自己会留着等补齐
                for input in parser.feed(&buf[..n]) {
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

/// termios 相关代码只在 Unix 上存在，Windows 用不到。
#[cfg(unix)]
mod raw {
    // c_lflag 的位标志
    pub const ECHO: u32 = 0o10;
    pub const ICANON: u32 = 0o2;
    pub const ECHOE: u32 = 0o20;
    pub const ECHOK: u32 = 0o40;
    pub const ECHONL: u32 = 0o100;
    pub const IEXTEN: u32 = 0o100000;
    const TCSANOW: i32 = 0;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u32,
        pub c_oflag: u32,
        pub c_cflag: u32,
        pub c_lflag: u32,
        pub c_line: u8,
        pub c_cc: [u8; 32],
        pub c_ispeed: u32,
        pub c_ospeed: u32,
    }

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        pub fn tcsetattr(fd: i32, optional_actions: i32, termios: *const Termios) -> i32;
    }

    pub static SAVED: std::sync::Mutex<Option<Termios>> = std::sync::Mutex::new(None);

    pub fn enable() {
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            return;
        }
        let mut current: Termios = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(0, &mut current) } != 0 {
            return;
        }
        {
            let Ok(mut guard) = SAVED.lock() else {
                return;
            };
            if guard.is_some() {
                return;
            }
            *guard = Some(current);
        }
        let mut raw_mode = current;
        raw_mode.c_lflag &= !(ICANON | ECHO | ECHOE | ECHOK | ECHONL | IEXTEN);
        unsafe {
            tcsetattr(0, TCSANOW, &raw_mode);
        }
    }

    pub fn restore() {
        let Ok(mut guard) = SAVED.lock() else {
            return;
        };
        if let Some(original) = guard.take() {
            unsafe {
                tcsetattr(0, TCSANOW, &original);
            }
        }
    }

    #[cfg(test)]
    pub const ALL_FLAGS: [u32; 6] = [ECHO, ICANON, ECHOE, ECHOK, ECHONL, IEXTEN];
}

/// 把终端切到"逐字节立即交付"模式。
///
/// 必须去掉 ICANON：规范模式下内核会一直缓冲到换行才给应用，
/// 方向键这类不发换行的键就永远读不到。同时去掉 ECHO，
/// 免得 ^[ 之类被回显到命令行里。ISIG 保留，Ctrl-C 仍然有效。
pub fn enable_raw_input() {
    #[cfg(unix)]
    raw::enable();
}

/// 退出前恢复终端原状，否则关掉程序后 shell 会没有回显。
pub fn restore_terminal() {
    #[cfg(unix)]
    raw::restore();
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
    fn a_lone_space_is_the_pause_key() {
        assert_eq!(keys(b" "), vec![Key::Space]);
        assert_eq!(lines(b" "), Vec::<String>::new(), "空格不该变成命令行文本");
    }

    #[test]
    fn a_space_inside_a_command_stays_a_character() {
        assert_eq!(keys(b"pl work\n"), Vec::<Key>::new());
        assert_eq!(lines(b"pl work\n"), vec!["pl work".to_string()]);
    }

    #[test]
    fn space_and_arrows_map_to_commands() {
        assert_eq!(
            Input::Key(Key::Space).to_command(),
            Some(Command::TogglePlay)
        );
        assert_eq!(Input::Key(Key::Left).to_command(), Some(Command::PageUp));
        assert_eq!(Input::Key(Key::Right).to_command(), Some(Command::PageDown));
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
        let mut parser = Parser::default();
        let mut all = Vec::new();
        for chunk in [&b"\x1b"[..], b"[", b"B"] {
            for input in parser.feed(chunk) {
                if let Input::Key(k) = input {
                    all.push(k);
                }
            }
        }
        assert_eq!(all, vec![Key::Down]);
    }

    #[test]
    fn an_incomplete_sequence_waits_for_the_next_read() {
        let mut parser = Parser::default();
        assert_eq!(
            parser.feed(b"ab\x1b["),
            vec![Input::Pending("ab".into())],
            "没收全的序列不该产生按键"
        );
        let got = parser.feed(b"B");
        assert_eq!(
            got,
            vec![
                Input::Pending(String::new()),
                Input::Line("ab".into()),
                Input::Key(Key::Down)
            ]
        );
    }

    #[test]
    fn a_partial_command_waits_for_enter() {
        // 终端一个按键一次 read：文字不能自己变成命令，
        // 否则 "pl work" 会被拆成两条命令分别执行。
        let mut parser = Parser::default();
        assert_eq!(parser.feed(b"pl"), vec![Input::Pending("pl".into())]);
        assert_eq!(parser.feed(b" "), vec![Input::Pending("pl ".into())]);
        assert_eq!(parser.feed(b"work"), vec![Input::Pending("pl work".into())]);
        assert_eq!(
            parser.feed(b"\n"),
            vec![Input::Pending(String::new()), Input::Line("pl work".into())]
        );
    }

    #[test]
    fn a_number_only_becomes_a_command_after_enter() {
        let mut parser = Parser::default();
        assert_eq!(parser.feed(b"5"), vec![Input::Pending("5".into())]);
        assert_eq!(
            parser.feed(b"\n"),
            vec![Input::Pending(String::new()), Input::Line("5".into())]
        );
    }

    #[test]
    fn a_space_stays_a_pause_key_while_typing() {
        let mut parser = Parser::default();
        assert_eq!(parser.feed(b" "), vec![Input::Key(Key::Space)]);
        assert_eq!(parser.feed(b"pl"), vec![Input::Pending("pl".into())]);
        assert_eq!(
            parser.feed(b" "),
            vec![Input::Pending("pl ".into())],
            "命令里的空格是普通字符"
        );
        assert_eq!(
            parser.feed(b"work\n"),
            vec![Input::Pending(String::new()), Input::Line("pl work".into())]
        );
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
    }

    #[test]
    fn a_single_key_command_fires_without_enter() {
        assert_eq!(parse_stream(b"n"), vec![Input::Line("n".into())]);
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
        assert_eq!(lines(b"zz\r\n"), vec!["zz".to_string()]);
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

    #[cfg(unix)]
    #[test]
    fn termios_bit_constants_are_distinct() {
        let bits = raw::ALL_FLAGS;
        for (i, a) in bits.iter().enumerate() {
            for b in &bits[i + 1..] {
                assert_eq!(a & b, 0, "位标志 {a:#o} 与 {b:#o} 有重叠");
            }
        }
    }
}
