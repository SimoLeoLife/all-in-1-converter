//! The operation output: coloured lines, where the last one stays open for more text (prompts,
//! progress dots). Only the newest lines are kept.

use eframe::egui::{text::LayoutJob, Color32, FontId, TextFormat};
use habbo_core::console::Color;

const MAX_LOG_LINES: usize = 5_000;

#[derive(Default, Clone)]
struct Line {
    /// Text runs with the console colour they were written in.
    segments: Vec<(String, Option<Color>)>,
}

impl Line {
    fn text(&self) -> String {
        self.segments.iter().map(|(text, _)| text.as_str()).collect()
    }

    fn push(&mut self, text: &str, color: Option<Color>) {
        match self.segments.last_mut() {
            Some((last, last_color)) if *last_color == color => last.push_str(text),
            _ => self.segments.push((text.to_string(), color)),
        }
    }
}

#[derive(Default)]
pub struct LogState {
    lines: Vec<Line>,
    last_line_open: bool,
    color: Option<Color>,
    text: String,
    job: Option<LayoutJob>,
    stale: bool,
}

impl LogState {
    pub fn append(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let normalized = chunk.replace("\r\n", "\n");
        let mut parts = normalized.split('\n');
        if self.last_line_open && !self.lines.is_empty() {
            if let Some(first) = parts.next() {
                let color = self.color;
                self.lines.last_mut().unwrap().push(first, color);
            }
        }
        for part in parts {
            let mut line = Line::default();
            if !part.is_empty() {
                line.push(part, self.color);
            }
            self.lines.push(line);
        }
        self.last_line_open = true;
        if self.lines.len() > MAX_LOG_LINES {
            let excess = self.lines.len() - MAX_LOG_LINES;
            self.lines.drain(..excess);
        }
        self.stale = true;
    }

    pub fn set_color(&mut self, color: Option<Color>) {
        self.color = color;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn refresh(&mut self) {
        if !self.stale {
            return;
        }
        self.text = self.lines.iter().map(Line::text).collect::<Vec<_>>().join("\n");
        self.job = None;
        self.stale = false;
    }

    /// All lines joined; rebuilt only after new output.
    pub fn text(&mut self) -> &str {
        self.refresh();
        &self.text
    }

    /// The coloured layout of [`text`](Self::text) for the log view.
    pub fn layout_job(&mut self, font: &FontId, default_color: Color32) -> LayoutJob {
        self.refresh();
        if let Some(job) = &self.job {
            return job.clone();
        }
        let mut job = LayoutJob::default();
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                job.append("\n", 0.0, TextFormat::simple(font.clone(), default_color));
            }
            for (text, color) in &line.segments {
                let color = color.map_or(default_color, console_color);
                job.append(text, 0.0, TextFormat::simple(font.clone(), color));
            }
        }
        self.job = Some(job.clone());
        job
    }

    /// The progress of the run as 0..=1, from the newest "(12.5%)" or "[ 40/120]" in the
    /// last lines; `None` when the operation prints none.
    pub fn progress(&self) -> Option<f32> {
        for line in self.lines.iter().rev().take(6) {
            let text = line.text();
            if let Some(value) = percent_in(&text) {
                return Some(value);
            }
            if let Some(value) = fraction_in(&text) {
                return Some(value);
            }
        }
        None
    }
}

/// The last "NN%" / "NN.N%" in the text.
fn percent_in(text: &str) -> Option<f32> {
    let end = text.rfind('%')?;
    let start = text[..end].rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).map_or(0, |i| i + 1);
    let value: f32 = text[start..end].replace(',', ".").parse().ok()?;
    (0.0..=100.0).contains(&value).then_some(value / 100.0)
}

/// "[ 40/120]" (spaces allowed inside the brackets).
fn fraction_in(text: &str) -> Option<f32> {
    let open = text.rfind('[')?;
    let close = open + text[open..].find(']')?;
    let (done, total) = text[open + 1..close].split_once('/')?;
    let done: f32 = done.trim().parse().ok()?;
    let total: f32 = total.trim().parse().ok()?;
    (total > 0.0 && done <= total).then_some(done / total)
}

/// Console colours, readable on the dark log background.
fn console_color(color: Color) -> Color32 {
    let hex = match color {
        Color::Black | Color::DarkGray => "#7A869C",
        Color::DarkBlue | Color::Blue => "#7AA2F7",
        Color::DarkGreen | Color::Green => "#5BD68A",
        Color::DarkCyan | Color::Cyan => "#5CCFE6",
        Color::DarkRed | Color::Red => "#F78A80",
        Color::DarkMagenta | Color::Magenta => "#C792EA",
        Color::DarkYellow | Color::Yellow => "#F5C451",
        Color::Gray => "#D3DAE6",
        Color::White => "#FFFFFF",
    };
    super::theme::hex(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_last_line_continues() {
        let mut log = LogState::default();
        log.append("Start ID: ");
        log.append("12\r\nnext\n");
        log.append("...");
        assert_eq!(log.text(), "Start ID: 12\nnext\n...");
        assert_eq!(log.line_count(), 3);
    }

    #[test]
    fn colours_follow_the_text() {
        let mut log = LogState::default();
        log.append("ok ");
        log.set_color(Some(Color::Red));
        log.append("bad\n");
        log.set_color(None);
        assert_eq!(log.lines[0].segments, vec![("ok ".to_string(), None), ("bad".to_string(), Some(Color::Red))]);
    }

    #[test]
    fn progress_from_output() {
        let mut log = LogState::default();
        log.append("⚡ [   10/40] (25.0%) |  10.8 items/sec\n");
        assert_eq!(log.progress(), Some(0.25));
        log.append("✅ 3/4 batches processed.\nhello [ 3/4]\n");
        assert_eq!(log.progress(), Some(0.75));
        assert_eq!(LogState::default().progress(), None);
        assert_eq!(percent_in("Disk Space Saved: 0,51 MB (49,7% overall reduction)"), Some(0.497));
    }
}
