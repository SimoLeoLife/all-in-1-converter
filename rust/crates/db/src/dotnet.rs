//! The bits of .NET text formatting / parsing the generated files depend on, so the Rust
//! output is byte-identical: `double.ToString(InvariantCulture)`, `ToString("0.00")`,
//! `int.Parse` / `int.TryParse`, `StringComparer.OrdinalIgnoreCase` and System.Text.Json's
//! indented writer.

use std::cmp::Ordering;

use anyhow::{bail, Result};
use serde_json::Value;

/// `double.ToString(CultureInfo.InvariantCulture)` (.NET Core 3.0+): the shortest round-trip
/// digits, scientific (`1E-05`, `1E+15`) below 1e-4 or from 1e15 on.
pub fn double_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "∞".into() } else { "-∞".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }

    // "{:e}" gives the shortest round-trip digits: "-1.2345e-7".
    let sci = format!("{value:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..15).contains(&exponent) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    } else if exponent < 0 {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let int_len = exponent as usize + 1;
        if digits.len() <= int_len {
            out.push_str(&digits);
            for _ in digits.len()..int_len {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..int_len]);
            out.push('.');
            out.push_str(&digits[int_len..]);
        }
    }
    out
}

/// `value.ToString("0.00")` (or `{value:0.00}`) with .NET's rounding: half away from zero on
/// the exact decimal value.
pub fn format_fixed(value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return double_to_string(value);
    }
    // Rust prints the exact decimal expansion for a given precision; take a few more digits
    // than needed and round them by hand, away from zero.
    let exact = format!("{:.*}", decimals + 30, value.abs());
    let (int_part, frac_part) = exact.split_once('.').unwrap();
    let mut digits: Vec<u8> = int_part.bytes().chain(frac_part[..decimals].bytes()).map(|b| b - b'0').collect();
    let round_up = frac_part.as_bytes()[decimals] >= b'5';
    if round_up {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, 1);
                break;
            }
            index -= 1;
            if digits[index] == 9 {
                digits[index] = 0;
            } else {
                digits[index] += 1;
                break;
            }
        }
    }
    let int_len = digits.len() - decimals;
    let mut out = String::new();
    let is_zero = digits.iter().all(|d| *d == 0);
    if value < 0.0 && !is_zero {
        out.push('-');
    }
    out.extend(digits[..int_len].iter().map(|d| (d + b'0') as char));
    if decimals > 0 {
        out.push('.');
        out.extend(digits[int_len..].iter().map(|d| (d + b'0') as char));
    }
    out
}

/// The white space `int.Parse` skips around the number.
fn trim_dotnet(text: &str) -> &str {
    text.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{85}') || c.is_whitespace())
}

/// `int.TryParse(text, out n)` (NumberStyles.Integer, invariant).
pub fn try_parse_int(text: &str) -> Option<i32> {
    let trimmed = trim_dotnet(text);
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i32>().ok()
}

/// `int.Parse(Console.ReadLine())`: errors like .NET does.
pub fn parse_int(text: Option<&str>) -> Result<i32> {
    let Some(text) = text else {
        bail!("Value cannot be null. (Parameter 's')");
    };
    match try_parse_int(text) {
        Some(value) => Ok(value),
        None => {
            let trimmed = trim_dotnet(text);
            if !trimmed.is_empty() && trimmed.trim_start_matches(['-', '+']).bytes().all(|b| b.is_ascii_digit()) {
                bail!("Value was either too large or too small for an Int32.");
            }
            bail!("The input string '{text}' was not in a correct format.")
        }
    }
}

/// `double.Parse(text, CultureInfo.InvariantCulture)` for attribute values.
pub fn parse_double(text: &str) -> Result<f64> {
    let trimmed = trim_dotnet(text);
    let lower = trimmed.to_ascii_lowercase();
    // Rust accepts "inf" / "nan" spellings .NET does not.
    if lower.contains("inf") || lower.contains("nan") || trimmed.is_empty() {
        bail!("The input string '{text}' was not in a correct format.");
    }
    trimmed.parse::<f64>().map_err(|_| anyhow::anyhow!("The input string '{text}' was not in a correct format."))
}

/// `StringComparer.OrdinalIgnoreCase.Compare`: ordinal on the upper-cased UTF-16 units.
pub fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    let upper = |s: &str| -> Vec<u16> { s.encode_utf16().map(upper_unit).collect() };
    upper(a).cmp(&upper(b))
}

fn upper_unit(unit: u16) -> u16 {
    match char::from_u32(unit as u32) {
        Some(c) => {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) if (u as u32) <= 0xFFFF => u as u32 as u16,
                _ => unit,
            }
        }
        None => unit,
    }
}

/// The key `StringComparer.OrdinalIgnoreCase` treats as equal.
pub fn ignore_case_key(s: &str) -> String {
    String::from_utf16_lossy(&s.encode_utf16().map(upper_unit).collect::<Vec<_>>())
}

/// `JsonSerializer.Serialize(value, new JsonSerializerOptions { WriteIndented = true })`:
/// two-space indent, `"key": value`, the default (JavaScriptEncoder.Default) escaping and
/// `Environment.NewLine` between lines.
pub fn stj_indented(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

fn newline_indent(out: &mut String, depth: usize) {
    out.push_str(habbo_core::paths::NEWLINE);
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(out, s),
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
                newline_indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            newline_indent(out, depth);
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
                newline_indent(out, depth + 1);
                write_string(out, key);
                out.push_str(": ");
                write_value(out, item, depth + 1);
            }
            newline_indent(out, depth);
            out.push('}');
        }
    }
}

/// A JSON string the way System.Text.Json's default encoder writes it: only printable ASCII
/// stays as is, minus `" & ' + < > \` and the backtick.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for unit in s.encode_utf16() {
        match unit {
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0A => out.push_str("\\n"),
            0x0C => out.push_str("\\f"),
            0x0D => out.push_str("\\r"),
            0x5C => out.push_str("\\\\"),
            0x22 | 0x26 | 0x27 | 0x2B | 0x3C | 0x3E | 0x60 => out.push_str(&format!("\\u{unit:04X}")),
            0x20..=0x7E => out.push(unit as u8 as char),
            _ => out.push_str(&format!("\\u{unit:04X}")),
        }
    }
    out.push('"');
}

/// `JToken.ToString()` of a JSON value as the furnidata code used it on scalar values.
pub fn jtoken_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if !n.is_i64() && !n.is_u64() => double_to_string(f),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// `token.ToObject<int>()` (Newtonsoft): numbers, numeric strings and booleans convert.
pub fn jtoken_to_i32(value: &Value) -> Result<i32> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return i32::try_from(i).map_err(|_| anyhow::anyhow!("Value was either too large or too small for an Int32."));
            }
            let f = n.as_f64().unwrap_or(f64::NAN);
            let rounded = f.round_ties_even();
            if rounded.is_finite() && rounded >= i32::MIN as f64 && rounded <= i32::MAX as f64 {
                Ok(rounded as i32)
            } else {
                bail!("Value was either too large or too small for an Int32.")
            }
        }
        Value::String(s) => parse_int(Some(s)),
        Value::Bool(b) => Ok(*b as i32),
        Value::Null => bail!("Null object cannot be converted to a value type."),
        _ => bail!("Can not convert {} to Int32.", if value.is_array() { "Array" } else { "Object" }),
    }
}

/// `token.ToObject<bool>()` (Newtonsoft).
pub fn jtoken_to_bool(value: &Value) -> Result<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_f64().is_some_and(|f| f != 0.0)),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => bail!("String '{s}' was not recognized as a valid Boolean."),
        },
        Value::Null => bail!("Null object cannot be converted to a value type."),
        _ => bail!("Can not convert {} to Boolean.", if value.is_array() { "Array" } else { "Object" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_like_dotnet() {
        assert_eq!(double_to_string(1.0), "1");
        assert_eq!(double_to_string(0.0), "0");
        assert_eq!(double_to_string(0.5), "0.5");
        assert_eq!(double_to_string(1.2), "1.2");
        assert_eq!(double_to_string(123.456), "123.456");
        assert_eq!(double_to_string(0.0001), "0.0001");
        assert_eq!(double_to_string(0.00001), "1E-05");
        assert_eq!(double_to_string(1.5e-7), "1.5E-07");
        assert_eq!(double_to_string(1e14), "100000000000000");
        assert_eq!(double_to_string(1e15), "1E+15");
        assert_eq!(double_to_string(-2.25), "-2.25");
    }

    #[test]
    fn fixed_rounds_away_from_zero() {
        assert_eq!(format_fixed(0.0, 2), "0.00");
        assert_eq!(format_fixed(0.125, 2), "0.13");
        assert_eq!(format_fixed(1.005, 2), "1.00"); // 1.005 is 1.00499999... in binary
        assert_eq!(format_fixed(99.999, 2), "100.00");
        assert_eq!(format_fixed(33.333333, 2), "33.33");
        assert_eq!(format_fixed(-1.5, 2), "-1.50");
    }

    #[test]
    fn ints_like_dotnet() {
        assert_eq!(try_parse_int(" 12 "), Some(12));
        assert_eq!(try_parse_int("+3"), Some(3));
        assert_eq!(try_parse_int("-1"), Some(-1));
        assert_eq!(try_parse_int("1.0"), None);
        assert_eq!(try_parse_int(""), None);
        assert!(parse_int(Some("abc")).unwrap_err().to_string().contains("'abc'"));
        assert!(parse_int(None).is_err());
    }

    #[test]
    fn ignore_case_order() {
        assert_eq!(cmp_ignore_case("abc", "ABD"), Ordering::Less);
        assert_eq!(cmp_ignore_case("Abc", "aBC"), Ordering::Equal);
        // '_' (0x5F) sorts after upper-case letters, so after "Z".
        assert_eq!(cmp_ignore_case("a_b", "aZ"), Ordering::Greater);
    }

    #[test]
    fn stj_writer() {
        let value: Value = serde_json::from_str(r#"{"a":[1,{"b":"x<y\"é"}],"e":{},"f":[],"g":null,"h":0.5}"#).unwrap();
        let nl = habbo_core::paths::NEWLINE;
        let expected = [
            "{",
            "  \"a\": [",
            "    1,",
            "    {",
            "      \"b\": \"x\\u003Cy\\u0022\\u00E9\"",
            "    }",
            "  ],",
            "  \"e\": {},",
            "  \"f\": [],",
            "  \"g\": null,",
            "  \"h\": 0.5",
            "}",
        ]
        .join(nl);
        assert_eq!(stj_indented(&value), expected);
    }
}
