//! Console routing: the Rust stand-in for `Console.Write`, `Console.ForegroundColor`
//! and `Console.ReadLine`.
//!
//! Operations print with [`out!`](crate::out) / [`outln!`](crate::outln) and read with
//! [`read_line`]. By default that is the real terminal; the Professional window swaps in
//! its own [`Sink`] and [`Source`] while an operation runs (see `OperationRunner`).

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, RwLock};

/// The colours the C# code used through `ConsoleColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    Black,
    DarkBlue,
    DarkGreen,
    DarkCyan,
    DarkRed,
    DarkMagenta,
    DarkYellow,
    Gray,
    DarkGray,
    Blue,
    Green,
    Cyan,
    Red,
    Magenta,
    Yellow,
    White,
}

impl Color {
    fn ansi_background(self) -> &'static str {
        match self {
            Color::Black => "\x1b[40m",
            Color::DarkRed => "\x1b[41m",
            Color::DarkGreen => "\x1b[42m",
            Color::DarkYellow => "\x1b[43m",
            Color::DarkBlue => "\x1b[44m",
            Color::DarkMagenta => "\x1b[45m",
            Color::DarkCyan => "\x1b[46m",
            Color::Gray => "\x1b[47m",
            Color::DarkGray => "\x1b[100m",
            Color::Red => "\x1b[101m",
            Color::Green => "\x1b[102m",
            Color::Yellow => "\x1b[103m",
            Color::Blue => "\x1b[104m",
            Color::Magenta => "\x1b[105m",
            Color::Cyan => "\x1b[106m",
            Color::White => "\x1b[107m",
        }
    }

    fn ansi(self) -> &'static str {
        match self {
            Color::Black => "\x1b[30m",
            Color::DarkRed => "\x1b[31m",
            Color::DarkGreen => "\x1b[32m",
            Color::DarkYellow => "\x1b[33m",
            Color::DarkBlue => "\x1b[34m",
            Color::DarkMagenta => "\x1b[35m",
            Color::DarkCyan => "\x1b[36m",
            Color::Gray => "\x1b[37m",
            Color::DarkGray => "\x1b[90m",
            Color::Red => "\x1b[91m",
            Color::Green => "\x1b[92m",
            Color::Yellow => "\x1b[93m",
            Color::Blue => "\x1b[94m",
            Color::Magenta => "\x1b[95m",
            Color::Cyan => "\x1b[96m",
            Color::White => "\x1b[97m",
        }
    }
}

/// Where output goes. Text arrives already formatted; colour changes arrive in order with it.
pub trait Sink: Send + Sync {
    fn write(&self, text: &str);
    /// `None` resets both colours to the defaults.
    fn set_color(&self, _color: Option<Color>) {}
    fn set_background(&self, _color: Color) {}
    fn clear(&self) {}
}

/// Where `read_line` gets its lines. `None` means end of input (C#'s `ReadLine() == null`).
pub trait Source: Send + Sync {
    fn read_line(&self) -> Option<String>;
    /// `Console.ReadKey()`: waits for one key. Sources without single keys wait for a line.
    fn read_key(&self) {
        let _ = self.read_line();
    }
}

struct TerminalSink {
    lock: Mutex<()>,
    ansi: bool,
}

impl Sink for TerminalSink {
    fn write(&self, text: &str) {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }

    // Colours and clearing only on a real terminal; redirected output stays plain text.
    fn set_color(&self, color: Option<Color>) {
        if self.ansi {
            self.write(color.map_or("\x1b[0m", Color::ansi));
        }
    }

    fn set_background(&self, color: Color) {
        if self.ansi {
            self.write(color.ansi_background());
        }
    }

    fn clear(&self) {
        if self.ansi {
            self.write("\x1b[2J\x1b[3J\x1b[H");
        }
    }
}

struct TerminalSource;

impl Source for TerminalSource {
    fn read_line(&self) -> Option<String> {
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
        }
    }
}

static SINK: RwLock<Option<Arc<dyn Sink>>> = RwLock::new(None);
static SOURCE: RwLock<Option<Arc<dyn Source>>> = RwLock::new(None);

fn sink() -> Arc<dyn Sink> {
    if let Some(sink) = SINK.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return sink.clone();
    }
    let mut slot = SINK.write().unwrap_or_else(|e| e.into_inner());
    slot.get_or_insert_with(|| {
        use std::io::IsTerminal;
        let ansi = std::io::stdout().is_terminal() && enable_ansi_support::enable_ansi_support().is_ok();
        Arc::new(TerminalSink { lock: Mutex::new(()), ansi })
    })
    .clone()
}

fn source() -> Arc<dyn Source> {
    SOURCE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| Arc::new(TerminalSource))
}

/// Drops the terminal sink so the next write checks the console again (after the process got a
/// new console, e.g. `AllocConsole`). A custom sink set with [`set_sink`] is dropped too.
pub fn reinit_terminal() {
    *SINK.write().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Replaces the output sink; returns the previous one so the caller can restore it.
pub fn set_sink(new_sink: Arc<dyn Sink>) -> Arc<dyn Sink> {
    let previous = sink();
    *SINK.write().unwrap_or_else(|e| e.into_inner()) = Some(new_sink);
    previous
}

/// Replaces the input source; returns the previous one (None = the terminal).
pub fn set_source(new_source: Option<Arc<dyn Source>>) -> Option<Arc<dyn Source>> {
    std::mem::replace(&mut *SOURCE.write().unwrap_or_else(|e| e.into_inner()), new_source)
}

/// `Console.Write`.
pub fn write(text: &str) {
    if !text.is_empty() {
        sink().write(text);
    }
}

/// `Console.WriteLine`.
pub fn writeln(text: &str) {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    sink().write(&line);
}

/// `Console.ForegroundColor = color`.
pub fn set_color(color: Color) {
    sink().set_color(Some(color));
}

/// `Console.BackgroundColor = color`.
pub fn set_background(color: Color) {
    sink().set_background(color);
}

/// `Console.ResetColor()`.
pub fn reset_color() {
    sink().set_color(None);
}

/// `Console.ReadKey()`.
pub fn read_key() {
    source().read_key();
}

/// `Console.Clear()`.
pub fn clear() {
    sink().clear();
}

/// Writes one line in a colour, then resets.
pub fn writeln_colored(color: Color, text: &str) {
    set_color(color);
    writeln(text);
    reset_color();
}

/// `Console.ReadLine()`; `None` at end of input.
pub fn read_line_opt() -> Option<String> {
    source().read_line()
}

/// `Console.ReadLine() ?? ""`.
pub fn read_line() -> String {
    read_line_opt().unwrap_or_default()
}

/// Prints a prompt (no newline) and reads the answer, trimmed.
pub fn prompt(text: &str) -> String {
    write(text);
    read_line().trim().to_string()
}

/// `Console.Write` with `format!` arguments.
#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => { $crate::console::write(&format!($($arg)*)) };
}

/// `Console.WriteLine` with `format!` arguments.
#[macro_export]
macro_rules! outln {
    () => { $crate::console::writeln("") };
    ($($arg:tt)*) => { $crate::console::writeln(&format!($($arg)*)) };
}

/// One coloured line: `outc!(Color::Red, "failed: {e}")`.
#[macro_export]
macro_rules! outc {
    ($color:expr, $($arg:tt)*) => { $crate::console::writeln_colored($color, &format!($($arg)*)) };
}

/// Formats an integer with thousands separators, like C#'s `{n:N0}`.
pub fn n0<T: Into<i128>>(value: T) -> String {
    let value: i128 = value.into();
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        grouped.push('-');
    }
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n0_groups_thousands() {
        assert_eq!(n0(0), "0");
        assert_eq!(n0(999), "999");
        assert_eq!(n0(1000), "1,000");
        assert_eq!(n0(134_217_728i64), "134,217,728");
        assert_eq!(n0(-1234567i64), "-1,234,567");
    }
}
