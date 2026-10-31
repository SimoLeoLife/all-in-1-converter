//! A tiny writer that reproduces `System.Text.Json.JsonSerializer.Serialize` with
//! `WriteIndented = true` and the default encoder: two-space indent, `"key": value`,
//! `Environment.NewLine` between lines, `{}` / `[]` for empty containers, and the default
//! `JavaScriptEncoder` escaping (`"` -> `"`, HTML-sensitive and non-ASCII characters as
//! `\uXXXX`). The C# compilers serialise anonymous objects with System.Text.Json (not
//! Newtonsoft), so the Rust output follows its rules byte for byte.

use habbo_core::paths::NEWLINE;

/// A JSON node with properties in serialisation order. Null properties are simply not added
/// (`DefaultIgnoreCondition = WhenWritingNull`).
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    String(String),
    /// A number or literal already formatted the way System.Text.Json writes it.
    Raw(String),
}

impl Node {
    pub fn int(value: i64) -> Self {
        Node::Raw(value.to_string())
    }

    pub fn bool(value: bool) -> Self {
        Node::Raw(if value { "true" } else { "false" }.to_string())
    }

    pub fn str(value: impl Into<String>) -> Self {
        Node::String(value.into())
    }
}

/// Builds an object, skipping `None` properties.
#[derive(Default)]
pub struct ObjectBuilder(Vec<(String, Node)>);

impl ObjectBuilder {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn put(mut self, key: &str, value: Node) -> Self {
        self.0.push((key.to_string(), value));
        self
    }

    pub fn opt(self, key: &str, value: Option<Node>) -> Self {
        match value {
            Some(value) => self.put(key, value),
            None => self,
        }
    }

    pub fn opt_str(self, key: &str, value: &Option<String>) -> Self {
        self.opt(key, value.as_ref().map(|v| Node::str(v.clone())))
    }

    pub fn opt_int(self, key: &str, value: Option<i32>) -> Self {
        self.opt(key, value.map(|v| Node::int(v as i64)))
    }

    pub fn build(self) -> Node {
        Node::Object(self.0)
    }
}

/// `JsonSerializer.Serialize(value, new JsonSerializerOptions { WriteIndented = true })`.
pub fn to_string_indented(node: &Node) -> String {
    let mut out = String::new();
    write_node(node, 0, &mut out);
    out
}

fn write_node(node: &Node, indent: usize, out: &mut String) {
    match node {
        Node::Raw(raw) => out.push_str(raw),
        Node::String(s) => write_string(s, out),
        Node::Object(props) => {
            if props.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, value)) in props.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(NEWLINE);
                push_indent(indent + 2, out);
                write_string(key, out);
                out.push_str(": ");
                write_node(value, indent + 2, out);
            }
            out.push_str(NEWLINE);
            push_indent(indent, out);
            out.push('}');
        }
        Node::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, value) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(NEWLINE);
                push_indent(indent + 2, out);
                write_node(value, indent + 2, out);
            }
            out.push_str(NEWLINE);
            push_indent(indent, out);
            out.push(']');
        }
    }
}

fn push_indent(n: usize, out: &mut String) {
    out.extend(std::iter::repeat_n(' ', n));
}

/// The default System.Text.Json string escaping.
pub fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (' '..='~').contains(&c) && !matches!(c, '"' | '&' | '\'' | '+' | '<' | '>' | '`') => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04X}", unit));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indented_layout_matches_system_text_json() {
        let node = ObjectBuilder::new()
            .put("a", Node::int(1))
            .put("empty", Node::Object(vec![]))
            .put("list", Node::Array(vec![ObjectBuilder::new().put("x", Node::bool(true)).build()]))
            .put("none", Node::Array(vec![]))
            .build();
        let expected = "{\n  \"a\": 1,\n  \"empty\": {},\n  \"list\": [\n    {\n      \"x\": true\n    }\n  ],\n  \"none\": []\n}"
            .replace('\n', NEWLINE);
        assert_eq!(to_string_indented(&node), expected);
    }

    #[test]
    fn escaping_matches_default_encoder() {
        let mut out = String::new();
        write_string("a\"b<c>&'+`\\\n\u{1}é😀 ~", &mut out);
        assert_eq!(out, "\"a\\u0022b\\u003Cc\\u003E\\u0026\\u0027\\u002B\\u0060\\\\\\n\\u0001\\u00E9\\uD83D\\uDE00 ~\"");
    }
}
