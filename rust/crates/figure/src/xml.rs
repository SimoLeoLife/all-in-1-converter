//! LINQ to XML helpers over `roxmltree`: `XElement.Parse`, `Element(name)`,
//! `Elements(name)`, `Elements()` and `Value`. Names without a namespace match like
//! `XName` strings do (local name, no namespace).

use roxmltree::{Document, Node, ParsingOptions};

pub fn parse(text: &str) -> Result<Document<'_>, roxmltree::Error> {
    Document::parse_with_options(text, ParsingOptions { allow_dtd: true, ..ParsingOptions::default() })
}

pub fn is_named(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace().is_none()
}

/// `XElement.Element(name)`: the first child element with this name.
pub fn element<'a, 'i>(node: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is_named(c, name))
}

/// `XElement.Elements(name)`.
pub fn elements<'a, 'i: 'a>(node: &Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |c| is_named(c, name))
}

/// `XElement.Elements()`.
pub fn child_elements<'a, 'i: 'a>(node: &Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(|c| c.is_element())
}

/// `XElement.Value`: the concatenated text of all descendants.
pub fn value(node: &Node) -> String {
    node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect()
}

/// `Attribute(name)?.Value`.
pub fn attr(node: &Node, name: &str) -> Option<String> {
    node.attribute(name).map(str::to_string)
}
