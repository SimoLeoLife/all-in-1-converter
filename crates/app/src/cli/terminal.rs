//! Terminal input for the CLI: lines from stdin and single keys for "Press any key".

use crossterm::event::{self, Event, KeyEventKind};
use habbo_core::console::Source;

pub struct TerminalSource;

impl Source for TerminalSource {
    fn read_line(&self) -> Option<String> {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
        }
    }

    fn read_key(&self) {
        if crossterm::terminal::enable_raw_mode().is_err() {
            let _ = self.read_line();
            return;
        }
        loop {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// The console width, clamped like the C# `FrameWidth` (60..120, 100 when unknown).
pub fn frame_width() -> usize {
    crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(100).clamp(60, 120)
}
