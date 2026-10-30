//! `IniFileParser`: reads config.ini into "Section:key" -> value.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};

/// Every `key=value` line, keyed `"<section>:<key>"` (or just `key` before any section).
/// Comment lines start with `;` or `#`.
pub fn parse(path: impl AsRef<Path>) -> Result<HashMap<String, String>> {
    let path = path.as_ref();
    if !path.is_file() {
        bail!("The configuration file '{}' was not found.", path.display());
    }
    Ok(parse_str(&std::fs::read_to_string(path)?))
}

pub fn parse_str(text: &str) -> HashMap<String, String> {
    let mut config = HashMap::new();
    let mut section: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
            section = Some(trimmed[1..trimmed.len() - 1].to_string());
            continue;
        }
        if let Some(separator) = trimmed.find('=') {
            if separator == 0 {
                continue;
            }
            let key = trimmed[..separator].trim();
            let value = trimmed[separator + 1..].trim();
            let full_key = match &section {
                Some(section) => format!("{section}:{key}"),
                None => key.to_string(),
            };
            config.insert(full_key, value.to_string());
        }
    }
    config
}

/// config.ini next to the working directory, as every C# downloader loaded it.
pub fn default_config_path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("config.ini")
}

/// Parses the default config.ini and returns `[AppSettings]` values by bare key.
pub fn app_settings() -> Result<HashMap<String, String>> {
    let all = parse(default_config_path())?;
    Ok(all
        .into_iter()
        .filter_map(|(key, value)| key.strip_prefix("AppSettings:").map(|k| (k.to_string(), value)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_comments_and_values() {
        let map = parse_str("# c\nroot=1\n[AppSettings]\n; c\nkey = a=b \n=skip\n");
        assert_eq!(map["root"], "1");
        assert_eq!(map["AppSettings:key"], "a=b");
        assert_eq!(map.len(), 2);
    }
}
