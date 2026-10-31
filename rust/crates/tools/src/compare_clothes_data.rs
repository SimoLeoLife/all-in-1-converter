//! Port of `CompareClothesData.Compare`: merges Merge/Import_ClothesData/FigureData*.json and
//! FigureMap*.json into the originals in Merge/Original_ClothesData and writes both to
//! Merge/Merged_ClothesData.

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use habbo_core::json::{save_object, JObject, FIGUREDATA_FILE, FIGUREMAP_FILE};
use habbo_core::outln;
use serde_json::Value;

use crate::jtoken::{self, Loaded, NULL_REFERENCE};

/// `CompareClothesData.Compare`.
pub async fn compare() -> Result<()> {
    let base_dir = std::env::current_dir()?.join("Merge");
    let original_dir = base_dir.join("Original_ClothesData");
    let import_dir = base_dir.join("Import_ClothesData");
    let merged_dir = base_dir.join("Merged_ClothesData");

    std::fs::create_dir_all(&original_dir)?;
    std::fs::create_dir_all(&import_dir)?;
    std::fs::create_dir_all(&merged_dir)?;

    let figure_data_path = original_dir.join(FIGUREDATA_FILE);
    let figure_map_path = original_dir.join(FIGUREMAP_FILE);
    let mut original_figure_data = match jtoken::load(&figure_data_path)? {
        Loaded::Ok(json) => json,
        Loaded::NotFound(message) => {
            outln!("Missing original clothes input: {message}");
            return Ok(());
        }
    };
    let mut original_figure_map = match jtoken::load(&figure_map_path)? {
        Loaded::Ok(json) => json,
        Loaded::NotFound(message) => {
            outln!("Missing original clothes input: {message}");
            return Ok(());
        }
    };

    let result = (|| -> Result<()> {
        let mut total_imported = 0;

        let figure_data_entries = jtoken::json_files(&import_dir, "FigureData");
        let figure_map_entries = jtoken::json_files(&import_dir, "FigureMap");

        if figure_data_entries.is_empty() && figure_map_entries.is_empty() {
            outln!("No FigureData* or FigureMap* import entries found in Import_ClothesData/.");
            outln!("The original JSON files will be written unchanged.");
        }

        for entry in &figure_data_entries {
            outln!("Processing FigureData entry: {}", habbo_core::paths::file_name(entry));
            let import_json = habbo_core::json::load_object(entry)?;
            let n = merge_figure_data(&mut original_figure_data, &import_json)?;
            total_imported += n;
            outln!("  + {n} merged into FigureData");
        }
        for entry in &figure_map_entries {
            outln!("Processing FigureMap entry: {}", habbo_core::paths::file_name(entry));
            let import_json = habbo_core::json::load_object(entry)?;
            let n = merge_figure_map(&mut original_figure_map, &import_json)?;
            total_imported += n;
            outln!("  + {n} merged into FigureMap");
        }

        save_object(&original_figure_data, merged_dir.join(FIGUREDATA_FILE))?;
        save_object(&original_figure_map, merged_dir.join(FIGUREMAP_FILE))?;
        outln!("Clothes merged -> {}", merged_dir.display());
        outln!("Total items imported: {total_imported}");
        Ok(())
    })();
    if let Err(error) = result {
        outln!("Error merging clothes data: {error}");
    }
    Ok(())
}

/// `json[key] as JArray` on the original, which must exist when the import has the key.
fn original_array<'a>(json: &'a mut JObject, key: &str) -> Result<&'a mut Vec<Value>> {
    json.get_mut(key).and_then(Value::as_array_mut).ok_or_else(|| anyhow!("Value cannot be null. (Parameter 'source')"))
}

/// `import[key] as JArray`, iterated (null when it is not an array).
fn import_array<'a>(json: &'a JObject, key: &str) -> Result<&'a Vec<Value>> {
    json.get(key).and_then(Value::as_array).ok_or_else(|| anyhow!(NULL_REFERENCE))
}

/// Adds the children of `import_children` whose `id` is not in `original` (by `ToString()`).
/// `original: None` is the C# `?? new JArray()` stand-in: additions are counted (and seen by
/// the next ids) but land in a throwaway array.
fn merge_children(original: Option<&mut Vec<Value>>, import_children: Option<&Value>) -> Result<usize> {
    let mut scratch = Vec::new();
    let target = original.unwrap_or(&mut scratch);
    let empty = Vec::new();
    let import_children = import_children.and_then(Value::as_array).unwrap_or(&empty);
    let mut count = 0;
    for import_child in import_children {
        let import_id = jtoken::key_string(import_child, "id")?;
        let mut exists = false;
        for child in target.iter() {
            if jtoken::key_string(child, "id")? == import_id {
                exists = true;
                break;
            }
        }
        if !exists {
            target.push(import_child.clone());
            count += 1;
        }
    }
    Ok(count)
}

/// Palettes by `id` (new colours by `id`) and set types by `type` (new sets by `id`).
pub(crate) fn merge_figure_data(original_json: &mut JObject, import_json: &JObject) -> Result<usize> {
    let mut imported_count = 0;

    for (section, key, children) in [("palettes", "id", "colors"), ("setTypes", "type", "sets")] {
        if !import_json.contains_key(section) {
            continue;
        }
        let import_items = import_array(import_json, section)?;
        for import_item in import_items {
            let import_key = jtoken::key_string(import_item, key)?;
            let originals = original_array(original_json, section)?;

            let mut existing = None;
            for (index, item) in originals.iter().enumerate() {
                if jtoken::key_string(item, key)? == import_key {
                    existing = Some(index);
                    break;
                }
            }

            match existing {
                Some(index) => {
                    let target = originals[index].as_object_mut().and_then(|o| o.get_mut(children)).and_then(Value::as_array_mut);
                    imported_count += merge_children(target, jtoken::index(Some(import_item), children))?;
                }
                None => {
                    originals.push(import_item.clone());
                    imported_count += 1;
                }
            }
        }
    }

    Ok(imported_count)
}

/// Libraries by `id`; a library already in the original is left alone.
pub(crate) fn merge_figure_map(original_json: &mut JObject, import_json: &JObject) -> Result<usize> {
    let mut imported_count = 0;

    // ToDictionary: a duplicate id in the original throws.
    let mut original_libraries: HashSet<String> = HashSet::new();
    for library in original_json.get("libraries").and_then(Value::as_array).ok_or_else(|| anyhow!(NULL_REFERENCE))? {
        let id = jtoken::key_string(library, "id")?;
        if !original_libraries.insert(id.clone()) {
            return Err(jtoken::duplicate_key(&id));
        }
    }
    let mut processed_library_ids: HashSet<String> = HashSet::new();

    for import_library in import_array(import_json, "libraries")? {
        let library_id = jtoken::key_string(import_library, "id")?;

        if !processed_library_ids.insert(library_id.clone()) {
            continue;
        }

        if !original_libraries.contains(&library_id) {
            original_array(original_json, "libraries")?.push(import_library.clone());
            imported_count += 1;
        }
    }

    Ok(imported_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> JObject {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn figure_data_merges_palettes_and_set_types() {
        let mut original = obj(json!({
            "palettes": [{"id": 1, "colors": [{"id": 10}]}, {"id": 2}],
            "setTypes": [{"type": "hr", "sets": [{"id": 100}]}]
        }));
        let import = obj(json!({
            "palettes": [
                {"id": 1, "colors": [{"id": 10}, {"id": 11}, {"id": "11"}]},
                {"id": 2, "colors": [{"id": 20}]},
                {"id": 3, "colors": []}
            ],
            "setTypes": [{"type": "hr", "sets": [{"id": 100}, {"id": 101}]}, {"type": "ch", "sets": []}]
        }));
        // 11 (once: "11" matches it by ToString), 20 (throwaway array), palette 3, set 101, setType ch.
        assert_eq!(merge_figure_data(&mut original, &import).unwrap(), 5);
        assert_eq!(original["palettes"][0]["colors"], json!([{"id": 10}, {"id": 11}]));
        assert_eq!(original["palettes"][1], json!({"id": 2}));
        assert_eq!(original["palettes"][2]["id"], 3);
        assert_eq!(original["setTypes"][0]["sets"], json!([{"id": 100}, {"id": 101}]));
        assert_eq!(original["setTypes"][1]["type"], "ch");

        // An import without the sections merges nothing.
        assert_eq!(merge_figure_data(&mut original, &obj(json!({}))).unwrap(), 0);
    }

    #[test]
    fn figure_map_adds_new_libraries_once() {
        let mut original = obj(json!({"libraries": [{"id": "hh_human_body"}]}));
        let import = obj(json!({"libraries": [{"id": "hh_human_body", "x": 1}, {"id": "shirt_u"}, {"id": "shirt_u", "x": 2}]}));
        assert_eq!(merge_figure_map(&mut original, &import).unwrap(), 1);
        assert_eq!(original["libraries"], json!([{"id": "hh_human_body"}, {"id": "shirt_u"}]));

        let mut duplicated = obj(json!({"libraries": [{"id": "a"}, {"id": "a"}]}));
        assert!(merge_figure_map(&mut duplicated, &import).is_err());
    }
}
