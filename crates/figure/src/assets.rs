//! `ClothesAssetsMapper` and `EffectAssetsMapper`: asset offsets from the `*_manifest` binary,
//! the `source` links from the symbolClass CSV, and (effects) the manifest aliases.

use std::path::Path;

use habbo_core::console::{reset_color, set_color, Color};
use habbo_core::outln;
use indexmap::IndexMap;

use crate::stj::{Node, ObjectBuilder};
use crate::symbols::extract_symbol_class_tags;
use crate::util::{ends_with_ignore_case, read_all_text, try_parse_bool, try_parse_int};
use crate::xml;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Asset {
    pub x: i32,
    pub y: i32,
    pub source: Option<String>,
}

impl Asset {
    pub fn to_node(&self) -> Node {
        ObjectBuilder::new()
            .put("x", Node::int(self.x as i64))
            .put("y", Node::int(self.y as i64))
            .opt_str("source", &self.source)
            .build()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Alias {
    pub link: Option<String>,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Alias {
    pub fn to_node(&self) -> Node {
        ObjectBuilder::new()
            .opt_str("link", &self.link)
            .put("flipH", Node::bool(self.flip_h))
            .put("flipV", Node::bool(self.flip_v))
            .build()
    }
}

pub type Assets = IndexMap<String, Asset>;
pub type Aliases = IndexMap<String, Alias>;

pub fn assets_node(assets: &Assets) -> Node {
    Node::Object(assets.iter().map(|(k, v)| (k.clone(), v.to_node())).collect())
}

pub fn aliases_node(aliases: &Aliases) -> Node {
    Node::Object(aliases.iter().map(|(k, v)| (k.clone(), v.to_node())).collect())
}

/// `EffectAssetsMapper.AssetData`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssetData {
    pub library_name: String,
    pub assets: Assets,
    pub aliases: Aliases,
}

#[derive(Clone, Copy, PartialEq)]
enum Flavor {
    Clothes,
    Effects,
}

/// `ClothesAssetsMapper.ParseAssetsFileAsync`: (library name, assets).
pub fn parse_clothes_assets_file(manifest_file_path: &Path, debug_xml_path: &Path) -> (String, Assets) {
    let data = parse_assets_file(manifest_file_path, debug_xml_path, Flavor::Clothes);
    (data.library_name, data.assets)
}

/// `EffectAssetsMapper.ParseAssetsFileAsync`: library name, assets and aliases.
pub fn parse_effect_assets_file(manifest_file_path: &Path, debug_xml_path: &Path) -> AssetData {
    parse_assets_file(manifest_file_path, debug_xml_path, Flavor::Effects)
}

fn parse_assets_file(manifest_file_path: &Path, debug_xml_path: &Path, flavor: Flavor) -> AssetData {
    let mut data = AssetData::default();
    let result = (|| -> Result<(), String> {
        if !manifest_file_path.is_file() {
            outln!("❌ Error: Manifest file not found: {}", manifest_file_path.display());
            return Ok(());
        }
        let content = read_all_text(manifest_file_path).map_err(|e| e.to_string())?;
        let doc = xml::parse(&content).map_err(|e| e.to_string())?;
        let root = doc.root_element();

        data.assets = map_assets_from_manifest(&root, flavor);
        data.library_name = xml::element(&root, "library").and_then(|l| xml::attr(&l, "name")).unwrap_or_default();

        build_mappings_in_memory(&mut data.assets, debug_xml_path);

        if flavor == Flavor::Effects {
            data.aliases = map_aliases_from_manifest(&root);
        }
        Ok(())
    })();
    if let Err(message) = result {
        set_color(Color::Red);
        outln!("❌ Error parsing manifest file: {message}");
    }
    reset_color();
    data
}

/// `MapAssetsFromManifest`: `image/png` assets with an `offset` param.
pub fn map_assets_from_manifest_xml(text: &str, clothes: bool) -> Result<Assets, roxmltree::Error> {
    let doc = xml::parse(text)?;
    Ok(map_assets_from_manifest(&doc.root_element(), if clothes { Flavor::Clothes } else { Flavor::Effects }))
}

fn map_assets_from_manifest(root: &roxmltree::Node, flavor: Flavor) -> Assets {
    let mut output = Assets::new();

    let Some(library) = xml::element(root, "library") else {
        if flavor == Flavor::Clothes {
            outln!("❌ No <library> element found in manifest.");
        }
        return output;
    };
    let Some(assets) = xml::element(&library, "assets") else {
        if flavor == Flavor::Clothes {
            outln!("❌ No <assets> element found in manifest.");
        }
        return output;
    };

    for asset in xml::elements(&assets, "asset") {
        if asset.attribute("mimeType") != Some("image/png") {
            continue;
        }
        let Some(name) = asset.attribute("name").filter(|n| !n.is_empty()) else { continue };
        let Some(param) = xml::elements(&asset, "param").find(|p| p.attribute("key") == Some("offset")) else {
            continue;
        };
        let Some(offset) = param.attribute("value").filter(|v| !v.is_empty()) else { continue };

        let parts: Vec<&str> = offset.split(',').collect();
        if parts.len() != 2 {
            continue;
        }
        let (Some(x), Some(y)) = (try_parse_int(Some(parts[0].trim())), try_parse_int(Some(parts[1].trim()))) else {
            continue;
        };
        output.insert(name.to_string(), Asset { x, y, source: None });
    }
    output
}

/// `EffectAssetsMapper.MapAliasesFromManifest`.
pub fn map_aliases_from_manifest_xml(text: &str) -> Result<Aliases, roxmltree::Error> {
    let doc = xml::parse(text)?;
    Ok(map_aliases_from_manifest(&doc.root_element()))
}

fn map_aliases_from_manifest(root: &roxmltree::Node) -> Aliases {
    let mut output = Aliases::new();
    let Some(library) = xml::element(root, "library") else { return output };
    let Some(aliases) = xml::element(&library, "aliases") else { return output };

    for alias in xml::elements(&aliases, "alias") {
        let Some(name) = alias.attribute("name").filter(|n| !n.is_empty()) else { continue };
        let Some(link) = alias.attribute("link").filter(|l| !l.is_empty()) else { continue };

        let flip = |attr: &str| {
            let value = alias.attribute(attr);
            try_parse_bool(value).unwrap_or_else(|| try_parse_int(value).is_some_and(|n| n != 0))
        };
        output.insert(name.to_string(), Alias { link: Some(link.to_string()), flip_h: flip("fliph"), flip_v: flip("flipv") });
    }
    output
}

/// `BuildMappingsInMemoryAsync`: gives each asset the `source` of its symbol id (the first
/// name of the id in the CSV, with the SWF prefix removed).
pub fn build_mappings_in_memory(assets: &mut Assets, debug_xml_path: &Path) {
    let tag_mappings = extract_symbol_class_tags(debug_xml_path);
    apply_symbol_class_tags(assets, &tag_mappings);
}

pub fn apply_symbol_class_tags(assets: &mut Assets, tag_mappings: &IndexMap<String, Vec<String>>) {
    let mut lines = Vec::new();

    let swf_prefix = tag_mappings.get("0").and_then(|names| names.first().cloned()).unwrap_or_default();
    if swf_prefix.is_empty() {
        outln!("⚠️ Warning: Unable to determine SWF file prefix (ID 0).");
    }

    for (tag_id, names) in tag_mappings {
        if tag_id == "0" {
            continue;
        }
        for original in names {
            let cleaned = remove_swf_prefix(original, &swf_prefix);
            if ["visualization", "logic", "index", "assets", "manifest"]
                .iter()
                .any(|suffix| ends_with_ignore_case(&cleaned, suffix))
            {
                continue;
            }
            lines.push(format!("{tag_id},{cleaned}"));
        }
    }

    // UpdateAssetsWithSourceFromCsvLinesAsync
    let mut source_map: IndexMap<String, String> = IndexMap::new();
    for line in &lines {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 2 {
            continue;
        }
        let (id, name) = (parts[0], parts[1]);
        if let Some(source) = source_map.get(id) {
            if let Some(asset) = assets.get_mut(name) {
                asset.source = Some(source.clone());
            }
        } else {
            source_map.insert(id.to_string(), name.to_string());
        }
    }
}

/// `RemoveSwfPrefix`: strips `<prefix>_` from the start, ignoring case.
pub fn remove_swf_prefix(name: &str, swf_prefix: &str) -> String {
    if name.is_empty() || swf_prefix.is_empty() {
        return name.to_string();
    }
    match regex::Regex::new(&format!("(?i)^{}_", regex::escape(swf_prefix))) {
        Ok(re) => re.replacen(name, 1, "").into_owned(),
        Err(_) => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <library name="hh_human_shirt" version="0.1">
    <assets>
      <asset name="h_std_ch_1_0_0" mimeType="image/png"><param key="offset" value="-10, 22"/></asset>
      <asset name="h_std_ch_1_1_0" mimeType="image/png"><param key="offset" value="3,4"/></asset>
      <asset name="sh_std_ch_1_0_0" mimeType="image/png"><param key="offset" value="1,2"/></asset>
      <asset name="no_offset" mimeType="image/png"><param key="other" value="1,2"/></asset>
      <asset name="bad" mimeType="image/png"><param key="offset" value="1,2,3"/></asset>
      <asset name="xml" mimeType="text/xml"><param key="offset" value="1,2"/></asset>
    </assets>
    <aliases>
      <alias name="a1" link="h_std_ch_1_0_0" fliph="1" flipv="false"/>
      <alias name="a2" link="h_std_ch_1_1_0" fliph="True"/>
      <alias name="a3" link=""/>
    </aliases>
  </library>
</manifest>"#;

    #[test]
    fn manifest_assets_and_aliases() {
        let assets = map_assets_from_manifest_xml(MANIFEST, true).unwrap();
        assert_eq!(assets.keys().collect::<Vec<_>>(), vec!["h_std_ch_1_0_0", "h_std_ch_1_1_0", "sh_std_ch_1_0_0"]);
        assert_eq!(assets["h_std_ch_1_0_0"], Asset { x: -10, y: 22, source: None });

        let aliases = map_aliases_from_manifest_xml(MANIFEST).unwrap();
        assert_eq!(aliases.len(), 2);
        assert_eq!(aliases["a1"], Alias { link: Some("h_std_ch_1_0_0".into()), flip_h: true, flip_v: false });
        assert!(aliases["a2"].flip_h && !aliases["a2"].flip_v);
    }

    #[test]
    fn sources_come_from_the_first_name_of_each_id() {
        let mut assets = map_assets_from_manifest_xml(MANIFEST, true).unwrap();
        let mut tags = IndexMap::new();
        tags.insert("0".to_string(), vec!["hh_human_shirt".to_string()]);
        tags.insert("1".to_string(), vec!["HH_HUMAN_SHIRT_h_std_ch_1_0_0".to_string(), "hh_human_shirt_h_std_ch_1_1_0".to_string()]);
        tags.insert("2".to_string(), vec!["hh_human_shirt_manifest".to_string()]);
        apply_symbol_class_tags(&mut assets, &tags);
        assert_eq!(assets["h_std_ch_1_1_0"].source.as_deref(), Some("h_std_ch_1_0_0"));
        assert_eq!(assets["h_std_ch_1_0_0"].source, None);

        let json = crate::stj::to_string_indented(&assets_node(&assets));
        let nl = habbo_core::paths::NEWLINE;
        assert!(json.contains(&format!("\"h_std_ch_1_1_0\": {{{nl}    \"x\": 3,{nl}    \"y\": 4,{nl}    \"source\": \"h_std_ch_1_0_0\"{nl}  }}")));
    }

    #[test]
    fn prefix_removal_ignores_case() {
        assert_eq!(remove_swf_prefix("HH_Fx_a", "hh_fx"), "a");
        assert_eq!(remove_swf_prefix("x_hh_fx_a", "hh_fx"), "x_hh_fx_a");
        assert_eq!(remove_swf_prefix("a.b_c", "a.b"), "c");
        assert_eq!(remove_swf_prefix("axb_c", "a.b"), "axb_c");
    }
}
