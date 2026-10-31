//! `Mapper/Index` (`IndexMapper`) and the pets `IndexPetsMapper`.

use std::path::Path;

use habbo_core::console::{reset_color, set_color, Color};
use habbo_core::outln;

use crate::net;
use crate::stj::{Obj, J};
use crate::xml::{self, attr};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexData {
    pub name: Option<String>,
    pub visualization_type: Option<String>,
    pub logic_type: Option<String>,
}

/// `IndexMapper.ParseIndexFileAsync`; `None` (after a red message) when the XML is invalid.
pub fn parse_index_file(index_file_path: &Path) -> Option<IndexData> {
    let result = net::read_all_text(index_file_path).map_err(anyhow::Error::from).and_then(|text| parse_index_text(&text));
    match result {
        Ok(data) => {
            reset_color();
            Some(data)
        }
        Err(error) => {
            set_color(Color::Red);
            outln!("❌ Error parsing _index.bin: {error}");
            reset_color();
            None
        }
    }
}

pub fn parse_index_text(text: &str) -> anyhow::Result<IndexData> {
    let doc = xml::parse(text)?;
    let root = doc.root_element();
    Ok(IndexData {
        name: attr(root, "type").map(str::to_string),
        visualization_type: attr(root, "visualization").map(str::to_string),
        logic_type: attr(root, "logic").map(str::to_string),
    })
}

/// `IndexMapper.GenerateJson`.
pub fn generate_json(index: &IndexData) -> anyhow::Result<String> {
    Obj::new()
        .opt("name", index.name.clone().map(J::Str))
        .opt("logicType", index.logic_type.clone().map(J::Str))
        .opt("visualizationType", index.visualization_type.clone().map(J::Str))
        .build()
        .to_string_indented()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexPetsData {
    pub r#type: Option<String>,
    pub name: Option<String>,
    pub visualization_type: Option<String>,
    pub logic_type: Option<String>,
}

/// `IndexPetsMapper.ParsePetsIndexFileAsync`.
pub fn parse_pets_index_file(index_file_path: &Path) -> Option<IndexPetsData> {
    let result = net::read_all_text(index_file_path).map_err(anyhow::Error::from).and_then(|text| parse_pets_index_text(&text));
    match result {
        Ok(data) => {
            reset_color();
            Some(data)
        }
        Err(error) => {
            set_color(Color::Red);
            outln!("❌ Error parsing _index.bin for pets: {error}");
            reset_color();
            None
        }
    }
}

pub fn parse_pets_index_text(text: &str) -> anyhow::Result<IndexPetsData> {
    let doc = xml::parse(text)?;
    let root = doc.root_element();
    Ok(IndexPetsData {
        r#type: attr(root, "logic").map(str::to_string),
        name: attr(root, "type").map(str::to_string),
        visualization_type: attr(root, "visualization").map(str::to_string),
        logic_type: attr(root, "logic").map(str::to_string),
    })
}

/// `IndexPetsMapper.GeneratePetsJson`.
pub fn generate_pets_json(index: &IndexPetsData) -> anyhow::Result<String> {
    Obj::new()
        .opt("type", index.r#type.clone().map(J::Str))
        .opt("name", index.name.clone().map(J::Str))
        .opt("logicType", index.logic_type.clone().map(J::Str))
        .opt("visualizationType", index.visualization_type.clone().map(J::Str))
        .build()
        .to_string_indented()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_index_xml() {
        let index = parse_index_text(r#"<?xml version="1.0"?><object type="chair" visualization="furniture_animated" logic="furniture_multistate"/>"#).unwrap();
        assert_eq!(index.name.as_deref(), Some("chair"));
        assert_eq!(index.logic_type.as_deref(), Some("furniture_multistate"));
        let json = generate_json(&index).unwrap();
        assert_eq!(
            json,
            "{\n  \"name\": \"chair\",\n  \"logicType\": \"furniture_multistate\",\n  \"visualizationType\": \"furniture_animated\"\n}"
                .replace('\n', habbo_core::paths::NEWLINE)
        );
        let pets = parse_pets_index_text(r#"<object type="dog" visualization="pet_animated" logic="pet"/>"#).unwrap();
        assert_eq!(pets.r#type.as_deref(), Some("pet"));
        assert_eq!(pets.name.as_deref(), Some("dog"));
        assert!(parse_index_text("not xml").is_err());
    }
}
