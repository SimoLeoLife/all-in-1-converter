//! `FixSpriteIDSettings`: items_base.sprite_id from FurnitureData.json (by classname), then
//! catalog_items.item_ids from items_base (wallpaper / floor / landscape singles and public_name).

use std::collections::HashMap;

use anyhow::{bail, Result};
use habbo_core::outln;
use mysql_async::prelude::Queryable;
use mysql_async::Row;
use serde::Deserialize;

use crate::fix_item_settings::{furniture_data_path, read_all_text};
use crate::progress::{partition, CONSOLE_LOCK};
use crate::rows;

#[derive(Deserialize)]
struct FixFurnitureData {
    #[serde(default)]
    roomitemtypes: Option<FixItemType>,
    #[serde(default)]
    wallitemtypes: Option<FixItemType>,
}

#[derive(Deserialize)]
struct FixItemType {
    #[serde(default)]
    furnitype: Option<Vec<FixItemId>>,
}

#[derive(Deserialize)]
struct FixItemId {
    #[serde(default)]
    id: i32,
    #[serde(default)]
    classname: Option<String>,
}

fn say(text: &str) {
    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    habbo_core::console::writeln(text);
}

/// `FixSpriteIDSettings.RunAsync`.
pub async fn run() -> Result<()> {
    let json_file_path = furniture_data_path();
    if !json_file_path.is_file() {
        say(&format!("⚠️ FurnitureData.json not found: {}", json_file_path.display()));
        return Ok(());
    }

    let json_content = match read_all_text(&json_file_path) {
        Ok(text) => text,
        Err(error) => {
            say(&format!("❌ Error reading JSON file: {error}"));
            return Ok(());
        }
    };

    let data: Option<FixFurnitureData> = match serde_json::from_str(&json_content) {
        Ok(data) => data,
        Err(error) => {
            say(&format!("❌ Error deserializing JSON: {error}"));
            return Ok(());
        }
    };

    let Some(data) = data.filter(|d| {
        d.roomitemtypes.as_ref().is_some_and(|t| t.furnitype.is_some()) || d.wallitemtypes.as_ref().is_some_and(|t| t.furnitype.is_some())
    }) else {
        say("❌ No valid furniture items found in JSON.");
        return Ok(());
    };

    let furniture_data = classname_map(data)?;
    outln!("✅ Loaded {} items from JSON.", furniture_data.len());

    update_database_sprite_ids(&furniture_data).await;
    Ok(())
}

/// `furnitureData[item.classname] = item.id` over room then wall items (the last one wins).
fn classname_map(data: FixFurnitureData) -> Result<HashMap<String, i32>> {
    let mut map = HashMap::new();
    for items in [data.roomitemtypes, data.wallitemtypes].into_iter().flatten().filter_map(|t| t.furnitype) {
        for item in items {
            let Some(classname) = item.classname else {
                bail!("Value cannot be null. (Parameter 'key')");
            };
            map.insert(classname, item.id);
        }
    }
    Ok(map)
}

struct DatabaseItem {
    id: i32,
    item_name: String,
    current_sprite_id: i32,
    public_name: String,
}

struct CatalogItem {
    catalog_id: i32,
    catalog_name: String,
    item_ids: String,
}

async fn fetch() -> Result<(Vec<DatabaseItem>, Vec<CatalogItem>)> {
    let mut connection = crate::config::connect().await?;
    let mut database_items = Vec::new();
    let result: Vec<Row> = connection.query("SELECT id, item_name, sprite_id, public_name FROM items_base").await?;
    for row in &result {
        database_items.push(DatabaseItem {
            id: rows::get_i32(row, "id")?,
            item_name: rows::get_string(row, "item_name")?,
            current_sprite_id: rows::get_i32(row, "sprite_id")?,
            public_name: rows::get_string(row, "public_name")?,
        });
    }

    let mut catalog_items = Vec::new();
    let result: Vec<Row> = connection.query("SELECT id, catalog_name, item_ids FROM catalog_items").await?;
    for row in &result {
        catalog_items.push(CatalogItem {
            catalog_id: rows::get_i32(row, "id")?,
            catalog_name: rows::get_string(row, "catalog_name")?,
            item_ids: rows::get_string(row, "item_ids")?,
        });
    }
    let _ = connection.disconnect().await;
    Ok((database_items, catalog_items))
}

/// The items_base id of the special item, else the item_ids of its catalog_items row.
fn special_item_id(database_items: &[DatabaseItem], catalog_items: &[CatalogItem], name: &str) -> Option<String> {
    // FirstOrDefault on the tuple list: the first match, and only when its id is > 0.
    match database_items.iter().find(|i| i.item_name == name) {
        Some(item) if item.id > 0 => return Some(item.id.to_string()),
        _ => {}
    }
    match catalog_items.iter().find(|c| c.catalog_name == name) {
        Some(c) if c.catalog_id > 0 => Some(c.item_ids.clone()),
        _ => {
            outln!("⚠️ No '{name}' item found in items_base or catalog_items. Skipping {name}_single updates.");
            None
        }
    }
}

/// (catalog id, new item_ids, current item_ids) of every catalog_items row that changes.
fn catalog_updates(
    database_items: &[DatabaseItem],
    catalog_items: &[CatalogItem],
    wallpaper: Option<&str>,
    floor: Option<&str>,
    landscape: Option<&str>,
) -> Vec<(i32, String, String)> {
    let mut first_by_public_name: HashMap<&str, i32> = HashMap::new();
    for item in database_items {
        first_by_public_name.entry(item.public_name.as_str()).or_insert(item.id);
    }

    catalog_items
        .iter()
        .filter_map(|catalog_item| {
            let name = catalog_item.catalog_name.as_str();
            let new_item_ids = if let (true, Some(id)) = (name.starts_with("wallpaper_single"), wallpaper) {
                Some(id.to_string())
            } else if let (true, Some(id)) = (name.starts_with("floor_single"), floor) {
                Some(id.to_string())
            } else if let (true, Some(id)) = (name.starts_with("landscape_single"), landscape) {
                Some(id.to_string())
            } else {
                first_by_public_name.get(name).filter(|id| **id > 0).map(|id| id.to_string())
            };
            new_item_ids
                .filter(|new| *new != catalog_item.item_ids)
                .map(|new| (catalog_item.catalog_id, new, catalog_item.item_ids.clone()))
        })
        .collect()
}

async fn update_database_sprite_ids(furniture_data: &HashMap<String, i32>) {
    let (database_items, catalog_items) = match fetch().await {
        Ok(fetched) => fetched,
        Err(error) => {
            say(&format!("❌ Error fetching items from database: {error}"));
            return;
        }
    };

    if database_items.is_empty() || catalog_items.is_empty() {
        say(if database_items.is_empty() { "❌ No items found in items_base." } else { "❌ No items found in catalog_items." });
        return;
    }

    outln!("✅ Loaded {} items from items_base.", database_items.len());
    outln!("✅ Loaded {} items from catalog_items.", catalog_items.len());

    let items_to_update: Vec<(i32, i32)> = database_items
        .iter()
        .filter_map(|item| {
            furniture_data
                .get(&item.item_name)
                .filter(|new_sprite_id| **new_sprite_id != item.current_sprite_id)
                .map(|new_sprite_id| (item.id, *new_sprite_id))
        })
        .collect();

    if !items_to_update.is_empty() {
        outln!("🔄 {} items need sprite_id updates.", items_to_update.len());
        update_sprite_ids(&items_to_update).await;
    } else {
        outln!("✅ No sprite_id updates needed.");
    }

    let wallpaper = special_item_id(&database_items, &catalog_items, "wallpaper");
    let floor = special_item_id(&database_items, &catalog_items, "floor");
    let landscape = special_item_id(&database_items, &catalog_items, "landscape");

    outln!("🔍 Checking catalog_items to update:");
    let updates = catalog_updates(&database_items, &catalog_items, wallpaper.as_deref(), floor.as_deref(), landscape.as_deref());
    for (catalog_id, new_item_ids, current_item_ids) in &updates {
        outln!("🔍 Catalog ID: {catalog_id}, Current item_ids: {current_item_ids}, New item_ids: {new_item_ids}");
    }

    if !updates.is_empty() {
        outln!("🔄 {} catalog items need item_ids updates.", updates.len());
        let pairs: Vec<(i32, String)> = updates.into_iter().map(|(id, new, _)| (id, new)).collect();
        update_catalog_item_ids(&pairs).await;
    } else {
        outln!("✅ No item_ids updates needed in catalog_items.");
    }
}

pub(crate) fn sprite_id_batch_query(batch: &[(i32, i32)]) -> String {
    let mut case = String::from("CASE id ");
    for (id, new_sprite_id) in batch {
        case.push_str(&format!("WHEN {id} THEN {new_sprite_id} "));
    }
    case.push_str("ELSE sprite_id END");
    let ids: Vec<String> = batch.iter().map(|(id, _)| id.to_string()).collect();
    format!("UPDATE items_base SET sprite_id = {case} WHERE id IN ({})", ids.join(","))
}

pub(crate) fn item_ids_batch_query(batch: &[(i32, String)]) -> String {
    let mut case = String::from("CASE id ");
    for (catalog_id, new_item_ids) in batch {
        case.push_str(&format!("WHEN {catalog_id} THEN '{new_item_ids}' "));
    }
    case.push_str("ELSE item_ids END");
    let ids: Vec<String> = batch.iter().map(|(id, _)| id.to_string()).collect();
    format!("UPDATE catalog_items SET item_ids = {case} WHERE id IN ({})", ids.join(","))
}

async fn update_sprite_ids(items_to_update: &[(i32, i32)]) {
    let result: Result<()> = async {
        let mut connection = crate::config::connect().await?;
        let batch_size = 100;
        let total_batches = items_to_update.len().div_ceil(batch_size);
        let mut processed_batches = 0;

        outln!("🔄 Updating {} sprite_ids in {} batches...", items_to_update.len(), total_batches);

        for batch in partition(items_to_update, batch_size) {
            if let Err(error) = connection.query_drop(sprite_id_batch_query(&batch)).await {
                outln!("❌ Error updating batch: {error}");
            }
            processed_batches += 1;
            outln!("✅ {processed_batches}/{total_batches} batches processed.");
        }

        outln!("✅ sprite_id updates completed.");
        let _ = connection.disconnect().await;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        outln!("❌ Error updating sprite_ids: {error}");
    }
}

async fn update_catalog_item_ids(catalog_items_to_update: &[(i32, String)]) {
    let result: Result<()> = async {
        let mut connection = crate::config::connect().await?;
        let batch_size = 100;
        let total_batches = catalog_items_to_update.len().div_ceil(batch_size);
        let mut processed_batches = 0;

        outln!("🔄 Updating {} item_ids in {} batches...", catalog_items_to_update.len(), total_batches);

        for batch in partition(catalog_items_to_update, batch_size) {
            if let Err(error) = connection.query_drop(item_ids_batch_query(&batch)).await {
                outln!("❌ Error updating batch: {error}");
            }
            processed_batches += 1;
            outln!("✅ {processed_batches}/{total_batches} batches processed.");
        }

        outln!("✅ item_ids updates in catalog_items completed.");
        let _ = connection.disconnect().await;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        outln!("❌ Error updating item_ids in catalog_items: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(id: i32, name: &str, public: &str) -> DatabaseItem {
        DatabaseItem { id, item_name: name.into(), current_sprite_id: 0, public_name: public.into() }
    }

    fn cat(id: i32, name: &str, ids: &str) -> CatalogItem {
        CatalogItem { catalog_id: id, catalog_name: name.into(), item_ids: ids.into() }
    }

    #[test]
    fn catalog_mapping() {
        let items = vec![db(5, "wallpaper", "wallpaper"), db(7, "chair", "Chair"), db(8, "chair2", "Chair")];
        let catalog = vec![cat(1, "wallpaper_single_1", "3"), cat(2, "Chair", "7"), cat(3, "Chair", "1"), cat(4, "floor_single", "9")];
        let wallpaper = special_item_id(&items, &catalog, "wallpaper");
        assert_eq!(wallpaper.as_deref(), Some("5"));
        let updates = catalog_updates(&items, &catalog, wallpaper.as_deref(), None, None);
        assert_eq!(updates, vec![(1, "5".to_string(), "3".to_string()), (3, "7".to_string(), "1".to_string())]);
    }

    #[test]
    fn batch_queries() {
        assert_eq!(sprite_id_batch_query(&[(1, 10), (2, 20)]), "UPDATE items_base SET sprite_id = CASE id WHEN 1 THEN 10 WHEN 2 THEN 20 ELSE sprite_id END WHERE id IN (1,2)");
        assert_eq!(item_ids_batch_query(&[(3, "4".into())]), "UPDATE catalog_items SET item_ids = CASE id WHEN 3 THEN '4' ELSE item_ids END WHERE id IN (3)");
    }

    #[test]
    fn classnames_last_wins() {
        let data: FixFurnitureData = serde_json::from_str(r#"{"roomitemtypes":{"furnitype":[{"id":1,"classname":"a"}]},"wallitemtypes":{"furnitype":[{"id":2,"classname":"a"}]}}"#).unwrap();
        assert_eq!(classname_map(data).unwrap()["a"], 2);
    }
}
