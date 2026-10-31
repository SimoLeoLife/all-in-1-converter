//! A tiny JSON tree written exactly like `System.Text.Json` with `WriteIndented = true`:
//! two-space indent, `Environment.NewLine` line breaks, the default (HTML-safe) escaping,
//! raw array items written verbatim (`Utf8JsonWriter.WriteRawValue`), and the two float
//! styles the C# converters use (default `float` writing and `FloatToFixedDecimalConverter`).

use anyhow::{bail, Result};

use crate::net;

/// One JSON value, already in output order.
#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    /// A number already formatted the way .NET writes it.
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
    /// `WriteRawValue` inside an array: no newline/indent before it.
    RawItem(String),
    /// A value whose writing throws in C# (e.g. a raw "NaN" that fails validation).
    Fail(String),
}

impl J {
    pub fn int(v: i64) -> J {
        J::Num(v.to_string())
    }

    pub fn str(v: impl Into<String>) -> J {
        J::Str(v.into())
    }

    /// `double` / `double?` with the default writer.
    pub fn f64(v: f64) -> J {
        if !v.is_finite() {
            // AllowNamedFloatingPointLiterals writes these as strings.
            return J::Str(net::format_f64(v));
        }
        J::Num(net::format_f64(v))
    }

    /// `float` with the default writer (`AllowNamedFloatingPointLiterals`).
    pub fn f32_default(v: f32) -> J {
        if !v.is_finite() {
            return J::Str(net::format_f32(v));
        }
        J::Num(net::format_f32(v))
    }

    /// `float` through `FloatToFixedDecimalConverter`.
    pub fn f32_fixed(v: f32) -> J {
        let text = float_to_fixed_decimal(v);
        if !v.is_finite() {
            let first = text.chars().next().unwrap_or('?');
            return J::Fail(format!("'{first}' is an invalid start of a value. LineNumber: 0 | BytePositionInLine: 0."));
        }
        J::Num(text)
    }

    /// Floats in the style the serializer options pick.
    pub fn f32_with(v: f32, style: FloatStyle) -> J {
        match style {
            FloatStyle::Fixed => J::f32_fixed(v),
            FloatStyle::Default => J::f32_default(v),
        }
    }

    pub fn to_string_indented(&self) -> Result<String> {
        let mut out = String::new();
        write_value(self, &mut out, 0)?;
        Ok(out)
    }
}

/// Which converter a `float` goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatStyle {
    /// `FloatToFixedDecimalConverter` registered in the options (furniture).
    Fixed,
    /// Plain System.Text.Json float writing (pets).
    Default,
}

/// `FloatToFixedDecimalConverter.Write`.
pub fn float_to_fixed_decimal(value: f32) -> String {
    if value % 1.0 == 0.0 {
        // (int)value: saturating on .NET 9+, like Rust's `as`.
        (value as i32).to_string()
    } else if value.abs() < 0.0001f32 {
        format!("{value:.6}")
    } else {
        net::format_f32(value)
    }
}

/// Builder for an object, with the `WhenWritingNull` omission rule as `opt`.
#[derive(Debug, Default)]
pub struct Obj(Vec<(String, J)>);

impl Obj {
    pub fn new() -> Self {
        Obj(Vec::new())
    }

    pub fn put(mut self, key: impl Into<String>, value: J) -> Self {
        self.0.push((key.into(), value));
        self
    }

    pub fn opt(self, key: impl Into<String>, value: Option<J>) -> Self {
        match value {
            Some(v) => self.put(key, v),
            None => self,
        }
    }

    pub fn build(self) -> J {
        J::Obj(self.0)
    }
}

impl From<Obj> for J {
    fn from(o: Obj) -> J {
        o.build()
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push_str(habbo_core::paths::NEWLINE);
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(value: &J, out: &mut String, depth: usize) -> Result<()> {
    match value {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => out.push_str(n),
        J::Str(s) => write_string(s, out),
        J::RawItem(raw) => out.push_str(raw),
        J::Fail(message) => bail!("{message}"),
        J::Obj(props) => {
            out.push('{');
            if props.is_empty() {
                out.push('}');
                return Ok(());
            }
            for (index, (key, item)) in props.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, out, depth + 1)?;
            }
            newline(out, depth);
            out.push('}');
        }
        J::Arr(items) => {
            out.push('[');
            if items.is_empty() {
                out.push(']');
                return Ok(());
            }
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if !matches!(item, J::RawItem(_)) {
                    newline(out, depth + 1);
                }
                write_value(item, out, depth + 1)?;
            }
            newline(out, depth);
            out.push(']');
        }
    }
    Ok(())
}

/// The default `JavaScriptEncoder` escaping System.Text.Json uses.
pub fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' | '&' | '\'' | '+' | '<' | '>' | '`' => push_unicode_escape(c, out),
            ' '..='~' => out.push(c),
            _ => push_unicode_escape(c, out),
        }
    }
    out.push('"');
}

fn push_unicode_escape(c: char, out: &mut String) {
    let mut units = [0u16; 2];
    for unit in c.encode_utf16(&mut units) {
        out.push_str(&format!("\\u{:04X}", unit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl(s: &str) -> String {
        s.replace('\n', habbo_core::paths::NEWLINE)
    }

    #[test]
    fn indented_like_system_text_json() {
        let j = Obj::new()
            .put("a", J::int(1))
            .put("b", J::Arr(vec![]))
            .put("c", Obj::new().build())
            .put("d", J::Arr(vec![J::int(1), J::Bool(true)]))
            .build();
        assert_eq!(
            j.to_string_indented().unwrap(),
            nl("{\n  \"a\": 1,\n  \"b\": [],\n  \"c\": {},\n  \"d\": [\n    1,\n    true\n  ]\n}")
        );
    }

    #[test]
    fn raw_items_are_not_indented() {
        let j = Obj::new().put("rgb", J::Arr(vec![J::RawItem("[1,2,3]".into()), J::RawItem("[4,5,6]".into())])).build();
        assert_eq!(j.to_string_indented().unwrap(), nl("{\n  \"rgb\": [[1,2,3],[4,5,6]\n  ]\n}"));
    }

    #[test]
    fn escaping_is_html_safe() {
        let mut out = String::new();
        write_string("a\"b'c+<>&\\é😀\u{1}", &mut out);
        assert_eq!(out, "\"a\\u0022b\\u0027c\\u002B\\u003C\\u003E\\u0026\\\\\\u00E9\\uD83D\\uDE00\\u0001\"");
    }

    #[test]
    fn fixed_decimal_converter() {
        assert_eq!(float_to_fixed_decimal(1.0), "1");
        assert_eq!(float_to_fixed_decimal(-0.0), "0");
        assert_eq!(float_to_fixed_decimal(0.000001), "0.000001");
        assert_eq!(float_to_fixed_decimal(0.5), "0.5");
        assert_eq!(float_to_fixed_decimal(1.25), "1.25");
        assert_eq!(float_to_fixed_decimal(0.1), "0.1");
        assert_eq!(float_to_fixed_decimal(1e10), "2147483647");
        assert!(matches!(J::f32_fixed(f32::NAN), J::Fail(_)));
        assert_eq!(J::f32_default(f32::NAN), J::Str("NaN".into()));
    }
}
