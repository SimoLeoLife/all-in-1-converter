//! Port of `CompareProductData.Compare`: merges every Merge/Import_ProductData/*.json into the
//! original ProductData.json (asking before replacing a product code that exists) and writes
//! Merge/Merged_ProductData/ProductData.json sorted by code.

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use habbo_core::json::{save_object, JObject, PRODUCTDATA_FILE};
use habbo_core::{console, out, outln};
use serde_json::Value;

use crate::jtoken::{self, Loaded, NULL_REFERENCE};

/// `CompareProductData.Compare`.
pub async fn compare() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let base_dir = cwd.join("Merge");
    let import_dir = base_dir.join("Import_ProductData");
    let merged_dir = base_dir.join("Merged_ProductData");
    std::fs::create_dir_all(&import_dir)?;
    std::fs::create_dir_all(&merged_dir)?;

    outln!("Where do you want to load the Original ProductData from?");
    outln!("  (D) From the Habbo Default directory (Habbo_Default/files/json/ProductData.json)");
    outln!("  (I) From Original_ProductData/ProductData.json in Merge");
    out!("Select (I) or (D) [default D]: ");
    let choice = console::read_line_opt().map(|s| s.trim().to_uppercase());

    let original_path = if choice.as_deref() == Some("I") {
        base_dir.join("Original_ProductData").join(PRODUCTDATA_FILE)
    } else {
        cwd.join("Habbo_Default").join("files").join("json").join("ProductData.json")
    };

    let mut original_json = match jtoken::load(&original_path)? {
        Loaded::Ok(json) => json,
        Loaded::NotFound(message) => {
            outln!("Original ProductData not found: {message}");
            return Ok(());
        }
    };
    let n = original_json
        .get("productdata")
        .and_then(|p| p.as_object())
        .and_then(|p| p.get("product"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    outln!("Loaded ProductData.json - {n} products");

    let result = (|| -> Result<()> {
        let mut total_imported = 0;
        let import_entries = jtoken::json_files(&import_dir, "");
        if import_entries.is_empty() {
            outln!("No import entries found in Import_ProductData/.");
            outln!("The original JSON will be written unchanged.");
        } else {
            let mut replace_all = false;
            let mut skip_all = false;

            for entry in &import_entries {
                outln!("Processing: {}", habbo_core::paths::file_name(entry));
                let import_json = habbo_core::json::load_object(entry)?;
                let imported_count = merge_json(&mut original_json, &import_json, "productdata", &mut replace_all, &mut skip_all)?;
                total_imported += imported_count;
                outln!("  + {imported_count} items merged");
            }

            sort_json_by_code(&mut original_json, "productdata")?;
        }

        let merged_file_path = merged_dir.join(PRODUCTDATA_FILE);
        save_object(&original_json, &merged_file_path)?;
        outln!("ProductData saved to {}", merged_file_path.display());
        outln!("Total Products imported: {total_imported}");
        Ok(())
    })();
    if let Err(error) = result {
        outln!("Error merging ProductData: {error}");
    }
    Ok(())
}

/// `(JArray)json[itemType]["product"]`.
fn products_mut<'a>(json: &'a mut JObject, item_type: &str) -> Result<&'a mut Vec<Value>> {
    let section = json.get_mut(item_type).ok_or_else(|| anyhow!(NULL_REFERENCE))?;
    let products = section.as_object_mut().and_then(|s| s.get_mut("product")).ok_or_else(|| anyhow!(NULL_REFERENCE))?;
    products.as_array_mut().ok_or_else(|| anyhow!("Unable to cast object to type 'Newtonsoft.Json.Linq.JArray'."))
}

pub(crate) fn merge_json(
    original_json: &mut JObject,
    import_json: &JObject,
    item_type: &str,
    replace_all: &mut bool,
    skip_all: &mut bool,
) -> Result<usize> {
    let original_array = products_mut(original_json, item_type)?;
    // Grouped by code (first occurrence wins), so duplicate codes in the original are fine.
    let mut original_items: HashSet<String> = HashSet::new();
    for item in original_array.iter() {
        original_items.insert(jtoken::key_string(item, "code")?);
    }

    let import_products = jtoken::req(import_json.get(item_type), "product")?;
    let import_products: Vec<Value> = match import_products {
        Value::Array(items) => items.clone(),
        // Enumerating a JObject gives its properties; none of them have a "code".
        Value::Object(_) => return Err(anyhow!("Cannot access child value on Newtonsoft.Json.Linq.JProperty.")),
        _ => return Err(anyhow!("Cannot access child value on Newtonsoft.Json.Linq.JValue.")),
    };

    // Tracks the codes already seen in this import file.
    let mut processed_keys: HashSet<String> = HashSet::new();
    let mut imported_count = 0;

    for import_item in import_products {
        let import_key = jtoken::key_string(&import_item, "code")?;

        // Skip duplicates within the imported JSON.
        if !processed_keys.insert(import_key.clone()) {
            continue;
        }

        if original_items.contains(&import_key) {
            if *skip_all {
                continue;
            } else if !*replace_all {
                outln!("Would you like to replace the following product?");
                outln!("Code: {import_key}");
                outln!("Enter (Y) for Yes, (A) for Yes to all, (N) for No, or (Z) for No to all:");
                let response = console::read_line_opt().map(|s| s.to_uppercase());

                match response.as_deref() {
                    Some("Y") => {}
                    Some("A") => *replace_all = true,
                    Some("N") => continue,
                    Some("Z") => {
                        *skip_all = true;
                        continue;
                    }
                    _ => {
                        outln!("Invalid input. Skipping this entry.");
                        continue;
                    }
                }
            }

            // Replace the original item with the imported item.
            let mut duplicate_index = None;
            for (index, item) in original_array.iter().enumerate() {
                if jtoken::key_string(item, "code")? == import_key {
                    duplicate_index = Some(index);
                    break;
                }
            }
            let index = duplicate_index.ok_or_else(|| anyhow!("Sequence contains no matching element"))?;
            original_array[index] = import_item;
            imported_count += 1;
        } else {
            // Add the new item to the original JSON.
            original_array.push(import_item);
            imported_count += 1;
        }
    }

    Ok(imported_count)
}

/// Stable sort by `code` (as string; a null code first).
pub(crate) fn sort_json_by_code(json: &mut JObject, item_type: &str) -> Result<()> {
    let section = json.get_mut(item_type).ok_or_else(|| anyhow!(NULL_REFERENCE))?;
    let Some(array) = section.as_object_mut().and_then(|s| s.get_mut("product")).and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut keyed: Vec<(Option<String>, Value)> = Vec::with_capacity(array.len());
    for item in array.drain(..) {
        let key = jtoken::value_string(jtoken::req(Some(&item), "code")?)?;
        keyed.push((key, item));
    }
    keyed.sort_by(|(a, _), (b, _)| match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => jtoken::culture_compare(a, b),
    });
    array.extend(keyed.into_iter().map(|(_, item)| item));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    struct Answers(std::sync::Mutex<Vec<String>>);
    impl habbo_core::console::Source for Answers {
        fn read_line(&self) -> Option<String> {
            let mut answers = self.0.lock().unwrap();
            if answers.is_empty() {
                None
            } else {
                Some(answers.remove(0))
            }
        }
    }

    fn obj(value: Value) -> JObject {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn adds_new_codes_asks_before_replacing_and_sorts() {
        let previous = habbo_core::console::set_source(Some(Arc::new(Answers(std::sync::Mutex::new(vec!["n".into(), "y".into()])))));

        let mut original = obj(json!({"productdata": {"product": [
            {"code": "b", "name": "old b"}, {"code": "a", "name": "old a"}, {"code": "a", "name": "dup a"}
        ]}}));
        let import = obj(json!({"productdata": {"product": [
            {"code": "a", "name": "new a"},
            {"code": "b", "name": "new b"},
            {"code": "c", "name": "c"},
            {"code": "c", "name": "c again"}
        ]}}));
        let (mut replace_all, mut skip_all) = (false, false);
        let count = merge_json(&mut original, &import, "productdata", &mut replace_all, &mut skip_all).unwrap();
        habbo_core::console::set_source(previous);

        assert_eq!(count, 2); // "a" refused, "b" replaced, "c" added once
        sort_json_by_code(&mut original, "productdata").unwrap();
        assert_eq!(
            original["productdata"]["product"],
            json!([{"code": "a", "name": "old a"}, {"code": "a", "name": "dup a"}, {"code": "b", "name": "new b"}, {"code": "c", "name": "c"}])
        );

        // Skip-all: nothing replaced, no question asked.
        let (mut replace_all, mut skip_all) = (false, true);
        let again = obj(json!({"productdata": {"product": [{"code": "a"}]}}));
        assert_eq!(merge_json(&mut original, &again, "productdata", &mut replace_all, &mut skip_all).unwrap(), 0);
        // Replace-all: replaced without a question.
        let (mut replace_all, mut skip_all) = (true, false);
        assert_eq!(merge_json(&mut original, &again, "productdata", &mut replace_all, &mut skip_all).unwrap(), 1);
        assert_eq!(original["productdata"]["product"][0], json!({"code": "a"}));
    }
}
