//! .NET behaviour the compilers rely on: `File.ReadAllText` decoding, `ReadAllLines` line
//! splitting, `Directory.GetFiles` order and wildcard matching, `File.Move`, and the
//! `int.TryParse` / `bool.TryParse` rules.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// `File.ReadAllText`: BOM-detected UTF-8 / UTF-16, invalid UTF-8 replaced.
pub fn read_all_text(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(decode_text(&bytes))
}

pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// `File.ReadAllLines` / `File.ReadLines`: splits on "\r\n", "\n" and "\r".
pub fn read_all_lines(path: &Path) -> io::Result<Vec<String>> {
    Ok(split_lines(&read_all_text(path)?))
}

pub fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut current));
            }
            '\n' => lines.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `string.IsNullOrWhiteSpace`.
pub fn is_null_or_white_space(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

/// `Directory.GetFiles(dir, pattern, TopDirectoryOnly)`: files in the order the file system
/// returns them (NTFS: its case-insensitive name order), which is the order .NET sees. The
/// sprite sheet layout depends on it, so this does not sort. A missing folder is an error,
/// like .NET's `DirectoryNotFoundException`.
pub fn get_files(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => bail!("Could not find a part of the path '{}'.", full_path(dir).display()),
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) && !path.is_file() {
            continue;
        }
        if win32_match(pattern, &habbo_core::paths::file_name(&path)) {
            files.push(path);
        }
    }
    Ok(files)
}

/// `Directory.GetFiles(dir, pattern, AllDirectories)`: breadth first (each folder's files,
/// then its subfolders queued), as .NET's enumerator does.
pub fn get_files_recursive(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        bail!("Could not find a part of the path '{}'.", full_path(dir).display());
    }
    let mut files = Vec::new();
    let mut pending = VecDeque::from([dir.to_path_buf()]);
    while let Some(current) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push_back(path);
            } else if win32_match(pattern, &habbo_core::paths::file_name(&path)) {
                files.push(path);
            }
        }
    }
    Ok(files)
}

pub fn full_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The Win32 wildcard rules .NET uses for `Directory.GetFiles` patterns: `*`, `?`, and a `.`
/// before a wildcard (or at the end) that also matches the end of the name. Case-insensitive
/// on Windows.
pub fn win32_match(pattern: &str, name: &str) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Tok {
        Star,
        Any,
        DosDot,
        Ch(char),
    }
    let fold = |s: &str| if cfg!(windows) { s.to_lowercase() } else { s.to_string() };
    let pattern = fold(pattern);
    let name: Vec<char> = fold(name).chars().collect();

    if pattern == "*" || pattern == "*.*" {
        return true;
    }

    let pchars: Vec<char> = pattern.chars().collect();
    let mut toks = Vec::new();
    for (i, &c) in pchars.iter().enumerate() {
        toks.push(match c {
            '*' => Tok::Star,
            '?' => Tok::Any,
            '.' if i + 1 == pchars.len() || matches!(pchars[i + 1], '*' | '?') => Tok::DosDot,
            c => Tok::Ch(c),
        });
    }

    fn matches(toks: &[Tok], name: &[char]) -> bool {
        match toks.first() {
            None => name.is_empty(),
            Some(Tok::Star) => (0..=name.len()).any(|skip| matches(&toks[1..], &name[skip..])),
            Some(Tok::Any) => !name.is_empty() && matches(&toks[1..], &name[1..]),
            Some(Tok::DosDot) => {
                (name.first() == Some(&'.') && matches(&toks[1..], &name[1..]))
                    || (name.is_empty() && matches(&toks[1..], name))
            }
            Some(Tok::Ch(c)) => name.first() == Some(c) && matches(&toks[1..], &name[1..]),
        }
    }
    matches(&toks, &name)
}

/// `File.Move(source, target)` (no overwrite).
pub fn move_file(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        bail!("The file '{}' already exists.", full_path(target).display());
    }
    if !source.is_file() {
        bail!("Could not find file '{}'.", full_path(source).display());
    }
    std::fs::rename(source, target)?;
    Ok(())
}

/// `File.Move(source, target, overwrite: true)`.
pub fn move_file_overwrite(source: &Path, target: &Path) -> Result<()> {
    // rename replaces an existing file on both Windows and Unix.
    std::fs::rename(source, target)?;
    Ok(())
}

/// `int.TryParse(value, out int n)` (NumberStyles.Integer: surrounding ASCII white space, a
/// leading sign, decimal digits, 32-bit range).
pub fn try_parse_int(value: Option<&str>) -> Option<i32> {
    let value = value?;
    let trimmed = value.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' '));
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<i32>().ok()
}

/// `bool.TryParse`: "true"/"false" in any case, surrounding white space (and trailing NULs)
/// allowed.
pub fn try_parse_bool(value: Option<&str>) -> Option<bool> {
    let value = value?.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// `string.EndsWith(suffix, StringComparison.OrdinalIgnoreCase)`.
pub fn ends_with_ignore_case(s: &str, suffix: &str) -> bool {
    s.to_uppercase().ends_with(&suffix.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_follow_win32_rules() {
        assert!(win32_match("*_manifest.*", "12_hh_manifest.bin"));
        assert!(win32_match("*_manifest.*", "12_hh_manifest"));
        assert!(!win32_match("*_manifest.*", "12_hh_manifestx.bin"));
        assert!(win32_match("*_animation.bin", "3_fx_animation.bin"));
        assert!(!win32_match("*_animation.bin", "3_fx_animation.bin2"));
        assert!(win32_match("*.png", "a.png"));
        assert!(!win32_match("*.png", "a.pngx"));
    }

    #[test]
    fn int_and_bool_parsing() {
        assert_eq!(try_parse_int(Some(" -12 ")), Some(-12));
        assert_eq!(try_parse_int(Some("+3")), Some(3));
        assert_eq!(try_parse_int(Some("1.5")), None);
        assert_eq!(try_parse_int(Some("")), None);
        assert_eq!(try_parse_int(None), None);
        assert_eq!(try_parse_int(Some("99999999999")), None);
        assert_eq!(try_parse_bool(Some(" TRUE ")), Some(true));
        assert_eq!(try_parse_bool(Some("1")), None);
    }

    #[test]
    fn lines_split_like_dotnet() {
        assert_eq!(split_lines("a\r\nb\rc\n\nd"), vec!["a", "b", "c", "", "d"]);
        assert_eq!(split_lines("a\n"), vec!["a"]);
    }
}
