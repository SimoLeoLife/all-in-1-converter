//! Run history kept across sessions: `logs/history.json` (newest first) and one log file per
//! run in `logs/`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const HISTORY_FILE: &str = "history.json";
const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub title: String,
    pub succeeded: bool,
    #[serde(default)]
    pub cancelled: bool,
    /// RFC 3339, local time.
    pub finished_at: String,
    pub elapsed_secs: f64,
    /// The saved output, relative to the working directory.
    #[serde(default)]
    pub log_file: Option<PathBuf>,
}

impl HistoryEntry {
    pub fn finished_local(&self) -> Option<chrono::DateTime<chrono::Local>> {
        chrono::DateTime::parse_from_rfc3339(&self.finished_at).ok().map(|t| t.with_timezone(&chrono::Local))
    }
}

pub struct History {
    folder: PathBuf,
    pub entries: Vec<HistoryEntry>,
}

impl History {
    pub fn load(folder: impl Into<PathBuf>) -> Self {
        let folder = folder.into();
        let entries = std::fs::read(folder.join(HISTORY_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { folder, entries }
    }

    /// Saves the run's output as a log file and puts the run first in the history.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        id: &str,
        title: &str,
        succeeded: bool,
        cancelled: bool,
        finished_at: chrono::DateTime<chrono::Local>,
        elapsed: std::time::Duration,
        log_text: &str,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.folder)?;
        let log_file = self.folder.join(format!("{}_{}.log", finished_at.format("%Y-%m-%d_%H-%M-%S"), id));
        std::fs::write(&log_file, log_text.replace('\n', habbo_core::paths::NEWLINE))?;
        self.entries.insert(
            0,
            HistoryEntry {
                id: id.to_string(),
                title: title.to_string(),
                succeeded,
                cancelled,
                finished_at: finished_at.to_rfc3339(),
                elapsed_secs: elapsed.as_secs_f64(),
                log_file: Some(log_file),
            },
        );
        // Forget the oldest runs, and their log files.
        for old in self.entries.drain(MAX_ENTRIES.min(self.entries.len())..) {
            if let Some(file) = old.log_file {
                let _ = std::fs::remove_file(file);
            }
        }
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let path = self.folder.join(HISTORY_FILE);
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&self.entries)?)?;
        std::fs::rename(temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_survive_a_reload_and_old_runs_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut history = History::load(dir.path());
        let now = chrono::Local::now();
        for i in 0..(MAX_ENTRIES + 3) {
            let at = now + chrono::Duration::seconds(i as i64);
            history.record("tools.compile-hab", "Compile HabFiles", i % 2 == 0, false, at, std::time::Duration::from_secs(2), "line 1\nline 2").unwrap();
        }
        let reloaded = History::load(dir.path());
        assert_eq!(reloaded.entries.len(), MAX_ENTRIES);
        assert!(reloaded.entries[0].finished_local().unwrap() > reloaded.entries[1].finished_local().unwrap());
        let log = reloaded.entries[0].log_file.clone().unwrap();
        assert!(std::fs::read_to_string(log).unwrap().contains("line 2"));
        // 50 logs kept + history.json
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), MAX_ENTRIES + 1);
    }
}
