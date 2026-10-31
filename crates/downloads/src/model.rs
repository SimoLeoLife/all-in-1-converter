//! `Model/FiguredataModel.cs` and `Model/FurnitureDataModel.cs`: figuredata.xml / figuremap.xml /
//! furnidata_xml -> the Nitro JSON files, with the same property names and order as the C#
//! classes (`ExactXmlNamingPolicy` for the figure files, `[JsonPropertyName]` for furnidata).

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

use crate::common::{parse_int, parse_int_strict, NULL_REFERENCE};
use crate::jsonw::{to_string_indented, Escaping};
use crate::xml::{Document, Element};

fn int_attr_or_zero(element: &Element, name: &str) -> Result<i32> {
    // int.Parse(attr?.Value ?? "0")
    parse_int_strict(Some(element.attr(name).unwrap_or("0")))
}

fn flag(element: &Element, name: &str) -> bool {
    element.attr(name) == Some("1")
}

/// `ClothesDownloader.ConvertFigureDataToJson`: the FigureData object.
/// `disable_sellable` writes every set as not sellable.
pub fn figure_data(doc: &Document, disable_sellable: bool) -> Result<Value> {
    let root = doc.element("figuredata").ok_or_else(|| anyhow!(NULL_REFERENCE))?;

    let palettes = match root.element("colors") {
        None => Value::Null,
        Some(colors) => {
            let mut list = Vec::new();
            for palette in colors.elements("palette") {
                let mut color_list = Vec::new();
                for color in palette.elements("color") {
                    let mut object = Map::new();
                    object.insert("id".into(), json!(int_attr_or_zero(color, "id")?));
                    object.insert("index".into(), json!(int_attr_or_zero(color, "index")?));
                    object.insert("club".into(), json!(int_attr_or_zero(color, "club")?));
                    object.insert("selectable".into(), json!(flag(color, "selectable")));
                    object.insert("hexCode".into(), json!(color.value().trim()));
                    color_list.push(Value::Object(object));
                }
                let mut object = Map::new();
                object.insert("id".into(), json!(int_attr_or_zero(palette, "id")?));
                object.insert("colors".into(), Value::Array(color_list));
                list.push(Value::Object(object));
            }
            Value::Array(list)
        }
    };

    let set_types = match root.element("sets") {
        None => Value::Null,
        Some(sets) => {
            let mut list = Vec::new();
            for set_type in sets.elements("settype") {
                let mut set_list = Vec::new();
                for set in set_type.elements("set") {
                    let hidden: Vec<Value> = set
                        .elements("hiddenlayers")
                        .flat_map(|h| h.elements("layer"))
                        .map(|layer| json!({ "partType": layer.attr("parttype").unwrap_or("") }))
                        .collect();

                    let mut parts = Vec::new();
                    for part in set.elements("part") {
                        let mut object = Map::new();
                        object.insert("id".into(), json!(int_attr_or_zero(part, "id")?));
                        object.insert("type".into(), json!(part.attr("type").unwrap_or("")));
                        object.insert("colorable".into(), json!(flag(part, "colorable")));
                        object.insert("index".into(), json!(int_attr_or_zero(part, "index")?));
                        object.insert("colorindex".into(), json!(int_attr_or_zero(part, "colorindex")?));
                        parts.push(Value::Object(object));
                    }

                    let mut object = Map::new();
                    object.insert("id".into(), json!(int_attr_or_zero(set, "id")?));
                    object.insert("gender".into(), json!(set.attr("gender").unwrap_or("")));
                    object.insert("club".into(), json!(int_attr_or_zero(set, "club")?));
                    object.insert("colorable".into(), json!(flag(set, "colorable")));
                    object.insert("selectable".into(), json!(flag(set, "selectable")));
                    object.insert("preselectable".into(), json!(flag(set, "preselectable")));
                    object.insert("sellable".into(), json!(!disable_sellable && flag(set, "sellable")));
                    object.insert("parts".into(), Value::Array(parts));
                    // [JsonIgnore(WhenWritingNull)]: no hiddenLayers key when there are none.
                    if !hidden.is_empty() {
                        object.insert("hiddenLayers".into(), Value::Array(hidden));
                    }
                    set_list.push(Value::Object(object));
                }

                let mut object = Map::new();
                object.insert("type".into(), json!(set_type.attr("type").unwrap_or("")));
                object.insert("paletteId".into(), json!(int_attr_or_zero(set_type, "paletteid")?));
                object.insert("mandatory_f_0".into(), json!(flag(set_type, "mand_f_0")));
                object.insert("mandatory_f_1".into(), json!(flag(set_type, "mand_f_1")));
                object.insert("mandatory_m_0".into(), json!(flag(set_type, "mand_m_0")));
                object.insert("mandatory_m_1".into(), json!(flag(set_type, "mand_m_1")));
                object.insert("sets".into(), Value::Array(set_list));
                list.push(Value::Object(object));
            }
            Value::Array(list)
        }
    };

    let mut object = Map::new();
    object.insert("palettes".into(), palettes);
    object.insert("setTypes".into(), set_types);
    Ok(Value::Object(object))
}

/// FigureData.json text (System.Text.Json default encoder, indented).
pub fn figure_data_json(xml: &str, disable_sellable: bool) -> Result<String> {
    let doc = Document::parse(xml)?;
    Ok(to_string_indented(&figure_data(&doc, disable_sellable)?, Escaping::Default))
}

/// `ClothesDownloader.ConvertFigureMapToJson`: the FigureMap object.
pub fn figure_map(doc: &Document) -> Value {
    let libraries: Vec<Value> = doc
        .root
        .elements("lib")
        .map(|lib| {
            let parts: Vec<Value> = lib
                .elements("part")
                .map(|part| {
                    json!({
                        "id": part.attr("id").and_then(parse_int),
                        "type": part.attr("type"),
                    })
                })
                .collect();
            json!({
                "id": lib.attr("id"),
                "revision": lib.attr("revision").and_then(parse_int).unwrap_or(0),
                "parts": parts,
            })
        })
        .collect();
    json!({ "libraries": libraries })
}

pub fn figure_map_json(xml: &str) -> Result<String> {
    let doc = Document::parse(xml)?;
    Ok(to_string_indented(&figure_map(&doc), Escaping::Default))
}

/// The `lib` ids of figuremap.xml, in document order (the C# `XmlReader` loop).
pub fn figure_map_lib_ids(doc: &Document) -> Vec<String> {
    doc.descendants("lib")
        .into_iter()
        .filter_map(|lib| lib.attr("id").filter(|id| !id.is_empty()).map(str::to_string))
        .collect()
}

/// `(int?)element.Element(name) ?? 0`: missing = 0, present but not a number = FormatException.
fn child_int(element: &Element, name: &str) -> Result<i32> {
    match element.element(name) {
        None => Ok(0),
        Some(child) => parse_int_strict(Some(&child.value())),
    }
}

fn child_flag(element: &Element, name: &str) -> Result<bool> {
    Ok(child_int(element, name)? == 1)
}

fn child_string(element: &Element, name: &str, default: &str) -> String {
    element.element(name).map(|e| e.value()).unwrap_or_else(|| default.to_string())
}

/// `FurnitureType.FromXElement`: one furnitype entry ("floor" or "wall").
pub fn furniture_type(element: &Element, kind: &str) -> Result<Value> {
    let floor = kind == "floor";
    let mut object = Map::new();
    let id = match element.attr("id") {
        None => 0,
        Some(value) => parse_int_strict(Some(value))?,
    };
    object.insert("id".into(), json!(id));
    object.insert("classname".into(), json!(element.attr("classname").unwrap_or("")));
    object.insert("revision".into(), json!(child_int(element, "revision")?));
    object.insert("category".into(), json!(child_string(element, "category", "unknown")));

    let (mut defaultdir, mut xdim, mut ydim, mut partcolors) = (0, 0, 0, Value::Null);
    if floor {
        defaultdir = child_int(element, "defaultdir")?;
        xdim = child_int(element, "xdim")?;
        ydim = child_int(element, "ydim")?;
        if let Some(colors) = element.element("partcolors") {
            let list: Vec<Value> = colors
                .elements("color")
                .map(|c| {
                    let mut code = c.value();
                    if !code.is_empty() && !code.starts_with('#') {
                        code.insert(0, '#');
                    }
                    code
                })
                .filter(|c| !c.is_empty())
                .map(Value::String)
                .collect();
            partcolors = json!({ "color": list });
        }
    }
    object.insert("defaultdir".into(), json!(defaultdir));
    object.insert("xdim".into(), json!(xdim));
    object.insert("ydim".into(), json!(ydim));
    object.insert("partcolors".into(), partcolors);
    object.insert("name".into(), json!(child_string(element, "name", "")));
    object.insert("description".into(), json!(child_string(element, "description", "")));
    object.insert("adurl".into(), json!(child_string(element, "adurl", "")));
    object.insert("offerid".into(), json!(child_int(element, "offerid")?));
    object.insert("buyout".into(), json!(child_flag(element, "buyout")?));
    object.insert("rentofferid".into(), json!(child_int(element, "rentofferid")?));
    object.insert("rentbuyout".into(), json!(child_flag(element, "rentbuyout")?));
    object.insert("bc".into(), json!(child_flag(element, "bc")?));
    object.insert("excludeddynamic".into(), json!(child_flag(element, "excludeddynamic")?));
    object.insert("customparams".into(), json!(child_string(element, "customparams", "")));
    object.insert("specialtype".into(), json!(child_int(element, "specialtype")?));

    let (mut canstandon, mut cansiton, mut canlayon) = (false, false, false);
    if floor {
        canstandon = child_flag(element, "canstandon")?;
        cansiton = child_flag(element, "cansiton")?;
        canlayon = child_flag(element, "canlayon")?;
    }
    object.insert("canstandon".into(), json!(canstandon));
    object.insert("cansiton".into(), json!(cansiton));
    object.insert("canlayon".into(), json!(canlayon));
    object.insert("furniline".into(), json!(child_string(element, "furniline", "")));
    object.insert("environment".into(), json!(child_string(element, "environment", "")));
    object.insert("rare".into(), json!(child_flag(element, "rare")?));
    Ok(Value::Object(object))
}

/// `FurnidataConverter.ConvertXmlToJson` without the file IO: furnidata_xml -> FurnitureData.json text.
pub fn furniture_data_json(xml: &str) -> Result<String> {
    let doc = Document::parse(xml)?;
    let root = doc
        .element("furnidata")
        .ok_or_else(|| anyhow!("Invalid XML format: missing 'furnidata' root element."))?;

    let section = |name: &str, kind: &str| -> Result<Value> {
        let Some(items) = root.element(name) else { return Ok(Value::Null) };
        let list = items.elements("furnitype").map(|x| furniture_type(x, kind)).collect::<Result<Vec<_>>>()?;
        Ok(if list.is_empty() { Value::Null } else { json!({ "furnitype": list }) })
    };

    let mut object = Map::new();
    object.insert("roomitemtypes".into(), section("roomitemtypes", "floor")?);
    object.insert("wallitemtypes".into(), section("wallitemtypes", "wall")?);
    Ok(to_string_indented(&Value::Object(object), Escaping::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIGUREDATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<figuredata>
  <colors>
    <palette id="1">
      <color id="10" index="1" club="0" selectable="1"> FFCB98 </color>
      <color id="11" index="2" club="2" selectable="0">E4A671</color>
    </palette>
  </colors>
  <sets>
    <settype type="hr" paletteid="1" mand_m_0="0" mand_f_0="1" mand_m_1="0" mand_f_1="1">
      <set id="100" gender="M" club="0" colorable="1" selectable="1" preselectable="0" sellable="1">
        <part id="1" type="hr" colorable="1" index="0" colorindex="1"/>
      </set>
      <set id="101" gender="U" club="1" colorable="0" selectable="0">
        <part id="2" type="hrb" colorable="0" index="1" colorindex="0"/>
        <hiddenlayers><layer parttype="hr"/><layer parttype="hrb"/></hiddenlayers>
      </set>
    </settype>
  </sets>
</figuredata>"#;

    #[test]
    fn figuredata_maps_names_and_flags() {
        let doc = Document::parse(FIGUREDATA).unwrap();
        let value = figure_data(&doc, false).unwrap();
        let color = &value["palettes"][0]["colors"][0];
        assert_eq!(color.to_string(), r#"{"id":10,"index":1,"club":0,"selectable":true,"hexCode":"FFCB98"}"#);
        let set_type = &value["setTypes"][0];
        assert_eq!(set_type["paletteId"], 1);
        assert_eq!(set_type["mandatory_f_0"], true);
        assert_eq!(set_type["mandatory_m_0"], false);
        let keys: Vec<&String> = set_type.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["type", "paletteId", "mandatory_f_0", "mandatory_f_1", "mandatory_m_0", "mandatory_m_1", "sets"]);
        let first = &set_type["sets"][0];
        assert_eq!(first["sellable"], true);
        assert!(first.get("hiddenLayers").is_none());
        assert_eq!(first["parts"][0].to_string(), r#"{"id":1,"type":"hr","colorable":true,"index":0,"colorindex":1}"#);
        let second = &set_type["sets"][1];
        assert_eq!(second["hiddenLayers"].to_string(), r#"[{"partType":"hr"},{"partType":"hrb"}]"#);

        let disabled = figure_data(&doc, true).unwrap();
        assert_eq!(disabled["setTypes"][0]["sets"][0]["sellable"], false);
    }

    #[test]
    fn figuredata_errors_like_csharp() {
        let doc = Document::parse("<other/>").unwrap();
        assert_eq!(figure_data(&doc, false).unwrap_err().to_string(), NULL_REFERENCE);
        let doc = Document::parse("<figuredata/>").unwrap();
        assert_eq!(figure_data(&doc, false).unwrap().to_string(), r#"{"palettes":null,"setTypes":null}"#);
        let doc = Document::parse(r#"<figuredata><colors><palette id="x"/></colors></figuredata>"#).unwrap();
        assert!(figure_data(&doc, false).is_err());
    }

    #[test]
    fn figuremap_libraries_and_ids() {
        let xml = r#"<map><lib id="hh_human_body" revision="12"><part id="1" type="bd"/><part id="x" type="hd"/></lib><lib id="acc" revision="bad"/><lib revision="1"/></map>"#;
        let doc = Document::parse(xml).unwrap();
        let value = figure_map(&doc);
        assert_eq!(
            value.to_string(),
            r#"{"libraries":[{"id":"hh_human_body","revision":12,"parts":[{"id":1,"type":"bd"},{"id":null,"type":"hd"}]},{"id":"acc","revision":0,"parts":[]},{"id":null,"revision":1,"parts":[]}]}"#
        );
        assert_eq!(figure_map_lib_ids(&doc), ["hh_human_body", "acc"]);
        let text = crate::jsonw::to_string_with_newline(&value, Escaping::Default, "\n");
        assert!(text.starts_with("{\n  \"libraries\": [\n    {\n      \"id\": \"hh_human_body\","));
    }

    #[test]
    fn furnidata_xml_to_json() {
        let xml = r#"<furnidata>
  <roomitemtypes>
    <furnitype id="13" classname="shelves_norja*2">
      <revision>61856</revision>
      <defaultdir>0</defaultdir><xdim>1</xdim><ydim>1</ydim>
      <partcolors><color>#ffffff</color><color>F7EBBC</color><color></color></partcolors>
      <name>Beige Bookcase</name><description>For nic naks &amp; art</description>
      <adurl/><offerid>5</offerid><buyout>1</buyout><rentofferid>-1</rentofferid><rentbuyout>0</rentbuyout>
      <bc>0</bc><excludeddynamic>0</excludeddynamic><customparams/><specialtype>1</specialtype>
      <canstandon>0</canstandon><cansiton>1</cansiton><canlayon>0</canlayon><furniline>iced</furniline>
      <environment/><rare>0</rare>
    </furnitype>
  </roomitemtypes>
  <wallitemtypes>
    <furnitype id="4001" classname="poster">
      <revision>1</revision><name>Café</name><xdim>9</xdim><canstandon>1</canstandon>
    </furnitype>
  </wallitemtypes>
</furnidata>"#;
        let text = furniture_data_json(xml).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let floor = &value["roomitemtypes"]["furnitype"][0];
        assert_eq!(floor["id"], 13);
        assert_eq!(floor["classname"], "shelves_norja*2");
        assert_eq!(floor["revision"], 61856);
        assert_eq!(floor["category"], "unknown");
        assert_eq!(floor["partcolors"]["color"], json!(["#ffffff", "#F7EBBC"]));
        assert_eq!(floor["description"], "For nic naks & art");
        assert_eq!(floor["buyout"], true);
        assert_eq!(floor["rentofferid"], -1);
        assert_eq!(floor["cansiton"], true);
        let keys: Vec<&String> = floor.as_object().unwrap().keys().collect();
        assert_eq!(keys[..5], ["id", "classname", "revision", "category", "defaultdir"]);
        assert_eq!(keys.len(), 25);

        let wall = &value["wallitemtypes"]["furnitype"][0];
        assert_eq!(wall["xdim"], 0, "wall items take no dimensions");
        assert_eq!(wall["canstandon"], false);
        assert_eq!(wall["partcolors"], Value::Null);
        assert!(text.contains("\"name\": \"Café\""), "relaxed encoder keeps non-ASCII");

        assert_eq!(
            furniture_data_json("<x/>").unwrap_err().to_string(),
            "Invalid XML format: missing 'furnidata' root element."
        );
        let empty: Value = serde_json::from_str(&furniture_data_json("<furnidata><roomitemtypes/></furnidata>").unwrap()).unwrap();
        assert_eq!(empty, json!({"roomitemtypes": null, "wallitemtypes": null}));
        assert!(furniture_data_json("<furnidata><roomitemtypes><furnitype><revision>x</revision></furnitype></roomitemtypes></furnidata>").is_err());
    }
}
