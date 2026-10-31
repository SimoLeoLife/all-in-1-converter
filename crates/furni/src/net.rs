//! Small .NET compatibility helpers: `int.TryParse` / `float.TryParse` semantics,
//! `ToString()` ("G", shortest round-trip) number formatting, `File.ReadAllText` decoding,
//! and `Directory.GetFiles` enumeration order (raw directory order, like .NET).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Whitespace that .NET number parsing skips (`NumberStyles.AllowLeading/TrailingWhite`).
fn is_number_white(c: char) -> bool {
    c == ' ' || ('\u{9}'..='\u{d}').contains(&c)
}

/// `int.TryParse(value, out int)`: optional surrounding whitespace, optional sign, digits.
pub fn try_parse_int(value: Option<&str>) -> Option<i32> {
    let s = value?.trim_matches(is_number_white);
    let s = s.trim_end_matches('\0');
    let (negative, digits) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        acc = acc * 10 + (b - b'0') as i64;
        if acc > i32::MAX as i64 + 1 {
            return None;
        }
    }
    let v = if negative { -acc } else { acc };
    i32::try_from(v).ok()
}

/// `int.Parse(value)`: like [`try_parse_int`] but with the .NET error message.
pub fn parse_int(value: &str) -> anyhow::Result<i32> {
    try_parse_int(Some(value)).ok_or_else(|| anyhow::anyhow!("The input string '{value}' was not in a correct format."))
}

/// `bool.TryParse`: "true"/"false" in any case, surrounding whitespace/nulls ignored.
pub fn try_parse_bool(value: Option<&str>) -> Option<bool> {
    let s = value?.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if s.eq_ignore_ascii_case("true") {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Validates and parses a `NumberStyles.Float` number (invariant culture) as f64.
fn parse_float_text(value: &str) -> Option<f64> {
    let s = value.trim_matches(is_number_white).trim_end_matches('\0');
    if s.is_empty() {
        return None;
    }
    let (negative, body) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    // Named literals (.NET Core 3.0+ matches them case-insensitively).
    if body.eq_ignore_ascii_case("infinity") || body == "\u{221E}" {
        return Some(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if body.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut int_digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        int_digits += 1;
    }
    let mut frac_digits = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            frac_digits += 1;
        }
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if i != bytes.len() {
        return None;
    }
    let mut text = String::with_capacity(body.len() + 2);
    if negative {
        text.push('-');
    }
    if int_digits == 0 {
        text.push('0');
    }
    text.push_str(body);
    if text.ends_with('.') {
        text.push('0');
    }
    let text = text.replace(".e", ".0e").replace(".E", ".0E");
    text.parse::<f64>().ok()
}

/// `float.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out float)`.
pub fn try_parse_f32(value: Option<&str>) -> Option<f32> {
    let s = value?;
    let v = parse_float_text(s)?;
    if !v.is_finite() {
        return Some(v as f32);
    }
    let trimmed = s.trim_matches(is_number_white).trim_end_matches('\0');
    // Parse straight to f32 so the rounding is correct (no double rounding through f64).
    let mut text = trimmed.to_string();
    if let Some(rest) = text.strip_prefix('+') {
        text = rest.to_string();
    }
    let text = normalise_for_rust(&text);
    text.parse::<f32>().ok().or(Some(v as f32))
}

/// `double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out double)`.
pub fn try_parse_f64(value: Option<&str>) -> Option<f64> {
    parse_float_text(value?)
}

pub use habbo_core::dotnet::{format_f32, format_f64};

fn normalise_for_rust(text: &str) -> String {
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let mut out = String::from(sign);
    if body.starts_with('.') {
        out.push('0');
    }
    out.push_str(body);
    if out.ends_with('.') {
        out.push('0');
    }
    out.replace(".e", ".0e").replace(".E", ".0E")
}

/// `ToLowerInvariant`, one char per char (like .NET's simple case mapping).
pub fn lower_invariant(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\u{130}' {
                return 'i';
            }
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        })
        .collect()
}

/// `ToUpperInvariant`, one char per char.
pub fn upper_invariant(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}

/// The key a `StringComparer.OrdinalIgnoreCase` dictionary compares by.
pub fn ignore_case_key(s: &str) -> String {
    upper_invariant(s)
}

/// `File.ReadAllText`: UTF-8 (BOM stripped) or UTF-16 with a BOM; invalid bytes become U+FFFD.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn read_all_text(path: &Path) -> std::io::Result<String> {
    Ok(decode_text(&std::fs::read(path)?))
}

/// `File.ReadAllLines`: splits on "\r\n", "\n" or "\r"; no trailing empty line.
pub fn read_all_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let text = read_all_text(path)?;
    Ok(split_lines(&text))
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
            other => current.push(other),
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

fn wildcard_match(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| wildcard_match(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && wildcard_match(rest, &name[1..]),
        Some((c, rest)) => name.first().is_some_and(|n| n == c) && wildcard_match(rest, &name[1..]),
    }
}

/// True when a file name matches a `Directory.GetFiles` search pattern (`*`, `?`);
/// case-insensitive on Windows like .NET.
pub fn matches_pattern(file_name: &str, pattern: &str) -> bool {
    if cfg!(windows) {
        let p: Vec<char> = upper_invariant(pattern).chars().collect();
        let n: Vec<char> = upper_invariant(file_name).chars().collect();
        wildcard_match(&p, &n)
    } else {
        let p: Vec<char> = pattern.chars().collect();
        let n: Vec<char> = file_name.chars().collect();
        wildcard_match(&p, &n)
    }
}

/// `Directory.GetFiles(dir, pattern, TopDirectoryOnly)` in the order the file system
/// returns them (the order .NET uses). Errors when the folder is missing, like .NET.
pub fn get_files(dir: &Path, pattern: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false) || entry.path().is_dir();
        if is_dir {
            continue;
        }
        if matches_pattern(&entry.file_name().to_string_lossy(), pattern) {
            files.push(entry.path());
        }
    }
    Ok(files)
}

/// `Directory.GetFiles(dir, pattern, AllDirectories)`: breadth-first, like .NET's enumerator.
pub fn get_files_recursive(dir: &Path, pattern: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = VecDeque::new();
    let mut first = true;
    pending.push_back(dir.to_path_buf());
    while let Some(current) = pending.pop_front() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if first => return Err(error),
            Err(_) => continue,
        };
        first = false;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false) || path.is_dir();
            if is_dir {
                pending.push_back(path);
            } else if matches_pattern(&entry.file_name().to_string_lossy(), pattern) {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// `Environment.ProcessorCount`.
pub fn processor_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// `Path.GetExtension` (with the dot, "" when none).
pub fn extension_with_dot(path: &Path) -> String {
    path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
}

/// The message .NET prints for a null dereference.
pub const NULL_REFERENCE: &str = "Object reference not set to an instance of an object.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_parse_like_dotnet() {
        assert_eq!(try_parse_int(Some(" 12 ")), Some(12));
        assert_eq!(try_parse_int(Some("+7")), Some(7));
        assert_eq!(try_parse_int(Some("-2147483648")), Some(i32::MIN));
        assert_eq!(try_parse_int(Some("2147483648")), None);
        assert_eq!(try_parse_int(Some("1.0")), None);
        assert_eq!(try_parse_int(Some("")), None);
        assert_eq!(try_parse_int(None), None);
    }

    #[test]
    fn float_parse_like_dotnet() {
        assert_eq!(try_parse_f32(Some("0.5")), Some(0.5));
        assert_eq!(try_parse_f32(Some(".5")), Some(0.5));
        assert_eq!(try_parse_f32(Some("5.")), Some(5.0));
        assert_eq!(try_parse_f32(Some("1e3")), Some(1000.0));
        assert_eq!(try_parse_f32(Some(" -2.25 ")), Some(-2.25));
        assert!(try_parse_f32(Some("NaN")).unwrap().is_nan());
        assert_eq!(try_parse_f32(Some("inf")), None);
        assert_eq!(try_parse_f32(Some("1,5")), None);
        assert_eq!(try_parse_f64(Some("-0.5")), Some(-0.5));
    }

    #[test]
    fn general_format_like_dotnet() {
        assert_eq!(format_f32(1.0), "1");
        assert_eq!(format_f32(0.5), "0.5");
        assert_eq!(format_f32(-60.0), "-60");
        assert_eq!(format_f32(0.1), "0.1");
        assert_eq!(format_f32(0.0001), "0.0001");
        assert_eq!(format_f32(0.00001), "1E-05");
        assert_eq!(format_f32(1e9), "1E+09");
        assert_eq!(format_f32(123456790.0), "123456790");
        assert_eq!(format_f32(-0.0), "-0");
        assert_eq!(format_f32(3.4028235e38), "3.4028235E+38");
        assert_eq!(format_f64(0.1), "0.1");
        assert_eq!(format_f64(1e16), "10000000000000000");
        assert_eq!(format_f64(1e17), "1E+17");
        assert_eq!(format_f64(1.5), "1.5");
    }

    #[test]
    fn patterns() {
        assert!(matches_pattern("chair_index.bin", "*_index.bin"));
        assert!(!matches_pattern("chair_index.bin.bak", "*_index.bin"));
        assert!(matches_pattern("a_b.bin", "*_*.bin"));
        assert!(!matches_pattern("ab.bin", "*_*.bin"));
    }

    #[test]
    fn lines() {
        assert_eq!(split_lines("a\r\nb\nc\rd\n"), vec!["a", "b", "c", "d"]);
    }
}
