//! A tiny `XDocument` stand-in on top of quick-xml: elements, attributes and text, with the
//! LINQ-to-XML lookups the downloaders used (`Element`, `Elements`, `Descendants`, `Value`).

use std::path::Path;

use anyhow::{bail, Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    /// `Attribute(name)?.Value`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// `Element(name)`: the first child element with this name.
    pub fn element(&self, name: &str) -> Option<&Element> {
        self.elements(name).next()
    }

    /// `Elements(name)`.
    pub fn elements<'a, 'n>(&'a self, name: &'n str) -> impl Iterator<Item = &'a Element> + use<'a, 'n> {
        self.children.iter().filter_map(move |node| match node {
            Node::Element(element) if element.name == name => Some(element),
            _ => None,
        })
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    /// `Descendants(name)` (document order, not including this element).
    pub fn descendants<'a>(&'a self, name: &str) -> Vec<&'a Element> {
        let mut found = Vec::new();
        self.collect_descendants(name, &mut found);
        found
    }

    fn collect_descendants<'a>(&'a self, name: &str, found: &mut Vec<&'a Element>) {
        for child in self.child_elements() {
            if child.name == name {
                found.push(child);
            }
            child.collect_descendants(name, found);
        }
    }

    /// `XElement.Value`: every descendant text node, concatenated.
    pub fn value(&self) -> String {
        let mut text = String::new();
        self.collect_text(&mut text);
        text
    }

    fn collect_text(&self, text: &mut String) {
        for child in &self.children {
            match child {
                Node::Text(t) => text.push_str(t),
                Node::Element(e) => e.collect_text(text),
            }
        }
    }
}

/// `XDocument`: one root element.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub root: Element,
}

impl Document {
    /// `XDocument.Load(path)`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).with_context(|| format!("Could not find file '{}'.", path.display()))?;
        Self::parse(&crate::common::decode_text(&bytes))
    }

    /// `XDocument.Parse(text)`.
    pub fn parse(text: &str) -> Result<Self> {
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(false);
        reader.config_mut().check_end_names = true;

        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;

        loop {
            let event = reader.read_event().map_err(|e| anyhow::anyhow!("{e} (position {})", reader.buffer_position()))?;
            match event {
                Event::Start(start) => stack.push(start_element(&start)?),
                Event::Empty(start) => {
                    let element = start_element(&start)?;
                    attach(&mut stack, &mut root, element)?;
                }
                Event::End(_) => {
                    let element = stack.pop().context("Unexpected end tag.")?;
                    attach(&mut stack, &mut root, element)?;
                }
                Event::Text(text) => {
                    let value = normalize_newlines(&text.unescape()?);
                    push_text(&mut stack, value)?;
                }
                Event::CData(data) => {
                    let value = normalize_newlines(&String::from_utf8_lossy(&data.into_inner()));
                    push_text(&mut stack, value)?;
                }
                Event::Eof => break,
                _ => {}
            }
        }

        if !stack.is_empty() {
            bail!("Unexpected end of file has occurred. The following elements are not closed: {}.", stack
                .iter()
                .rev()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join(", "));
        }
        match root {
            Some(root) => Ok(Self { root }),
            None => bail!("Root element is missing."),
        }
    }

    /// `doc.Element(name)`: the root when it has this name.
    pub fn element(&self, name: &str) -> Option<&Element> {
        (self.root.name == name).then_some(&self.root)
    }

    /// `doc.Descendants(name)`: includes the root.
    pub fn descendants<'a>(&'a self, name: &str) -> Vec<&'a Element> {
        let mut found = Vec::new();
        if self.root.name == name {
            found.push(&self.root);
        }
        found.extend(self.root.descendants(name));
        found
    }
}

fn start_element(start: &BytesStart) -> Result<Element> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        if attributes.iter().any(|(k, _): &(String, String)| *k == key) {
            bail!("'{key}' is a duplicate attribute name.");
        }
        // XML attribute-value normalisation: literal tabs and line breaks become spaces.
        let value = normalize_newlines(&attribute.unescape_value()?).replace(['\t', '\n'], " ");
        attributes.push((key, value));
    }
    Ok(Element { name, attributes, children: Vec::new() })
}

fn attach(stack: &mut [Element], root: &mut Option<Element>, element: Element) -> Result<()> {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(element)),
        None => {
            if root.is_some() {
                bail!("There are multiple root elements.");
            }
            *root = Some(element);
        }
    }
    Ok(())
}

fn push_text(stack: &mut [Element], value: String) -> Result<()> {
    match stack.last_mut() {
        Some(parent) => {
            if let Some(Node::Text(previous)) = parent.children.last_mut() {
                previous.push_str(&value);
            } else if !value.is_empty() {
                parent.children.push(Node::Text(value));
            }
        }
        None => {
            if !value.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
                bail!("Data at the root level is invalid.");
            }
        }
    }
    Ok(())
}

fn normalize_newlines(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elements_attributes_and_text() {
        let doc = Document::parse(
            "<?xml version=\"1.0\"?>\r\n<a x=\"1 &amp; 2\"><b>t&lt;1<![CDATA[<c>]]></b><b/><c><b>deep</b></c></a>",
        )
        .unwrap();
        let a = doc.element("a").unwrap();
        assert_eq!(a.attr("x"), Some("1 & 2"));
        assert_eq!(a.elements("b").count(), 2);
        assert_eq!(a.element("b").unwrap().value(), "t<1<c>");
        assert_eq!(doc.descendants("b").len(), 3);
        assert_eq!(a.value(), "t<1<c>deep");
        assert!(doc.element("z").is_none());
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(Document::parse("<a><b></a>").is_err());
        assert!(Document::parse("<a>").is_err());
        assert!(Document::parse("").is_err());
        assert!(Document::parse("<a/><b/>").is_err());
    }
}
