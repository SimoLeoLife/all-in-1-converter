//! LINQ to XML stand-ins over `roxmltree`: `XElement.Parse`, `Element`, `Elements`,
//! `Descendants` and `Attribute(..)?.Value` (local names, no namespace, like the C# calls).

use anyhow::Result;
pub use roxmltree::{Document, Node};

/// `XElement.Parse(text)`.
pub fn parse(text: &str) -> Result<Document<'_>> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    Ok(Document::parse_with_options(text, options)?)
}

fn is_named(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace().is_none()
}

/// `element.Elements(name)`.
pub fn elements<'a, 'input>(node: Node<'a, 'input>, name: &'a str) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children().filter(move |child| is_named(child, name))
}

/// `element.Element(name)`.
pub fn element<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| is_named(child, name))
}

/// `element.Descendants(name)`.
pub fn descendants<'a, 'input>(node: Node<'a, 'input>, name: &'a str) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.descendants().skip(1).filter(move |child| is_named(child, name))
}

/// `element.Attribute(name)?.Value`.
pub fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes().find(|a| a.name() == name && a.namespace().is_none()).map(|a| a.value())
}
