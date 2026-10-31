//! The symbolClass readers: `DebugXmlParser` (shared with the furniture compiler) and
//! `ClothesDebugXmlParser` / `EffectXMLParser` (identical in the C# code, ported once as
//! [`get_image_mapping`]). FFDec writes `symbolClass/symbols.csv` as `tag;"name"` lines.

use std::path::Path;

use habbo_core::outln;
use indexmap::IndexMap;

use crate::util::{is_null_or_white_space, read_all_lines};
use crate::xml;

/// A `Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)`: keys stored lower-cased.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CiMap(IndexMap<String, String>);

impl CiMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: &str, value: String) {
        self.0.insert(key.to_lowercase(), value);
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(&key.to_lowercase())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `(tag, name)` pairs of the CSV lines (`tag;name`, quotes removed).
fn csv_entries(lines: &[String]) -> Vec<(String, String)> {
    lines
        .iter()
        .filter(|l| !is_null_or_white_space(l))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() < 2 {
                return None;
            }
            Some((parts[0].trim().to_string(), parts[1].trim().replace('"', "")))
        })
        .collect()
}

/// `GroupBy(e => e.Tag)`: groups in first-appearance order.
fn group_by_tag(entries: Vec<(String, String)>) -> IndexMap<String, Vec<String>> {
    let mut groups: IndexMap<String, Vec<String>> = IndexMap::new();
    for (tag, name) in entries {
        groups.entry(tag).or_default().push(name);
    }
    groups
}

/// `DebugXmlParser.ParseDebugXml`: for each tag with two or more names, maps the second
/// name (object) to the first (source), lower-cased.
pub fn parse_debug_xml(csv_path: &Path) -> CiMap {
    let mut mapping = CiMap::new();
    if !csv_path.is_file() {
        outln!("❌ Error CSV file not found: {}", csv_path.display());
        return mapping;
    }
    match read_all_lines(csv_path) {
        Ok(lines) => {
            for names in group_by_tag(csv_entries(&lines)).into_values() {
                if names.len() >= 2 {
                    mapping.insert(&names[1].to_lowercase(), names[0].to_lowercase());
                }
            }
        }
        Err(error) => outln!("❌ Error parsing CSV: {error}"),
    }
    mapping
}

/// `DebugXmlParser.ExtractSymbolClassTags`: tag -> names, in file order.
pub fn extract_symbol_class_tags(csv_path: &Path) -> IndexMap<String, Vec<String>> {
    if !csv_path.is_file() {
        outln!("❌ Error CSV file not found: {}", csv_path.display());
        return IndexMap::new();
    }
    match read_all_lines(csv_path) {
        Ok(lines) => group_by_tag(csv_entries(&lines)),
        Err(error) => {
            outln!("❌ Error extracting symbol class tags: {error}");
            IndexMap::new()
        }
    }
}

/// `ClothesDebugXmlParser.GetClothesImageMapping` / `EffectXMLParser.GetEffectsImageMapping`.
pub fn get_image_mapping(file_path: &Path) -> CiMap {
    if !file_path.is_file() {
        outln!("❌ Debug file not found: {}", file_path.display());
        return CiMap::new();
    }
    if habbo_core::paths::has_ext(file_path, "csv") {
        parse_symbols_csv(file_path)
    } else {
        parse_xml(file_path)
    }
}

fn parse_symbols_csv(csv_path: &Path) -> CiMap {
    let mut mapping = CiMap::new();
    match read_all_lines(csv_path) {
        Ok(lines) => mapping = symbols_csv_mapping(&lines),
        Err(error) => outln!("❌ Error parsing CSV: {error}"),
    }
    mapping
}

/// The CSV mapping of [`get_image_mapping`]: object -> source for groups of two or more
/// names, name -> itself for single names (all lower-cased).
pub fn symbols_csv_mapping(lines: &[String]) -> CiMap {
    let mut mapping = CiMap::new();
    for names in group_by_tag(csv_entries(lines)).into_values() {
        if names.len() >= 2 {
            mapping.insert(&names[1].to_lowercase(), names[0].to_lowercase());
        } else if names.len() == 1 {
            let only = names[0].to_lowercase();
            mapping.insert(&only, only.clone());
        }
    }
    mapping
}

fn parse_xml(xml_path: &Path) -> CiMap {
    match crate::util::read_all_text(xml_path) {
        Ok(text) => symbols_xml_mapping(&text),
        Err(error) => {
            outln!("❌ Error parsing XML: {error}");
            CiMap::new()
        }
    }
}

/// The FFDec debug.xml variant: `<item type="SymbolClassTag"><tags><item>..</item></tags>
/// <names><item>..</item></names></item>`.
pub fn symbols_xml_mapping(text: &str) -> CiMap {
    let mut mapping = CiMap::new();
    let doc = match xml::parse(text) {
        Ok(doc) => doc,
        Err(error) => {
            outln!("❌ Error parsing XML: {error}");
            return mapping;
        }
    };
    let symbol_class_tag = doc
        .root()
        .descendants()
        .find(|n| xml::is_named(n, "item") && n.attribute("type") == Some("SymbolClassTag"));
    let Some(symbol_class_tag) = symbol_class_tag else {
        outln!("❌ Error SymbolClassTag not found in XML.");
        return mapping;
    };

    let items = |name: &str| {
        xml::element(&symbol_class_tag, name)
            .map(|e| xml::elements(&e, "item").map(|i| xml::value(&i)).collect::<Vec<_>>())
    };
    let (Some(tags), Some(names)) = (items("tags"), items("names")) else {
        outln!("❌ Error: Tags and names count mismatch in XML.");
        return mapping;
    };
    if tags.len() != names.len() {
        outln!("❌ Error: Tags and names count mismatch in XML.");
        return mapping;
    }

    let mut temp: IndexMap<String, Vec<String>> = IndexMap::new();
    for (tag, name) in tags.into_iter().zip(names) {
        temp.entry(tag).or_default().push(name);
    }
    for names in temp.into_values() {
        if names.len() >= 2 {
            mapping.insert(&names[1].to_lowercase(), names[0].to_lowercase());
        }
    }
    mapping
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        crate::util::split_lines(text)
    }

    #[test]
    fn csv_mapping_groups_by_tag() {
        let map = symbols_csv_mapping(&lines(
            "0;\"hh_shirt\"\r\n1;\"hh_shirt_h_std_ch_1_0_0\"\r\n1;\"H_Std_Ch_1_1_0\"\r\n\r\n2;\"single\"\r\nbad line\r\n",
        ));
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("h_std_ch_1_1_0").unwrap(), "hh_shirt_h_std_ch_1_0_0");
        assert_eq!(map.get("SINGLE").unwrap(), "single");
        assert_eq!(map.get("hh_shirt").unwrap(), "hh_shirt");
    }

    #[test]
    fn xml_mapping_needs_two_names() {
        let map = symbols_xml_mapping(
            "<swf><item type=\"SymbolClassTag\"><tags><item>1</item><item>1</item><item>2</item></tags>\
             <names><item>Src</item><item>Obj</item><item>Lone</item></names></item></swf>",
        );
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("obj").unwrap(), "src");
    }

    #[test]
    fn symbol_class_tags_keep_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("symbols.csv");
        std::fs::write(&path, "0;\"lib\"\n5;\"lib_a\"\n3;\"lib_b\"\n5;\"lib_c\"\n").unwrap();
        let tags = extract_symbol_class_tags(&path);
        assert_eq!(tags.keys().collect::<Vec<_>>(), vec!["0", "5", "3"]);
        assert_eq!(tags["5"], vec!["lib_a", "lib_c"]);
        let debug = parse_debug_xml(&path);
        assert_eq!(debug.get("lib_c").unwrap(), "lib_a");
        assert_eq!(debug.len(), 1);
    }
}
