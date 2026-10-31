//! `JsonSerializer.Serialize(..., new JsonSerializerOptions { WriteIndented = true })`:
//! System.Text.Json's indented layout (two spaces, `"key": value`, `[]` / `{}` when empty,
//! `Environment.NewLine` line breaks) with either the default encoder or
//! `JavaScriptEncoder.UnsafeRelaxedJsonEscaping`.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escaping {
    /// `JavaScriptEncoder.Default`: only printable ASCII stays; HTML-sensitive characters and
    /// everything non-ASCII become `\uXXXX`.
    Default,
    /// `JavaScriptEncoder.UnsafeRelaxedJsonEscaping`: only `"`, `\` and control characters escaped.
    Relaxed,
}

/// The indented JSON text, with the platform line break (`\r\n` on Windows).
pub fn to_string_indented(value: &Value, escaping: Escaping) -> String {
    to_string_with_newline(value, escaping, habbo_core::paths::NEWLINE)
}

pub fn to_string_with_newline(value: &Value, escaping: Escaping, newline: &str) -> String {
    let mut out = String::new();
    write_value(&mut out, value, escaping, newline, 0);
    out
}

fn indent(out: &mut String, newline: &str, depth: usize) {
    out.push_str(newline);
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, value: &Value, escaping: Escaping, newline: &str, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                out.push_str(&n.to_string());
            } else {
                // NOTE(port): Rust's shortest round-trip float text; .NET writes exponents as "E+21".
                out.push_str(&format!("{}", n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(s) => write_string(out, s, escaping),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                indent(out, newline, depth + 1);
                write_value(out, item, escaping, newline, depth + 1);
            }
            indent(out, newline, depth);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                indent(out, newline, depth + 1);
                write_string(out, key, escaping);
                out.push_str(": ");
                write_value(out, item, escaping, newline, depth + 1);
            }
            indent(out, newline, depth);
            out.push('}');
        }
    }
}

fn push_unicode(out: &mut String, unit: u16) {
    out.push_str(&format!("\\u{unit:04X}"));
}

pub fn write_string(out: &mut String, text: &str, escaping: Escaping) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' if escaping == Escaping::Relaxed => out.push_str("\\\""),
            _ => {
                let escape = match escaping {
                    Escaping::Default => {
                        !(' '..='~').contains(&ch) || matches!(ch, '"' | '&' | '\'' | '+' | '<' | '>' | '`')
                    }
                    Escaping::Relaxed => {
                        // NOTE(port): .NET also escapes unassigned code points; not tracked here.
                        (ch as u32) < 0x20 || (0x7F..=0x9F).contains(&(ch as u32)) || matches!(ch, '\u{2028}' | '\u{2029}')
                    }
                };
                if escape {
                    let mut units = [0u16; 2];
                    for unit in ch.encode_utf16(&mut units) {
                        push_unicode(out, *unit);
                    }
                } else {
                    out.push(ch);
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn indented_layout_matches_system_text_json() {
        let value = json!({"a": 1, "b": [true, null], "c": [], "d": {}, "e": {"f": -0.9}});
        let text = to_string_with_newline(&value, Escaping::Relaxed, "\n");
        assert_eq!(
            text,
            "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null\n  ],\n  \"c\": [],\n  \"d\": {},\n  \"e\": {\n    \"f\": -0.9\n  }\n}"
        );
        assert_eq!(to_string_with_newline(&json!(-17.0), Escaping::Default, "\n"), "-17");
    }

    #[test]
    fn escaping_modes() {
        let value = json!("a\"<é>\n😀");
        assert_eq!(to_string_with_newline(&value, Escaping::Relaxed, "\n"), "\"a\\\"<é>\\n😀\"");
        assert_eq!(
            to_string_with_newline(&value, Escaping::Default, "\n"),
            "\"a\\u0022\\u003C\\u00E9\\u003E\\n\\uD83D\\uDE00\""
        );
    }
}
