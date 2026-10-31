//! The operation output: lines, where the last one stays open for more text (prompts,
//! progress dots). Only the newest lines are kept.

const MAX_LOG_LINES: usize = 5_000;

#[derive(Default)]
pub struct LogState {
    lines: Vec<String>,
    last_line_open: bool,
    text: String,
    text_stale: bool,
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
                self.lines.last_mut().unwrap().push_str(first);
            }
        }
        self.lines.extend(parts.map(str::to_string));
        self.last_line_open = true;
        if self.lines.len() > MAX_LOG_LINES {
            let excess = self.lines.len() - MAX_LOG_LINES;
            self.lines.drain(..excess);
        }
        self.text_stale = true;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.last_line_open = false;
        self.text.clear();
        self.text_stale = false;
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// All lines joined; rebuilt only after new output.
    pub fn text(&mut self) -> &str {
        if self.text_stale {
            self.text = self.lines.join("\n");
            self.text_stale = false;
        }
        &self.text
    }
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
}
