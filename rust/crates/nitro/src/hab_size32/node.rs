//! A small `System.Text.Json.Nodes` stand-in for the size-32 merge: numbers keep their source
//! text, object keys keep their order, and the writer escapes like `JsonSerializer` with the
//! default `JavaScriptEncoder` (compact, non-ASCII and `"&'+<>\`` as `\uXXXX`).

use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    /// The number as written in the source (or by us).
    Number(String),
    String(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    pub fn int(value: i64) -> Node {
        Node::Number(value.to_string())
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        match self {
            Node::Object(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `obj[key]` that is present and not JSON null (a C# `JsonNode` null).
    pub fn prop(&self, key: &str) -> Option<&Node> {
        self.get(key).filter(|n| !matches!(n, Node::Null))
    }

    /// `JsonObject[key] = value`: replaces in place, or appends.
    pub fn set(&mut self, key: &str, value: Node) -> Result<()> {
        let Node::Object(entries) = self else { bail!("The node must be of type 'JsonObject'.") };
        match entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_string(), value)),
        }
        Ok(())
    }

    pub fn as_object(&self) -> Result<&Vec<(String, Node)>> {
        match self {
            Node::Object(entries) => Ok(entries),
            _ => bail!("The node must be of type 'JsonObject'."),
        }
    }

    pub fn as_array_mut(&mut self) -> Result<&mut Vec<Node>> {
        match self {
            Node::Array(items) => Ok(items),
            _ => bail!("The node must be of type 'JsonArray'."),
        }
    }

    pub fn as_array(&self) -> Result<&Vec<Node>> {
        match self {
            Node::Array(items) => Ok(items),
            _ => bail!("The node must be of type 'JsonArray'."),
        }
    }

    /// `GetValue<int>()`.
    pub fn get_int(&self) -> Result<i32> {
        match self {
            Node::Number(raw) => raw.parse::<i32>().map_err(|_| anyhow!("The input string '{raw}' was not in a correct format.")),
            _ => bail!("An element of type '{}' cannot be converted to a 'System.Int32'.", self.kind()),
        }
    }

    /// `GetValue<string>()`.
    pub fn get_string(&self) -> Result<String> {
        match self {
            Node::String(s) => Ok(s.clone()),
            _ => bail!("An element of type '{}' cannot be converted to a 'System.String'.", self.kind()),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Node::Null => "Null",
            Node::Bool(true) => "True",
            Node::Bool(false) => "False",
            Node::Number(_) => "Number",
            Node::String(_) => "String",
            Node::Array(_) => "Array",
            Node::Object(_) => "Object",
        }
    }
}

// ---------------------------------------------------------------- parsing

pub fn parse(bytes: &[u8]) -> Result<Node> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut parser = Parser { bytes, pos: 0, depth: 0 };
    parser.skip_ws();
    let node = parser.value()?;
    parser.skip_ws();
    if parser.pos != bytes.len() {
        bail!("'{}' is invalid after a single JSON value. Expected end of data.", bytes[parser.pos] as char);
    }
    Ok(node)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.peek() != Some(byte) {
            bail!("Invalid JSON: expected '{}' at byte {}.", byte as char, self.pos);
        }
        self.pos += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Node> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Node::String(self.string()?)),
            Some(b't') => self.literal("true", Node::Bool(true)),
            Some(b'f') => self.literal("false", Node::Bool(false)),
            Some(b'n') => self.literal("null", Node::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => bail!("Invalid JSON value at byte {}.", self.pos),
        }
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("The maximum configured depth of 64 has been exceeded.");
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Node> {
        self.enter()?;
        self.pos += 1;
        let mut entries: Vec<(String, Node)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Node::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value()?;
            if entries.iter().any(|(k, _)| *k == key) {
                bail!("An item with the same key has already been added. Key: {key}");
            }
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => bail!("Invalid JSON object at byte {}.", self.pos),
            }
        }
        self.depth -= 1;
        Ok(Node::Object(entries))
    }

    fn array(&mut self) -> Result<Node> {
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Node::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => bail!("Invalid JSON array at byte {}.", self.pos),
            }
        }
        self.depth -= 1;
        Ok(Node::Array(items))
    }

    fn literal(&mut self, text: &str, node: Node) -> Result<Node> {
        if self.bytes[self.pos..].starts_with(text.as_bytes()) {
            self.pos += text.len();
            Ok(node)
        } else {
            bail!("Invalid JSON literal at byte {}.", self.pos)
        }
    }

    fn number(&mut self) -> Result<Node> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let s = p.pos;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos > s
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else if !digits(self) {
            bail!("Invalid JSON number at byte {start}.");
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                bail!("Invalid JSON number at byte {start}.");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                bail!("Invalid JSON number at byte {start}.");
            }
        }
        Ok(Node::Number(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned()))
    }

    fn hex4(&mut self) -> Result<u32> {
        let slice = self.bytes.get(self.pos..self.pos + 4).ok_or_else(|| anyhow!("Invalid JSON escape."))?;
        let text = std::str::from_utf8(slice)?;
        let value = u32::from_str_radix(text, 16).map_err(|_| anyhow!("Invalid JSON escape."))?;
        self.pos += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.peek() else { bail!("Unterminated JSON string.") };
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else { bail!("Unterminated JSON string.") };
                    self.pos += 1;
                    let ch = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            if (0xD800..0xDC00).contains(&first) && self.bytes[self.pos..].starts_with(b"\\u") {
                                self.pos += 2;
                                let second = self.hex4()?;
                                let combined = 0x10000 + ((first - 0xD800) << 10) + (second.wrapping_sub(0xDC00) & 0x3FF);
                                char::from_u32(combined).unwrap_or('\u{FFFD}')
                            } else {
                                char::from_u32(first).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => bail!("Invalid JSON escape at byte {}.", self.pos),
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                0x00..=0x1F => bail!("Invalid control character in JSON string at byte {}.", self.pos),
                _ => out.push(byte),
            }
        }
        String::from_utf8(out).map_err(|_| anyhow!("Invalid UTF-8 in JSON string."))
    }
}

// ---------------------------------------------------------------- writing

/// `JsonSerializer.SerializeToUtf8Bytes(node)`.
pub fn to_bytes(node: &Node) -> Vec<u8> {
    let mut out = String::new();
    write(node, &mut out);
    out.into_bytes()
}

fn write(node: &Node, out: &mut String) {
    match node {
        Node::Null => out.push_str("null"),
        Node::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Node::Number(raw) => out.push_str(raw),
        Node::String(s) => write_string(s, out),
        Node::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Node::Object(entries) => {
            out.push('{');
            for (i, (key, value)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write(value, out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\\' => out.push_str("\\\\"),
            '"' | '&' | '\'' | '+' | '<' | '>' | '`' => push_u(ch as u32, out),
            c if (c as u32) < 0x20 || (c as u32) >= 0x7F => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    push_u(*unit as u32, out);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_u(unit: u32, out: &mut String) {
    out.push_str(&format!("\\u{unit:04X}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_numbers_and_escapes_like_stj() {
        let node = parse("{\"a\":1.50,\"b\":[1e3,-0,true,null],\"s\":\"x\\\"<é>\\n/😀\"}".as_bytes()).unwrap();
        assert_eq!(
            String::from_utf8(to_bytes(&node)).unwrap(),
            "{\"a\":1.50,\"b\":[1e3,-0,true,null],\"s\":\"x\\u0022\\u003C\\u00E9\\u003E\\n/\\uD83D\\uDE00\"}"
        );
        assert!(parse(b"{\"a\":1,}").is_err());
        assert!(parse(b"{\"a\":1,\"a\":2}").is_err());
        assert_eq!(node.get("a").unwrap().get_int().ok(), None);
    }
}
