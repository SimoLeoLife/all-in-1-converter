//! .NET text formatting the outputs depend on, shared so every crate writes the same bytes:
//! `double.ToString()` / `float.ToString()` (also what System.Text.Json writes for numbers) and
//! System.Text.Json's string escaping.

/// The two System.Text.Json encoders the C# code used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escaping {
    /// `JavaScriptEncoder.Default`: only printable ASCII stays, minus `" & ' + < >` and backtick.
    Default,
    /// `JavaScriptEncoder.UnsafeRelaxedJsonEscaping`: `"` as `\"`; control, space-separator and
    /// private-use characters escaped, everything else as is.
    Relaxed,
}

/// Appends `text` as a quoted JSON string, escaped like System.Text.Json.
pub fn write_json_string(out: &mut String, text: &str, escaping: Escaping) {
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
                    Escaping::Default => !(' '..='~').contains(&ch) || matches!(ch, '"' | '&' | '\'' | '+' | '<' | '>' | '`'),
                    Escaping::Relaxed => {
                        // .NET never lets through Cc, Zs (except U+0020), Zl, Zp or private use.
                        // NOTE(port): it also escapes unassigned code points; not tracked here.
                        let c = ch as u32;
                        c < 0x20
                            || (0x7F..=0x9F).contains(&c)
                            || matches!(c, 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000)
                            || (0xE000..=0xF8FF).contains(&c)
                            || c >= 0xF0000
                    }
                };
                if escape {
                    let mut units = [0u16; 2];
                    for unit in ch.encode_utf16(&mut units) {
                        out.push_str(&format!("\\u{:04X}", unit));
                    }
                } else {
                    out.push(ch);
                }
            }
        }
    }
    out.push('"');
}

/// Shortest round-trip digits of a finite, non-zero value: (negative, digits, scale) where
/// value = 0.d1d2d3... * 10^scale (.NET's `NumberBuffer.Scale`).
fn shortest_digits(scientific: &str) -> (bool, String, i32) {
    // Rust's `{:e}` gives the shortest round-trip digits, e.g. "-1.2345e-5".
    let (negative, rest) = match scientific.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, scientific),
    };
    let (mantissa, exponent) = rest.split_once('e').unwrap_or((rest, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    (negative, digits, exponent + 1)
}

/// .NET `FormatGeneral` with `nMaxDigits = max(digitCount, maxRoundTripDigits)`.
fn format_general(negative: bool, digits: &str, scale: i32, max_round_trip_digits: usize) -> String {
    let n_max = digits.len().max(max_round_trip_digits) as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let mut dig_pos = scale;
    let mut scientific = false;
    if dig_pos > n_max || dig_pos < -3 {
        dig_pos = 1;
        scientific = true;
    }
    let digit_bytes = digits.as_bytes();
    let mut index = 0usize;
    if dig_pos > 0 {
        while dig_pos > 0 {
            out.push(if index < digit_bytes.len() { digit_bytes[index] as char } else { '0' });
            if index < digit_bytes.len() {
                index += 1;
            }
            dig_pos -= 1;
        }
    } else {
        out.push('0');
    }
    if index < digit_bytes.len() || dig_pos < 0 {
        out.push('.');
        while dig_pos < 0 {
            out.push('0');
            dig_pos += 1;
        }
        while index < digit_bytes.len() {
            out.push(digit_bytes[index] as char);
            index += 1;
        }
    }
    if scientific {
        let exponent = scale - 1;
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.unsigned_abs()));
    }
    out
}

fn special(value: f64) -> Option<String> {
    if value.is_nan() {
        return Some("NaN".into());
    }
    if value.is_infinite() {
        return Some(if value > 0.0 { "Infinity" } else { "-Infinity" }.into());
    }
    if value == 0.0 {
        return Some(if value.is_sign_negative() { "-0" } else { "0" }.into());
    }
    None
}

/// `float.ToString(InvariantCulture)` (.NET Core 3.0+), also System.Text.Json's float output.
pub fn format_f32(value: f32) -> String {
    if let Some(text) = special(value as f64) {
        return text;
    }
    let (negative, digits, scale) = shortest_digits(&format!("{value:e}"));
    format_general(negative, &digits, scale, 9)
}

/// `double.ToString(InvariantCulture)` (.NET Core 3.0+), also System.Text.Json's double output.
pub fn format_f64(value: f64) -> String {
    if let Some(text) = special(value) {
        return text;
    }
    let (negative, digits, scale) = shortest_digits(&format!("{value:e}"));
    format_general(negative, &digits, scale, 17)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Printed by .NET 11 (`d.ToString(InvariantCulture)` and `JsonSerializer.Serialize`).
    #[test]
    fn numbers_match_dotnet() {
        for (value, expected) in [
            (1e15, "1000000000000000"),
            (1e16, "10000000000000000"),
            (123456789012345.0, "123456789012345"),
            (1e-5, "1E-05"),
            (0.0001, "0.0001"),
            (1.5e300, "1.5E+300"),
            (0.1, "0.1"),
            (1e20, "1E+20"),
            (12345678901234567890.0, "1.2345678901234567E+19"),
            (-0.9, "-0.9"),
            (-17.0, "-17"),
        ] {
            assert_eq!(format_f64(value), expected, "{value}");
        }
        for (value, expected) in [
            (1e7f32, "10000000"),
            (12345678f32, "12345678"),
            (1e8f32, "100000000"),
            (123456789f32, "123456790"),
            (1e-5f32, "1E-05"),
            (0.1f32, "0.1"),
            (3.4e38f32, "3.4E+38"),
        ] {
            assert_eq!(format_f32(value), expected, "{value}");
        }
    }

    #[test]
    fn escaping_modes() {
        let mut default = String::new();
        write_json_string(&mut default, "a\"b<c>&'+`\\\n\u{1}é😀 ~", Escaping::Default);
        assert_eq!(default, "\"a\\u0022b\\u003Cc\\u003E\\u0026\\u0027\\u002B\\u0060\\\\\\n\\u0001\\u00E9\\uD83D\\uDE00 ~\"");
        let mut relaxed = String::new();
        write_json_string(&mut relaxed, "a\"<é>\n😀\u{a0}\u{200b}", Escaping::Relaxed);
        assert_eq!(relaxed, "\"a\\\"<é>\\n😀\\u00A0\u{200b}\"");
    }
}
