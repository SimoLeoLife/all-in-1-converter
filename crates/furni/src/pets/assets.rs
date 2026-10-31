//! `Mapper/Assets` (pets): `AssetsPetsMapper` (lower-case names, `usesPalette`) and
//! `DebugPetsXmlParser` (identical to the furniture parser).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use habbo_core::outln;
use indexmap::IndexMap;

use crate::assets::{extract_symbol_class_tags, parse_debug_xml, strip_prefix_ignore_case, IgnoreCaseMap};
use crate::net::{self, lower_invariant, try_parse_int};
use crate::stj::{Obj, J};
use crate::xml::{self, attr, descendants, elements};

/// `DebugPetsXmlParser` is a copy of the furniture `DebugXmlParser`.
pub use crate::assets::{extract_symbol_class_tags as debug_pets_extract_symbol_class_tags, parse_debug_xml as debug_pets_parse_debug_xml};

/// `AssetsPetsMapper.Asset`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Asset {
    pub source: Option<String>,
    pub x: i32,
    pub y: i32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub uses_palette: bool,
}

impl Asset {
    pub fn to_json(&self) -> J {
        Obj::new()
            .opt("source", self.source.as_ref().map(|s| J::str(s.clone())))
            .put("x", J::int(self.x as i64))
            .put("y", J::int(self.y as i64))
            .opt("flipH", self.flip_h.then_some(J::Bool(true)))
            .opt("flipV", self.flip_v.then_some(J::Bool(true)))
            .opt("usesPalette", self.uses_palette.then_some(J::Bool(true)))
            .build()
    }
}

pub fn assets_json(assets: &IndexMap<String, Asset>) -> J {
    J::Obj(assets.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedPetAssets {
    pub assets: IndexMap<String, Asset>,
    /// `AssetsPetsMapper.LatestImageMapping`: CSV id → image name.
    pub image_mapping: HashMap<String, String>,
}

/// `AssetsPetsMapper.ParseAssetsFileAsync`.
pub fn parse_assets_file(
    assets_file_path: &Path,
    image_sources: Option<&IgnoreCaseMap>,
    manifest_file_path: &Path,
    debug_xml_path: &Path,
    _swf_file_name: &str,
    _swf_output_directory: &Path,
) -> ParsedPetAssets {
    let empty = IgnoreCaseMap::new();
    let image_sources = image_sources.unwrap_or(&empty);
    let mut result = ParsedPetAssets::default();

    let outcome = (|| -> anyhow::Result<()> {
        if !assets_file_path.is_file() || !manifest_file_path.is_file() {
            outln!("❌ Error: Assets or manifest file not found.");
            return Ok(());
        }
        let assets_content = net::read_all_text(assets_file_path)?;
        let manifest_content = net::read_all_text(manifest_file_path)?;
        let assets_doc = xml::parse(&assets_content)?;
        let manifest_doc = xml::parse(&manifest_content)?;

        let mut assets = map_assets_xml(assets_doc.root_element(), manifest_doc.root_element(), debug_xml_path);
        result.image_mapping = build_mappings_in_memory(&mut assets, debug_xml_path, image_sources);

        let keys: Vec<String> = assets.keys().cloned().collect();
        for key in keys {
            if let Some(source) = assets[&key].source.clone().filter(|s| !s.is_empty()) {
                let resolved = resolve_source(&assets, source);
                assets[&key].source = Some(resolved);
            }
        }
        result.assets = assets;
        Ok(())
    })();

    if let Err(error) = outcome {
        outln!("❌ Error parsing assets or manifest file: {error}");
        return ParsedPetAssets::default();
    }
    result
}

/// `Regex.IsMatch(name, "visualization|logic|index|assets|manifest$")`.
fn is_data_symbol(name: &str) -> bool {
    ["visualization", "logic", "index", "assets"].iter().any(|w| name.contains(w)) || name.ends_with("manifest")
}

/// `AssetsPetsMapper.BuildMappingsInMemoryAsync`; returns the image mapping.
pub fn build_mappings_in_memory(
    assets: &mut IndexMap<String, Asset>,
    debug_xml_path: &Path,
    image_sources: &IgnoreCaseMap,
) -> HashMap<String, String> {
    let tag_mappings = extract_symbol_class_tags(debug_xml_path);
    let prefix = tag_mappings.get("0").and_then(|n| n.first()).cloned().unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut source_map: HashMap<String, String> = HashMap::new();
    let mut id_tracker = HashSet::new();

    for (tag_id, names) in &tag_mappings {
        for tag_name in names {
            let cleaned = remove_swf_prefix(tag_name, &prefix);
            if is_data_symbol(&cleaned) {
                continue;
            }
            if id_tracker.insert(tag_id.clone()) {
                source_map.insert(tag_id.clone(), cleaned.clone());
            }
            lines.push(format!("{tag_id},{cleaned}"));
        }
    }

    // NOTE(port): Skip(1) drops the first real line (there is no header here), like the C#.
    for line in lines.iter().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            continue;
        }
        let id = parts[0];
        let name = lower_invariant(parts[1]);
        if !assets.contains_key(&name) {
            continue;
        }
        if let Some(source) = source_map.get(id) {
            let value = if assets.contains_key(source) {
                source.clone()
            } else {
                image_sources.get(&name).map(str::to_string).unwrap_or_else(|| source.clone())
            };
            assets[&name].source = Some(value);
        }
    }
    source_map
}

/// `AssetsPetsMapper.RemoveSwfPrefix`.
pub fn remove_swf_prefix(name: &str, swf_prefix: &str) -> String {
    if name.is_empty() || swf_prefix.is_empty() {
        return name.to_string();
    }
    match strip_prefix_ignore_case(name, &format!("{swf_prefix}_")) {
        Some(rest) => rest.to_string(),
        None => name.to_string(),
    }
}

fn map_assets_xml(root: xml::Node, manifest_root: xml::Node, debug_xml_path: &Path) -> IndexMap<String, Asset> {
    let mut output = IndexMap::new();
    let manifest_names: HashSet<String> = descendants(manifest_root, "asset")
        .filter(|a| attr(*a, "mimeType") == Some("image/png"))
        .map(|a| attr(a, "name").map(lower_invariant).unwrap_or_default())
        .collect();

    for element in elements(root, "asset") {
        let name = attr(element, "name").map(lower_invariant).unwrap_or_default();
        if !manifest_names.contains(&name) && attr(element, "source").is_none() {
            continue;
        }
        output.insert(
            name,
            Asset {
                source: attr(element, "source").map(lower_invariant),
                x: try_parse_int(attr(element, "x")).unwrap_or(0),
                y: try_parse_int(attr(element, "y")).unwrap_or(0),
                flip_h: attr(element, "flipH") == Some("1"),
                flip_v: attr(element, "flipV") == Some("1"),
                uses_palette: attr(element, "usesPalette") == Some("1"),
            },
        );
    }

    for (key, value) in parse_debug_xml(debug_xml_path).iter() {
        let key = lower_invariant(key);
        if let Some(asset) = output.get_mut(&key) {
            if asset.source.as_deref().is_none_or(str::is_empty) {
                asset.source = Some(lower_invariant(value));
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
            return source;
        }
        if visited.contains(&next) {
            return source;
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
    fn maps_pet_assets() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("symbols.csv");
        std::fs::write(
            &csv,
            "0;pet_dog\n1;pet_dog_dog_64_body_std_0_0\n1;pet_dog_dog_64_body_std_1_0\n2;pet_dog_dog_64_head_std_0_0\n3;pet_dog_dog_assets\n",
        )
        .unwrap();
        let assets_file = dir.path().join("dog_assets.bin");
        std::fs::write(
            &assets_file,
            r#"<assets>
  <asset name="Dog_64_Body_std_0_0" x="1" y="2" usesPalette="1"/>
  <asset name="dog_64_body_std_1_0" x="3" y="4"/>
  <asset name="dog_64_head_std_0_0" x="5" y="6" flipV="1"/>
  <asset name="dog_64_body_std_2_0" source="DOG_64_body_std_1_0" x="0" y="0"/>
</assets>"#,
        )
        .unwrap();
        let manifest = dir.path().join("dog_manifest.bin");
        std::fs::write(
            &manifest,
            r#"<manifest><library><assets><asset name="dog_64_body_std_0_0" mimeType="image/png"/><asset name="dog_64_body_std_1_0" mimeType="image/png"/><asset name="dog_64_head_std_0_0" mimeType="image/png"/></assets></library></manifest>"#,
        )
        .unwrap();
        let image_sources = parse_debug_xml(&csv);
        let parsed = parse_assets_file(&assets_file, Some(&image_sources), &manifest, &csv, "dog.swf", dir.path());
        let json = assets_json(&parsed.assets).to_string_indented().unwrap();
        let expected = r#"{
  "dog_64_body_std_0_0": {
    "source": "dog_64_body_std_0_0",
    "x": 1,
    "y": 2,
    "usesPalette": true
  },
  "dog_64_body_std_1_0": {
    "source": "dog_64_body_std_0_0",
    "x": 3,
    "y": 4
  },
  "dog_64_head_std_0_0": {
    "source": "dog_64_head_std_0_0",
    "x": 5,
    "y": 6,
    "flipV": true
  },
  "dog_64_body_std_2_0": {
    "source": "dog_64_body_std_0_0",
    "x": 0,
    "y": 0
  }
}"#;
        assert_eq!(json, nl(expected));
        assert_eq!(parsed.image_mapping["1"], "dog_64_body_std_0_0");
        assert_eq!(parsed.image_mapping["0"], "pet_dog");
    }
}
