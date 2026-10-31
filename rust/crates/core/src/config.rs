//! `ConfigIniFile`: edits config.ini in place. Only changed values are rewritten;
//! comments, order and unknown keys stay. Keys live in `[AppSettings]`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

const SECTION_HEADER: &str = "[AppSettings]";

pub struct ConfigIniFile {
    path: PathBuf,
    lines: Vec<String>,
    exists: bool,
}

impl ConfigIniFile {
    pub fn default_path() -> PathBuf {
        crate::ini::default_config_path()
    }

    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = match path {
            Some(path) => std::path::absolute(path)?,
            None => Self::default_path(),
        };
        let exists = path.is_file();
        let lines = if exists {
            std::fs::read_to_string(&path)?.lines().map(str::to_string).collect()
        } else {
            Vec::new()
        };
        Ok(Self { path, lines, exists })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    /// The value of a key in `[AppSettings]`, or `None` when the key is not there.
    pub fn get(&self, key: &str) -> Option<String> {
        self.find_key(key).map(|index| value_of(&self.lines[index]))
    }

    /// Replaces the key's line, or adds it at the end of `[AppSettings]`.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if key.trim().is_empty() || key.contains(['=', '\r', '\n', '[', ']']) {
            bail!("Invalid setting name '{key}'.");
        }
        if value.contains(['\r', '\n']) {
            bail!("{key}: a value must be on one line.");
        }

        let line = format!("{key}={}", value.trim());
        if let Some(index) = self.find_key(key) {
            self.lines[index] = line;
            return Ok(());
        }

        let (start, end) = self.ensure_section();
        // After the last setting of the section, so it does not land under the next section's comment block.
        let mut insert_at = start + 1;
        for i in start + 1..end {
            if key_of(&self.lines[i]).is_some() {
                insert_at = i + 1;
            }
        }
        self.lines.insert(insert_at, line);
        Ok(())
    }

    /// Writes the file atomically; the previous version is kept as config.ini.bak.
    pub fn save(&self) -> Result<()> {
        if let Some(directory) = self.path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        if self.path.is_file() {
            std::fs::copy(&self.path, append_ext(&self.path, ".bak"))?;
        }
        let temporary = append_ext(&self.path, &format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut text = self.lines.join(crate::paths::NEWLINE);
        text.push_str(crate::paths::NEWLINE);
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }

    fn find_key(&self, key: &str) -> Option<usize> {
        let (start, end) = self.section_range()?;
        (start + 1..end).find(|&i| key_of(&self.lines[i]).is_some_and(|k| k.eq_ignore_ascii_case(key)))
    }

    fn section_range(&self) -> Option<(usize, usize)> {
        let start = self.lines.iter().position(|line| line.trim().eq_ignore_ascii_case(SECTION_HEADER))?;
        let end = (start + 1..self.lines.len())
            .find(|&i| is_section_header(&self.lines[i]))
            .unwrap_or(self.lines.len());
        Some((start, end))
    }

    fn ensure_section(&mut self) -> (usize, usize) {
        if let Some(range) = self.section_range() {
            return range;
        }
        if self.lines.last().is_some_and(|line| !line.trim().is_empty()) {
            self.lines.push(String::new());
        }
        self.lines.push(SECTION_HEADER.to_string());
        (self.lines.len() - 1, self.lines.len())
    }
}

pub(crate) fn append_ext(path: &Path, suffix: &str) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(suffix);
    PathBuf::from(os)
}

fn is_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[') && trimmed.ends_with(']')
}

fn key_of(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(['#', ';', '[']) {
        return None;
    }
    match trimmed.find('=') {
        Some(separator) if separator > 0 => Some(trimmed[..separator].trim()),
        _ => None,
    }
}

fn value_of(line: &str) -> String {
    let trimmed = line.trim();
    let separator = trimmed.find('=').map_or(0, |i| i + 1);
    trimmed[separator..].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_replaces_and_inserts_after_last_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ini");
        std::fs::write(&path, "[AppSettings]\n# note\na=1\n\n# next block\n[Other]\nb=2\n").unwrap();

        let mut config = ConfigIniFile::load(Some(&path)).unwrap();
        assert_eq!(config.get("A").as_deref(), Some("1"));
        assert_eq!(config.get("b"), None);
        config.set("a", " 5 ").unwrap();
        config.set("c", "3").unwrap();
        config.save().unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, ["[AppSettings]", "# note", "a=5", "c=3", "", "# next block", "[Other]", "b=2"]);
        assert!(dir.path().join("config.ini.bak").is_file());
    }

    #[test]
    fn rejects_bad_names() {
        let mut config = ConfigIniFile { path: PathBuf::new(), lines: vec![], exists: false };
        assert!(config.set("a=b", "1").is_err());
        assert!(config.set("a", "1\n2").is_err());
    }
}
