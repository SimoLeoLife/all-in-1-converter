//! Port of `CompareFurnidata.Compare`: adds the furni of every Merge/Import_Furnidata/*.json to the
//! original FurnitureData.json, skipping any whose classname or id is already taken, and writes
//! Merge/Merged_Furnidata/FurnitureData.json.

use std::collections::HashSet;

use anyhow::Result;
use habbo_core::json::{save_object, JObject, FURNIDATA_FILE};
use habbo_core::{console, out, outln};
use serde_json::Value;

use crate::jtoken::{self, Loaded};

/// `CompareFurnidata.Compare`.
pub async fn compare() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let base_dir = cwd.join("Merge");
    let original_dir = base_dir.join("Original_Furnidata");
    let import_dir = base_dir.join("Import_Furnidata");
    let merged_dir = base_dir.join("Merged_Furnidata");

    std::fs::create_dir_all(&original_dir)?;
    std::fs::create_dir_all(&import_dir)?;
    std::fs::create_dir_all(&merged_dir)?;

    outln!("Where do you want to load the Original Furnidata from?");
    outln!("  (D) From the Habbo Default directory (Habbo_Default/files/json/FurnitureData.json)");
    outln!("  (I) From Original_Furnidata/FurnitureData.json in Merge");
    out!("Select (I) or (D) [default D]: ");
    let user_selection = console::read_line_opt();

    let original_path = if user_selection.is_some_and(|s| s.eq_ignore_ascii_case("I")) {
        original_dir.join(FURNIDATA_FILE)
    } else {
        cwd.join("Habbo_Default").join("files").join("json").join("FurnitureData.json")
    };

    let mut original_json = match jtoken::load(&original_path)? {
        Loaded::Ok(json) => json,
        Loaded::NotFound(message) => {
            outln!("Original FurnitureData not found: {message}");
            return Ok(());
        }
    };
    outln!(
        "Loaded FurnitureData.json - floor={}, wall={}",
        furnitype_count(&original_json, "roomitemtypes"),
        furnitype_count(&original_json, "wallitemtypes")
    );

    let result = (|| -> Result<()> {
        let mut total_imported = 0;
        let import_entries = jtoken::json_files(&import_dir, "");
        if import_entries.is_empty() {
            outln!("No import entries found in Import_Furnidata/.");
            outln!("The original JSON will be written unchanged.");
        } else {
            for entry in &import_entries {
                outln!("Processing: {}", habbo_core::paths::file_name(entry));
                let import_json = habbo_core::json::load_object(entry)?;
                let mut imported_count = merge_json(&mut original_json, &import_json, "roomitemtypes")?;
                imported_count += merge_json(&mut original_json, &import_json, "wallitemtypes")?;
                total_imported += imported_count;
                outln!("  + {imported_count} items merged");
            }

            sort_json_by_id(&mut original_json, "roomitemtypes")?;
            sort_json_by_id(&mut original_json, "wallitemtypes")?;
        }

        let merged_file_path = merged_dir.join(FURNIDATA_FILE);
        save_object(&original_json, &merged_file_path)?;
        outln!("Furnidata saved to {}", merged_file_path.display());
        outln!("Total Furniture imported: {total_imported}");
        Ok(())
    })();
    if let Err(error) = result {
        outln!("Error merging furnidata: {error}");
    }
    Ok(())
}

fn furnitype_count(json: &JObject, item_type: &str) -> usize {
    furnitype(json, item_type).map(Vec::len).unwrap_or(0)
}

/// `json[itemType]?["furnitype"] as JArray`.
fn furnitype<'a>(json: &'a JObject, item_type: &str) -> Option<&'a Vec<Value>> {
    json.get(item_type).and_then(|t| t.as_object()).and_then(|o| o.get("furnitype")).and_then(Value::as_array)
}

fn furnitype_mut<'a>(json: &'a mut JObject, item_type: &str) -> Option<&'a mut Vec<Value>> {
    json.get_mut(item_type).and_then(|t| t.as_object_mut()).and_then(|o| o.get_mut("furnitype")).and_then(Value::as_array_mut)
}

/// Original additive merge: skip duplicates by classname OR by id.
pub(crate) fn merge_json(original_json: &mut JObject, import_json: &JObject, item_type: &str) -> Result<usize> {
    let Some(import_array) = furnitype(import_json, item_type) else { return Ok(0) };
    let Some(original_array) = furnitype_mut(original_json, item_type) else { return Ok(0) };

    // ToDictionary: a duplicate classname or id in the original throws.
    let mut original_by_class: HashSet<String> = HashSet::new();
    for item in original_array.iter() {
        if let Some(classname) = jtoken::as_jobject(item)?.get("classname") {
            let classname = jtoken::to_string(classname);
            if !original_by_class.insert(classname.clone()) {
                return Err(jtoken::duplicate_key(&classname));
            }
        }
    }
    let mut original_by_id: HashSet<i32> = HashSet::new();
    for item in original_array.iter() {
        if let Some(id) = jtoken::as_jobject(item)?.get("id") {
            let id = jtoken::value_int(id)?;
            if !original_by_id.insert(id) {
                return Err(jtoken::duplicate_key(&id.to_string()));
            }
        }
    }

    let mut processed_classnames: HashSet<String> = HashSet::new();
    let mut processed_ids: HashSet<i32> = HashSet::new();
    let mut imported_count = 0;

    for import_item in import_array {
        let import_object = jtoken::as_jobject(import_item)?;
        let (Some(classname), Some(id_token)) = (import_object.get("classname"), import_object.get("id")) else { continue };
        let classname = jtoken::to_string(classname);
        let id = jtoken::value_int(id_token)?;

        if original_by_class.contains(&classname)
            || processed_classnames.contains(&classname)
            || original_by_id.contains(&id)
            || processed_ids.contains(&id)
        {
            continue;
        }

        original_array.push(import_item.clone());
        processed_classnames.insert(classname);
        processed_ids.insert(id);
        imported_count += 1;
    }
    Ok(imported_count)
}

/// Stable sort by id; a furni without an id goes last.
pub(crate) fn sort_json_by_id(json: &mut JObject, item_type: &str) -> Result<()> {
    let Some(array) = furnitype_mut(json, item_type) else { return Ok(()) };
    let mut keyed: Vec<(i32, Value)> = Vec::with_capacity(array.len());
    for item in array.drain(..) {
        let key = match jtoken::index(Some(&item), "id") {
            Some(id) => jtoken::value_int(id)?,
            None => i32::MAX,
        };
        keyed.push((key, item));
    }
    keyed.sort_by_key(|(key, _)| *key);
    array.extend(keyed.into_iter().map(|(_, item)| item));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> JObject {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn skips_taken_classnames_and_ids_then_sorts() {
        let mut original = obj(json!({
            "roomitemtypes": {"furnitype": [{"id": 5, "classname": "chair"}, {"id": 1, "classname": "table"}]},
            "wallitemtypes": {"furnitype": []}
        }));
        let import = obj(json!({
            "roomitemtypes": {"furnitype": [
                {"id": 9, "classname": "chair"},
                {"id": 1, "classname": "sofa"},
                {"id": 3, "classname": "lamp"},
                {"id": 3, "classname": "lamp2"},
                {"id": 4, "classname": "lamp"},
                {"classname": "noid"}
            ]},
            "wallitemtypes": {"furnitype": [{"id": "7", "classname": "poster"}]}
        }));

        assert_eq!(merge_json(&mut original, &import, "roomitemtypes").unwrap(), 1);
        assert_eq!(merge_json(&mut original, &import, "wallitemtypes").unwrap(), 1);
        sort_json_by_id(&mut original, "roomitemtypes").unwrap();
        let ids: Vec<i64> = original["roomitemtypes"]["furnitype"].as_array().unwrap().iter().map(|f| f["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, [1, 3, 5]);
        assert_eq!(original["wallitemtypes"]["furnitype"][0]["classname"], "poster");
    }

    #[test]
    fn duplicate_in_original_errors_and_missing_sections_merge_nothing() {
        let mut original = obj(json!({"roomitemtypes": {"furnitype": [{"id": 1, "classname": "a"}, {"id": 2, "classname": "a"}]}}));
        let import = obj(json!({"roomitemtypes": {"furnitype": [{"id": 3, "classname": "b"}]}}));
        let error = merge_json(&mut original, &import, "roomitemtypes").unwrap_err();
        assert_eq!(error.to_string(), "An item with the same key has already been added. Key: a");
        assert_eq!(merge_json(&mut original, &import, "wallitemtypes").unwrap(), 0);

        let mut sorted = obj(json!({"roomitemtypes": {"furnitype": [{"classname": "x"}, {"id": 2}, {"id": 1}]}}));
        sort_json_by_id(&mut sorted, "roomitemtypes").unwrap();
        assert_eq!(sorted["roomitemtypes"]["furnitype"], json!([{"id": 1}, {"id": 2}, {"classname": "x"}]));
    }
}
