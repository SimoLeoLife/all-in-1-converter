//! `EffectsDownloader` (Effects.cs): effectmap.xml + HabboAvatarActions.xml, every effect
//! library into Habbo_Default/effects/{swf,hab}, and EffectMap.json / HabboAvatarActions.json.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use habbo_core::paths::HabboAssetFolder;
use habbo_core::{console, outln, settings, Color};
use serde_json::{json, Map, Value};

use crate::asset_downloader;
use crate::common::{self, colored, parse_double, parse_int, parse_int_strict, FetchError};
use crate::jsonw::{to_string_indented, Escaping};
use crate::xml::{Document, Element};

const JSON_DIRECTORY: &str = "./Habbo_Default/files/json";

/// `EffectsDownloader.DownloadEffectsAsync`.
pub async fn download_effects() -> Result<()> {
    let config = common::load_config()?;
    let external_vars_url = common::cfg(&config, "AppSettings:externalvarsurl")?;
    let effect_url = common::cfg(&config, "AppSettings:effecturl")?;

    if let Err(error) = run(&external_vars_url, &effect_url).await {
        colored(Color::Red, &format!("Error downloading effects: {error}"));
    }
    Ok(())
}

async fn run(external_vars_url: &str, effect_url: &str) -> Result<()> {
    // GetAsync without EnsureSuccessStatusCode: an error page is read like the real file.
    let response = common::send(common::default_client(), external_vars_url).await?;
    let source = common::decode_text(&common::body(response).await?);

    let release_effect = match common::release_from_line_drop_last(&source) {
        Some(Ok(release)) => {
            colored(Color::Blue, &format!("📃 Downloading Effects from habbo version: {release}"));
            release
        }
        Some(Err(_)) => {
            colored(Color::Red, "Error: Insufficient parts in URL to determine release effect.");
            return Ok(());
        }
        None => String::new(),
    };
    if release_effect.is_empty() {
        colored(Color::Red, "Error: Could not determine the release version.");
        return Ok(());
    }

    outln!("Would you like to use the Default effectmap.xml or do you want to use a custom effectmap.xml?");
    outln!("The custom effectmap must be in: SWFCompiler/import/effects/CustomXML");
    outln!("Please select (C)-Custom or (D)-Default:");
    let choice = console::read_line_opt();

    let default_path = "./Habbo_Default/files/xml/effectmap.xml";
    let effect_map_file_path = match choice.as_deref() {
        Some(c) if c.to_uppercase() == "C" && !c.trim().is_empty() => {
            let path = "./SWFCompiler/import/effects/CustomXML/effectmap.xml";
            if !Path::new(path).is_file() {
                colored(Color::Red, &format!("Error: Custom effectmap.xml file not found at {path}"));
                return Ok(());
            }
            path
        }
        // Empty, "D" or anything unrecognised: the default effectmap.xml, downloaded.
        _ => {
            let url = format!("{effect_url}/{release_effect}/effectmap.xml");
            download_file(&url, default_path, "effectmap.xml").await?;
            default_path
        }
    };

    let avatar_actions_url = format!("{effect_url}/{release_effect}/HabboAvatarActions.xml");
    let avatar_actions_file_path = "./Habbo_Default/files/xml/HabboAvatarActions.xml";
    download_file(&avatar_actions_url, avatar_actions_file_path, "HabboAvatarActions.xml").await?;

    download_swf_files(effect_url, &release_effect, effect_map_file_path).await;

    generate_json_from_xml(effect_map_file_path);
    generate_avatar_actions_json_from_xml(avatar_actions_file_path);

    colored(Color::Green, "🎉 Effects Downloaded, JSON generated, and Saved");
    Ok(())
}

async fn download_swf_files(effect_url: &str, release_effect: &str, effect_map_file_path: &str) {
    let result: Result<()> = async {
        if !Path::new(effect_map_file_path).is_file() {
            // NOTE(port): the C# code leaves the colour red here.
            console::set_color(Color::Red);
            outln!("Error: effectmap.xml file not found at {effect_map_file_path}");
            return Ok(());
        }

        let doc = Document::load(effect_map_file_path)?;
        let items = effect_libraries(&doc);
        outln!("Found {} effect libraries in effectmap.xml.", items.len());

        let folder = HabboAssetFolder::EFFECTS;
        outln!(
            "Download format: {} (config.ini download_format) into {}",
            settings::download_format(),
            folder.root().display()
        );
        asset_downloader::prepare_folder(folder).await?;

        let libraries = items
            .into_iter()
            .map(|item| {
                let url = format!("{effect_url}/{release_effect}/{item}");
                (item, url)
            })
            .collect();
        let counts = asset_downloader::download_libraries(folder, libraries).await?;
        outln!("{}", asset_downloader::summary("effect files", &counts));
        Ok(())
    }
    .await;
    if let Err(error) = result {
        colored(Color::Red, &format!("❌ downloading SWF files: {error}"));
    }
}

/// The distinct non-empty `lib` attributes of every `effect` element.
pub fn effect_libraries(doc: &Document) -> Vec<String> {
    let mut seen = HashSet::new();
    doc.descendants("effect")
        .into_iter()
        .filter_map(|effect| effect.attr("lib"))
        .filter(|lib| !lib.is_empty() && seen.insert(lib.to_string()))
        .map(str::to_string)
        .collect()
}

/// EffectMap.json: `{ effects: [{ id, lib, type, revision }] }`.
pub fn effect_map(doc: &Document) -> Result<Value> {
    let effects = doc
        .descendants("effect")
        .into_iter()
        .map(|x| {
            Ok(json!({
                "id": x.attr("id"),
                "lib": x.attr("lib"),
                "type": x.attr("type"),
                "revision": parse_int_strict(x.attr("revision"))?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "effects": effects }))
}

fn generate_json_from_xml(effect_map_file_path: &str) {
    if !Path::new(effect_map_file_path).is_file() {
        console::set_color(Color::Red);
        outln!("❌ effectmap.xml file not found at {effect_map_file_path}");
        return;
    }
    let result = (|| -> Result<PathBuf> {
        let value = effect_map(&Document::load(effect_map_file_path)?)?;
        let json = to_string_indented(&value, Escaping::Default);
        std::fs::create_dir_all(JSON_DIRECTORY)?;
        let path = Path::new(JSON_DIRECTORY).join("EffectMap.json");
        std::fs::write(&path, json)?;
        Ok(path)
    })();
    match result {
        Ok(path) => outln!("📦 Generated JSON : {}", path.display()),
        Err(error) => colored(Color::Red, &format!("❌ generating JSON: {error}")),
    }
}

fn nullable_bool(element: &Element, name: &str) -> Option<bool> {
    let value = element.attr(name)?;
    (!value.is_empty() && (value == "1" || value.to_lowercase() == "true")).then_some(true)
}

fn list_attr(element: &Element, name: &str) -> Vec<String> {
    match element.attr(name) {
        None => Vec::new(),
        Some(value) => value.split(',').filter(|s| !s.is_empty()).map(|s| s.trim().to_string()).collect(),
    }
}

/// Inserts the key unless the value is null (`JsonIgnoreCondition.WhenWritingNull`).
fn put(object: &mut Map<String, Value>, key: &str, value: Value) {
    if !value.is_null() {
        object.insert(key.to_string(), value);
    }
}

fn opt_list(list: Vec<String>) -> Value {
    if list.is_empty() { Value::Null } else { json!(list) }
}

/// One `action` element as the C# `AvatarAction` (nulls left out).
pub fn avatar_action(x: &Element) -> Value {
    let mut params: Vec<&Element> = x.element("params").map(|p| p.elements("param").collect()).unwrap_or_default();
    if params.is_empty() {
        params = x.elements("param").collect();
    }

    let mut object = Map::new();
    put(&mut object, "id", json!(x.attr("id")));
    put(&mut object, "state", json!(x.attr("state")));
    put(&mut object, "precedence", json!(x.attr("precedence").and_then(parse_int)));
    put(&mut object, "animation", json!(nullable_bool(x, "animation")));
    put(&mut object, "main", json!(nullable_bool(x, "main")));
    put(&mut object, "geometryType", json!(x.attr("geometrytype")));
    put(&mut object, "activePartSet", json!(x.attr("activepartset")));
    put(&mut object, "assetPartDefinition", json!(x.attr("assetpartdefinition")));
    put(&mut object, "startFromFrameZero", json!(nullable_bool(x, "startfromframezero")));
    put(&mut object, "preventHeadTurn", json!(nullable_bool(x, "preventheadturn")));
    put(&mut object, "prevents", opt_list(list_attr(x, "prevents")));
    if !params.is_empty() {
        let list: Vec<Value> = params
            .iter()
            .map(|y| {
                let mut param = Map::new();
                put(&mut param, "id", json!(y.attr("id")));
                put(&mut param, "value", json!(y.attr("value")));
                Value::Object(param)
            })
            .collect();
        object.insert("params".into(), Value::Array(list));
    }
    let types: Vec<Value> = x
        .elements("type")
        .map(|y| {
            let animated = y.attr("animated").is_some_and(|a| a == "1" || a.to_lowercase() == "true");
            let mut t = Map::new();
            put(&mut t, "id", json!(y.attr("id")));
            t.insert("animated".into(), json!(animated));
            put(&mut t, "preventHeadTurn", json!(nullable_bool(y, "preventheadturn")));
            put(&mut t, "prevents", opt_list(list_attr(y, "prevents")));
            Value::Object(t)
        })
        .collect();
    if !types.is_empty() {
        object.insert("types".into(), Value::Array(types));
    }
    Value::Object(object)
}

type DefaultOffset = (&'static str, i32, f64, f64, f64);

// habbo-avatar-render-lib action_offset_lay.xml / action_offset_swim.xml, used when no
// action_offset_<state>.xml sits next to HabboAvatarActions.xml.
fn default_action_offsets(state: &str) -> Option<&'static [DefaultOffset]> {
    const LAY: &[DefaultOffset] =
        &[("h", 4, -17.0, 17.0, -0.9), ("h", 2, 22.0, 17.0, -0.9), ("sh", 4, -5.0, 16.0, -0.9), ("sh", 2, 9.0, 16.0, -0.9)];
    const SWIM: &[DefaultOffset] = &[
        ("sh", 0, -28.0, 0.0, 0.0),
        ("sh", 1, -28.0, 0.0, 0.0),
        ("sh", 2, -28.0, 0.0, 0.0),
        ("sh", 3, -28.0, 0.0, 0.0),
        ("sh", 4, 0.0, 0.0, 0.0),
        ("sh", 5, 0.0, 0.0, 0.0),
        ("sh", 6, 0.0, 0.0, 0.0),
        ("sh", 7, -28.0, 0.0, 0.0),
    ];
    match state {
        "lay" => Some(LAY),
        "swim" => Some(SWIM),
        _ => None,
    }
}

fn offset_value(size: &str, direction: i32, x: f64, y: f64, z: f64) -> Value {
    json!({ "size": size, "direction": direction, "x": x, "y": y, "z": z })
}

/// AIR reads an "action_offset_<state>" asset for every action state (AvatarActionManager.parseActionOffsets).
pub fn build_action_offsets(actions: &[Value], xml_directory: &Path) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for state in actions.iter().filter_map(|a| a.get("state").and_then(Value::as_str)) {
        if state.is_empty() || !seen.insert(state.to_string()) {
            continue;
        }
        let offset_file_path = xml_directory.join(format!("action_offset_{state}.xml"));
        let offsets: Option<Vec<Value>> = if offset_file_path.is_file() {
            let doc = Document::load(&offset_file_path)?;
            Some(
                doc.descendants("offset")
                    .into_iter()
                    .filter_map(|o| {
                        let size = o.attr("size").filter(|s| !s.is_empty())?;
                        let direction = o.attr("direction").and_then(parse_int)?;
                        let x = o.attr("x").and_then(parse_int).unwrap_or(0);
                        let y = o.attr("y").and_then(parse_int).unwrap_or(0);
                        let z = o.attr("z").and_then(parse_double).unwrap_or(0.0);
                        Some(offset_value(size, direction, f64::from(x), f64::from(y), z))
                    })
                    .collect(),
            )
        } else {
            default_action_offsets(state)
                .map(|defaults| defaults.iter().map(|&(s, d, x, y, z)| offset_value(s, d, x, y, z)).collect())
        };

        if let Some(offsets) = offsets.filter(|o| !o.is_empty()) {
            result.push(json!({ "action": state, "offsets": offsets }));
        }
    }
    Ok(result)
}

/// HabboAvatarActions.json: `{ actions, actionOffsets }`.
pub fn avatar_actions(doc: &Document, xml_directory: &Path) -> Result<Value> {
    let actions: Vec<Value> = doc.descendants("action").into_iter().map(avatar_action).collect();
    let offsets = build_action_offsets(&actions, xml_directory)?;
    let mut object = Map::new();
    object.insert("actions".into(), Value::Array(actions));
    if !offsets.is_empty() {
        object.insert("actionOffsets".into(), Value::Array(offsets));
    }
    Ok(Value::Object(object))
}

fn generate_avatar_actions_json_from_xml(avatar_actions_file_path: &str) {
    if !Path::new(avatar_actions_file_path).is_file() {
        console::set_color(Color::Red);
        outln!("❌ HabboAvatarActions.xml file not found at {avatar_actions_file_path}");
        return;
    }
    let result = (|| -> Result<PathBuf> {
        let doc = Document::load(avatar_actions_file_path)?;
        let directory = Path::new(avatar_actions_file_path).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let value = avatar_actions(&doc, &directory)?;
        let json = to_string_indented(&value, Escaping::Default);
        std::fs::create_dir_all(JSON_DIRECTORY)?;
        let path = Path::new(JSON_DIRECTORY).join("HabboAvatarActions.json");
        std::fs::write(&path, json)?;
        Ok(path)
    })();
    match result {
        Ok(path) => outln!("📦 Generated JSON : {}", path.display()),
        Err(error) => colored(Color::Red, &format!("❌ generating AvatarActions JSON: {error}")),
    }
}

/// `DownloadFileAsync`: an HTTP failure means "custom" and is skipped; other errors escape.
async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<()> {
    let result: Result<(), FetchError> = async {
        let bytes = common::get_bytes(common::default_client(), url).await?;
        if let Some(directory) = Path::new(file_path).parent() {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(file_path, bytes)?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            colored(Color::Green, &format!("📥 Downloaded: {file_name}"));
            Ok(())
        }
        Err(FetchError::Http(_)) => {
            colored(Color::Yellow, &format!("ℹ️ We will skip downloading {file_name} as this is custom"));
            Ok(())
        }
        Err(FetchError::Other(error)) => Err(anyhow!(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effectmap_libraries_and_json() {
        let doc = Document::parse(
            r#"<map><effect id="1" lib="Dance1" type="dance" revision="3"/><effect id="2" lib="Dance1" type="dance" revision="4"/><effect id="3" lib="" type="fx" revision="0"/></map>"#,
        )
        .unwrap();
        assert_eq!(effect_libraries(&doc), ["Dance1"]);
        let value = effect_map(&doc).unwrap();
        assert_eq!(value["effects"][0].to_string(), r#"{"id":"1","lib":"Dance1","type":"dance","revision":3}"#);
        assert_eq!(value["effects"].as_array().unwrap().len(), 3);

        let bad = Document::parse(r#"<map><effect id="1"/></map>"#).unwrap();
        assert!(effect_map(&bad).is_err(), "int.Parse(null) throws");
    }

    #[test]
    fn avatar_actions_shape() {
        let xml = r#"<actions>
  <action id="Lay" state="lay" precedence="9" main="1" geometrytype="horizontal" activepartset="lay" assetpartdefinition="lay" prevents="Sit, Dance," animation="false">
    <params><param id="default" value="1"/></params>
    <type id="1" animated="TRUE" prevents="a"/>
    <type id="2"/>
  </action>
  <action id="Wave" state="wave" precedence="x"><param id="p"/></action>
  <action id="Std"/>
</actions>"#;
        let dir = tempfile::tempdir().unwrap();
        let doc = Document::parse(xml).unwrap();
        let value = avatar_actions(&doc, dir.path()).unwrap();
        let lay = &value["actions"][0];
        assert_eq!(
            lay.to_string(),
            r#"{"id":"Lay","state":"lay","precedence":9,"main":true,"geometryType":"horizontal","activePartSet":"lay","assetPartDefinition":"lay","prevents":["Sit","Dance"],"params":[{"id":"default","value":"1"}],"types":[{"id":"1","animated":true,"prevents":["a"]},{"id":"2","animated":false}]}"#
        );
        assert_eq!(value["actions"][1].to_string(), r#"{"id":"Wave","state":"wave","params":[{"id":"p"}]}"#);
        assert_eq!(value["actions"][2].to_string(), r#"{"id":"Std"}"#);

        // "lay" gets the built-in offsets, "wave" has none.
        let offsets = value["actionOffsets"].as_array().unwrap();
        assert_eq!(offsets.len(), 1);
        assert_eq!(offsets[0]["action"], "lay");
        assert_eq!(offsets[0]["offsets"][0].to_string(), r#"{"size":"h","direction":4,"x":-17.0,"y":17.0,"z":-0.9}"#);
        let text = crate::jsonw::to_string_with_newline(&offsets[0]["offsets"][0], Escaping::Default, "");
        assert_eq!(text, r#"{  "size": "h",  "direction": 4,  "x": -17,  "y": 17,  "z": -0.9}"#);

        // A file next to the xml wins over the defaults.
        std::fs::write(
            dir.path().join("action_offset_wave.xml"),
            r#"<offsets><offset size="h" direction="2" x="3" y="bad" z="0.5"/><offset size="" direction="1"/><offset size="sh"/></offsets>"#,
        )
        .unwrap();
        let value = avatar_actions(&doc, dir.path()).unwrap();
        let offsets = value["actionOffsets"].as_array().unwrap();
        assert_eq!(offsets[1]["action"], "wave");
        assert_eq!(offsets[1]["offsets"].to_string(), r#"[{"size":"h","direction":2,"x":3.0,"y":0.0,"z":0.5}]"#);
    }

    #[test]
    fn no_offsets_key_without_states() {
        let doc = Document::parse("<actions><action id=\"a\"/></actions>").unwrap();
        let value = avatar_actions(&doc, Path::new(".")).unwrap();
        assert!(value.get("actionOffsets").is_none());
    }
}
