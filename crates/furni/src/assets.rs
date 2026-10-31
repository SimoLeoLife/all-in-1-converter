//! `Mapper/Assests` (sic): `AssetNameMapper`, `DebugXmlParser` (reads symbols.csv) and
//! `AssetsMapper` (assets.bin + manifest.bin → the json `assets` map, plus the per-file
//! image mapping `ImageRestorer` uses).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use habbo_core::console::{reset_color, set_color, Color};
use habbo_core::outln;
use indexmap::IndexMap;

use crate::net::{self, ignore_case_key, lower_invariant, try_parse_int};
use crate::spritesheet::{clean_asset_name, force_cf_upper};
use crate::stj::{Obj, J};
use crate::xml::{self, attr, descendants, elements};

// ---------------------------------------------------------------------------------------
// Case-insensitive string map (`Dictionary<string,string>(StringComparer.OrdinalIgnoreCase)`)
// ---------------------------------------------------------------------------------------

/// A `Dictionary<string, string>` with `StringComparer.OrdinalIgnoreCase`, insertion order kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IgnoreCaseMap {
    entries: IndexMap<String, (String, String)>,
}

impl IgnoreCaseMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// `map[key] = value` (an existing key keeps its original spelling and position).
    pub fn set(&mut self, key: &str, value: String) {
        let folded = ignore_case_key(key);
        if let Some(entry) = self.entries.get_mut(&folded) {
            entry.1 = value;
        } else {
            self.entries.insert(folded, (key.to_string(), value));
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(&ignore_case_key(key))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(&ignore_case_key(key)).map(|(_, v)| v.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// (key as first inserted, value), in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.values().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

// ---------------------------------------------------------------------------------------
// AssetNameMapper
// ---------------------------------------------------------------------------------------

/// `AssetNameMapper.BuildCanonicalMapping`: cleaned (short) asset name → full CSV name.
pub fn build_canonical_mapping(csv_path: &Path) -> IgnoreCaseMap {
    let mut mapping = IgnoreCaseMap::new();
    if !csv_path.is_file() {
        outln!("❌ CSV file not found: {}", csv_path.display());
        return mapping;
    }
    // NOTE(port): File.ReadAllLines would throw on a read error here (C# lets it escape).
    let lines = net::read_all_lines(csv_path).unwrap_or_default();
    for line in lines.iter().filter(|l| !net::is_null_or_white_space(l)) {
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() < 2 {
            continue;
        }
        match try_parse_int(Some(parts[0].trim())) {
            Some(id) if id != 0 => {}
            _ => continue,
        }
        let name_part = parts[1].trim().replace('"', "");
        let lower = lower_invariant(&name_part);
        if ["manifest", "assets", "logic", "visualization", "index"].iter().any(|w| lower.contains(w)) {
            continue;
        }
        let full_name = lower_invariant(&name_part);
        let short_name = clean_asset_name(&full_name, false);
        if !mapping.contains_key(&short_name) {
            mapping.set(&short_name, full_name);
        }
    }
    mapping
}

// ---------------------------------------------------------------------------------------
// DebugXmlParser
// ---------------------------------------------------------------------------------------

fn csv_entries(csv_path: &Path) -> std::io::Result<Vec<(String, String)>> {
    let lines = net::read_all_lines(csv_path)?;
    Ok(lines
        .iter()
        .filter(|l| !net::is_null_or_white_space(l))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(';').collect();
            (parts.len() >= 2).then(|| (parts[0].trim().to_string(), parts[1].trim().replace('"', "")))
        })
        .collect())
}

/// `DebugXmlParser.ParseDebugXml`: for every CSV id with two or more names, maps the second
/// name (lower case) to the first one (lower case).
pub fn parse_debug_xml(csv_path: &Path) -> IgnoreCaseMap {
    let mut mapping = IgnoreCaseMap::new();
    if !csv_path.is_file() {
        outln!("❌ Error CSV file not found: {}", csv_path.display());
        return mapping;
    }
    match csv_entries(csv_path) {
        Ok(entries) => {
            let mut groups: IndexMap<String, Vec<String>> = IndexMap::new();
            for (tag, name) in entries {
                groups.entry(tag).or_default().push(name);
            }
            for names in groups.values() {
                if names.len() >= 2 {
                    mapping.set(&lower_invariant(&names[1]), lower_invariant(&names[0]));
                }
            }
        }
        Err(error) => outln!("❌ Error parsing CSV: {error}"),
    }
    mapping
}

/// `DebugXmlParser.ExtractSymbolClassTags`: CSV id → every name with that id, in order.
pub fn extract_symbol_class_tags(csv_path: &Path) -> IndexMap<String, Vec<String>> {
    let mut tags: IndexMap<String, Vec<String>> = IndexMap::new();
    if !csv_path.is_file() {
        outln!("❌ Error CSV file not found: {}", csv_path.display());
        return tags;
    }
    match csv_entries(csv_path) {
        Ok(entries) => {
            for (tag, name) in entries {
                tags.entry(tag).or_default().push(name);
            }
        }
        Err(error) => outln!("❌ Error extracting symbol class tags: {error}"),
    }
    tags
}

// ---------------------------------------------------------------------------------------
// AssetsMapper
// ---------------------------------------------------------------------------------------

/// `AssetsMapper.Asset`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Asset {
    pub source: Option<String>,
    pub x: i32,
    pub y: i32,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Asset {
    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("source", self.source.as_ref().map(|s| J::str(s.clone())))
            .put("x", J::int(self.x as i64))
            .put("y", J::int(self.y as i64))
            .opt("flipH", self.flip_h.then_some(J::Bool(true)))
            .opt("flipV", self.flip_v.then_some(J::Bool(true)))
            .build()
    }
}

pub fn assets_json(assets: &IndexMap<String, Asset>) -> J {
    J::Obj(assets.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
}

/// The result of one `ParseAssetsFileAsync` call: the assets and the mapping C# kept in
/// `AssetsMapper.LatestImageMapping` (CSV id → image name).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedAssets {
    pub assets: IndexMap<String, Asset>,
    pub image_mapping: HashMap<String, String>,
}

static UNSCOPED_MAPPING: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// `AssetsMapper.LatestImageMapping` outside a file conversion scope: the mapping of the
/// last [`parse_assets_file`] call (empty until one ran). The converters pass the mapping
/// along per file instead, which is what the C# `AsyncLocal` scope achieved.
pub fn latest_image_mapping() -> HashMap<String, String> {
    UNSCOPED_MAPPING.lock().unwrap().clone().unwrap_or_default()
}

/// `AssetsMapper.ParseAssetsFileAsync` called outside a file scope: also updates
/// [`latest_image_mapping`].
pub fn parse_assets_file(
    assets_file_path: &Path,
    image_sources: Option<&IgnoreCaseMap>,
    manifest_file_path: &Path,
    debug_xml_path: &Path,
    swf_output_directory: &Path,
) -> ParsedAssets {
    let parsed = parse_assets_file_scoped(assets_file_path, image_sources, manifest_file_path, debug_xml_path, swf_output_directory);
    *UNSCOPED_MAPPING.lock().unwrap() = Some(parsed.image_mapping.clone());
    parsed
}

/// `AssetsMapper.ParseAssetsFileAsync` inside a file scope (`BeginFile`).
pub(crate) fn parse_assets_file_scoped(
    assets_file_path: &Path,
    image_sources: Option<&IgnoreCaseMap>,
    manifest_file_path: &Path,
    debug_xml_path: &Path,
    _swf_output_directory: &Path,
) -> ParsedAssets {
    let mut result = ParsedAssets::default();
    if image_sources.is_none() {
        outln!("⚠️ WARNING: imageSources is null. Initializing an empty dictionary.");
    }

    let outcome = (|| -> anyhow::Result<()> {
        if !assets_file_path.is_file() {
            outln!("❌ Error Assets file not found: {}", assets_file_path.display());
            return Ok(());
        }
        if !manifest_file_path.is_file() {
            outln!("❌ Error Manifest file not found: {}", manifest_file_path.display());
            return Ok(());
        }
        let assets_content = net::read_all_text(assets_file_path)?;
        let assets_doc = xml::parse(&assets_content)?;
        let manifest_content = net::read_all_text(manifest_file_path)?;
        let manifest_doc = xml::parse(&manifest_content)?;

        let mut assets = map_assets_xml(assets_doc.root_element(), manifest_doc.root_element(), debug_xml_path);
        result.image_mapping = build_mappings_in_memory(&mut assets, debug_xml_path);

        let keys: Vec<String> = assets.keys().cloned().collect();
        for key in keys {
            let source = assets[&key].source.clone();
            if let Some(source) = source.filter(|s| !s.is_empty()) {
                let resolved = resolve_source(&assets, source);
                assets[&key].source = Some(resolved);
            }
        }
        result.assets = assets;
        Ok(())
    })();

    if let Err(error) = outcome {
        set_color(Color::Red);
        outln!("❌ Error parsing assets or manifest file: {error}");
        result = ParsedAssets::default();
    }
    reset_color();
    result
}

/// `AssetsMapper.BuildMappingsInMemoryAsync`; returns the new image mapping.
pub fn build_mappings_in_memory(assets: &mut IndexMap<String, Asset>, debug_xml_path: &Path) -> HashMap<String, String> {
    let tag_mappings = extract_symbol_class_tags(debug_xml_path);
    let mut asset_mapping_lines: Vec<String> = vec!["ID,Name".to_string()];
    let mut image_mapping = HashMap::new();

    let mut swf_prefix = tag_mappings.get("0").and_then(|names| names.first()).cloned().unwrap_or_default();
    if swf_prefix.is_empty() {
        outln!("⚠️ Warning: Unable to determine SWF file prefix (ID 0).");
        swf_prefix = String::new();
    }

    let mut id_tracker = HashSet::new();
    for (tag_id, names) in &tag_mappings {
        if tag_id == "0" {
            continue;
        }
        for original in names {
            let cleaned = force_cf_upper(&remove_swf_prefix(original, &swf_prefix));
            let lower = lower_invariant(&cleaned);
            if ["visualization", "logic", "index", "assets", "manifest"].iter().any(|s| lower.ends_with(s)) {
                continue;
            }
            asset_mapping_lines.push(format!("{tag_id},{cleaned}"));
            if id_tracker.insert(tag_id.clone()) {
                image_mapping.insert(tag_id.clone(), force_cf_upper(original));
            }
        }
    }

    update_assets_with_source_from_csv_lines(assets, asset_mapping_lines.iter().skip(1));
    image_mapping
}

fn update_assets_with_source_from_csv_lines<'a>(assets: &mut IndexMap<String, Asset>, lines: impl Iterator<Item = &'a String>) {
    let mut source_map: HashMap<String, String> = HashMap::new();
    for line in lines {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            continue;
        }
        let id = parts[0];
        let name = force_cf_upper(parts[1]);
        if let Some(source) = source_map.get(id) {
            if let Some(asset) = assets.get_mut(&name) {
                asset.source = Some(source.clone());
            }
        } else {
            source_map.insert(id.to_string(), name);
        }
    }
}

/// Case-insensitive `StartsWith` used by `Regex.Replace(name, "^prefix_", "", IgnoreCase)`.
pub(crate) fn strip_prefix_ignore_case<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let mut name_chars = name.char_indices();
    for p in prefix.chars() {
        let (_, n) = name_chars.next()?;
        if n != p && lower_invariant(&n.to_string()) != lower_invariant(&p.to_string()) {
            return None;
        }
    }
    let rest_start = name_chars.next().map(|(i, _)| i).unwrap_or(name.len());
    Some(&name[rest_start..])
}

/// `AssetsMapper.RemoveSwfPrefix`: drops a leading "{swfPrefix}_" (case-insensitive).
pub fn remove_swf_prefix(name: &str, swf_prefix: &str) -> String {
    if name.is_empty() || swf_prefix.is_empty() {
        return name.to_string();
    }
    let with_underscore = format!("{swf_prefix}_");
    match strip_prefix_ignore_case(name, &with_underscore) {
        Some(rest) => rest.to_string(),
        None => name.to_string(),
    }
}

/// `AssetsMapper.RemoveFirstPrefix`: drops everything up to and including the first '_'.
pub fn remove_first_prefix(name: &str) -> String {
    match name.find('_') {
        Some(index) if index > 0 => name[index + 1..].to_string(),
        _ => name.to_string(),
    }
}

fn map_assets_xml(root: xml::Node, manifest_root: xml::Node, debug_xml_path: &Path) -> IndexMap<String, Asset> {
    let mut output: IndexMap<String, Asset> = IndexMap::new();

    let manifest_names: HashSet<String> = descendants(manifest_root, "asset")
        .filter(|a| attr(*a, "mimeType") == Some("image/png"))
        .map(|a| force_cf_upper(attr(a, "name").unwrap_or("")))
        .collect();

    for element in elements(root, "asset") {
        let name = force_cf_upper(attr(element, "name").unwrap_or(""));
        if !manifest_names.contains(&name) && attr(element, "source").is_none() {
            continue;
        }
        output.insert(
            name,
            Asset {
                x: try_parse_int(attr(element, "x")).unwrap_or(0),
                y: try_parse_int(attr(element, "y")).unwrap_or(0),
                flip_h: attr(element, "flipH") == Some("1"),
                flip_v: attr(element, "flipV") == Some("1"),
                source: attr(element, "source").map(force_cf_upper),
            },
        );
    }

    let debug_mapping = parse_debug_xml(debug_xml_path);
    for (key, value) in debug_mapping.iter() {
        let key = force_cf_upper(key);
        let value = force_cf_upper(value);
        if let Some(asset) = output.get_mut(&key) {
            if asset.source.as_deref().is_none_or(str::is_empty) {
                asset.source = Some(value);
            }
        }
    }
    output
}

fn resolve_source(assets: &IndexMap<String, Asset>, mut source: String) -> String {
    let mut visited = HashSet::new();
    while !source.is_empty() {
        let Some(next) = assets.get(&source).and_then(|a| a.source.clone()).filter(|s| !s.is_empty()) else { break };
        if !visited.insert(source.clone()) {
            break;
        }
        source = next;
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl(s: &str) -> String {
        s.replace('\n', habbo_core::paths::NEWLINE)
    }

    #[test]
    fn maps_assets_and_sources() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("symbols.csv");
        std::fs::write(
            &csv,
            "0;\"chair\"\n1;\"chair_chair_64_a_0_0\"\n1;\"chair_chair_64_a_2_0\"\n2;\"chair_cf_chair_64_b_0_0\"\n3;\"chair_chair_assets\"\n",
        )
        .unwrap();
        let assets_file = dir.path().join("chair_assets.bin");
        std::fs::write(
            &assets_file,
            r#"<assets>
  <asset name="chair_64_a_0_0" x="-10" y="5"/>
  <asset name="chair_64_a_2_0" x="1" y="2" flipH="1"/>
  <asset name="chair_64_a_4_0" source="chair_64_a_2_0" x="1" y="2" flipH="1"/>
  <asset name="cf_chair_64_b_0_0" x="0" y="0"/>
  <asset name="chair_icon_a" x="0" y="0"/>
</assets>"#,
        )
        .unwrap();
        let manifest = dir.path().join("chair_manifest.bin");
        std::fs::write(
            &manifest,
            r#"<manifest><library><assets>
  <asset name="chair_64_a_0_0" mimeType="image/png"/>
  <asset name="chair_64_a_2_0" mimeType="image/png"/>
  <asset name="cf_chair_64_b_0_0" mimeType="image/png"/>
  <asset name="chair_icon_a" mimeType="text/plain"/>
</assets></library></manifest>"#,
        )
        .unwrap();

        let image_sources = parse_debug_xml(&csv);
        let parsed = parse_assets_file(&assets_file, Some(&image_sources), &manifest, &csv, dir.path());
        let json = assets_json(&parsed.assets).to_string_indented().unwrap();
        let expected = r#"{
  "chair_64_a_0_0": {
    "x": -10,
    "y": 5
  },
  "chair_64_a_2_0": {
    "source": "chair_64_a_0_0",
    "x": 1,
    "y": 2,
    "flipH": true
  },
  "chair_64_a_4_0": {
    "source": "chair_64_a_0_0",
    "x": 1,
    "y": 2,
    "flipH": true
  },
  "CF_chair_64_b_0_0": {
    "x": 0,
    "y": 0
  }
}"#;
        assert_eq!(json, nl(expected));
        assert_eq!(parsed.image_mapping.get("1").map(String::as_str), Some("chair_chair_64_a_0_0"));
        assert_eq!(parsed.image_mapping.get("2").map(String::as_str), Some("chair_CF_chair_64_b_0_0"));
        assert!(!parsed.image_mapping.contains_key("3"));
        assert_eq!(latest_image_mapping(), parsed.image_mapping);

        let canonical = build_canonical_mapping(&csv);
        assert_eq!(canonical.get("chair_64_a_0_0"), Some("chair_chair_64_a_0_0"));
        assert_eq!(canonical.get("CHAIR_cf_chair_64_b_0_0"), Some("chair_cf_chair_64_b_0_0"));
    }

    #[test]
    fn prefixes() {
        assert_eq!(remove_swf_prefix("Chair_chair_64", "chair"), "chair_64");
        assert_eq!(remove_swf_prefix("table_64", "chair"), "table_64");
        assert_eq!(remove_first_prefix("a_b_c"), "b_c");
    }
}
