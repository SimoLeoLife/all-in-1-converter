//! `FixItemSettings`: allow_walk / allow_sit / allow_lay (and the neutral wall-item size) in
//! items_base from FurnitureData.json, matched by sprite_id.

use std::collections::HashMap;

use anyhow::Result;
use habbo_core::outln;
use mysql_async::prelude::Queryable;
use mysql_async::Row;
use serde::Deserialize;

use crate::progress::{partition, progress_line, ProgressTimer, CONSOLE_LOCK};
use crate::rows;

#[derive(Deserialize, Default)]
pub(crate) struct FixFurnitureData {
    #[serde(default)]
    pub roomitemtypes: Option<FixItemTypes>,
    #[serde(default)]
    pub wallitemtypes: Option<FixItemTypes>,
}

#[derive(Deserialize, Default)]
pub(crate) struct FixItemTypes {
    #[serde(default)]
    pub furnitype: Option<Vec<FixItem>>,
}

#[derive(Deserialize, Default, Clone)]
pub(crate) struct FixItem {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    #[allow(dead_code)]
    pub classname: Option<String>,
    #[serde(default)]
    pub canstandon: bool,
    #[serde(default)]
    pub cansiton: bool,
    #[serde(default)]
    pub canlayon: bool,
    /// Not from JSON in practice: set for the wallitemtypes entries.
    #[serde(default, rename = "isWall")]
    pub is_wall: bool,
}

fn say(text: &str) {
    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    habbo_core::console::writeln(text);
}

pub(crate) fn furniture_data_path() -> std::path::PathBuf {
    habbo_core::workspace::runtime::router().game_data_file("FurnitureData.json", "./Database/Variables/FurnitureData.json")
}

/// `File.ReadAllText`: the BOM is dropped.
pub(crate) fn read_all_text(path: &std::path::Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

/// The batched UPDATE of one batch of (items_base id, furnidata entry).
pub(crate) fn batch_update_query(batch: &[(i32, FixItem)]) -> String {
    let mut walk = String::from("CASE id ");
    let mut sit = String::from("CASE id ");
    let mut lay = String::from("CASE id ");
    let mut width = String::from("CASE id ");
    let mut length = String::from("CASE id ");
    let mut stack_height = String::from("CASE id ");
    let mut ids = Vec::with_capacity(batch.len());

    for (item_base_id, fix_item) in batch {
        ids.push(item_base_id.to_string());
        let flag = |b: bool| if b { "1" } else { "0" };
        walk.push_str(&format!("WHEN {} THEN '{}' ", item_base_id, flag(fix_item.canstandon)));
        sit.push_str(&format!("WHEN {} THEN '{}' ", item_base_id, flag(fix_item.cansiton)));
        lay.push_str(&format!("WHEN {} THEN '{}' ", item_base_id, flag(fix_item.canlayon)));

        // Wall items ('i') don't use tile dimensions; normalize to neutral server defaults.
        if fix_item.is_wall {
            width.push_str(&format!("WHEN {item_base_id} THEN '1' "));
            length.push_str(&format!("WHEN {item_base_id} THEN '1' "));
            stack_height.push_str(&format!("WHEN {item_base_id} THEN '0.00' "));
        }
    }
    walk.push_str("ELSE allow_walk END");
    sit.push_str("ELSE allow_sit END");
    lay.push_str("ELSE allow_lay END");
    // Fix over the C#: a batch without wall items left "CASE id ELSE width END" (no WHEN),
    // which is invalid SQL, so the whole batch failed. Then the columns simply stay as they are.
    if batch.iter().any(|(_, item)| item.is_wall) {
        width.push_str("ELSE width END");
        length.push_str("ELSE length END");
        stack_height.push_str("ELSE stack_height END");
    } else {
        width = "width".into();
        length = "length".into();
        stack_height = "stack_height".into();
    }

    format!(
        "UPDATE items_base SET allow_walk = {walk}, allow_sit = {sit}, allow_lay = {lay}, width = {width}, length = {length}, stack_height = {stack_height} WHERE id IN ({})",
        ids.join(",")
    )
}

/// `FixItemSettings.RunAsync`.
pub async fn run() -> Result<()> {
    let json_file_path = furniture_data_path();
    if !json_file_path.is_file() {
        say(&format!("⚠️ FurnitureData.json not found: {}", json_file_path.display()));
        return Ok(());
    }

    let json_content = match read_all_text(&json_file_path) {
        Ok(text) => text,
        Err(error) => {
            say(&format!("Error reading JSON file: {error}"));
            return Ok(());
        }
    };

    let data: Option<FixFurnitureData> = match serde_json::from_str(&json_content) {
        Ok(data) => data,
        Err(error) => {
            say(&format!("Error deserializing JSON: {error}"));
            return Ok(());
        }
    };

    let mut all_fix_items: Vec<FixItem> = Vec::new();
    if let Some(data) = data {
        if let Some(items) = data.roomitemtypes.and_then(|t| t.furnitype) {
            all_fix_items.extend(items);
        }
        if let Some(items) = data.wallitemtypes.and_then(|t| t.furnitype) {
            all_fix_items.extend(items.into_iter().map(|mut item| {
                item.is_wall = true;
                item
            }));
        }
    }

    if all_fix_items.is_empty() {
        say("No fix items found in the JSON file.");
        return Ok(());
    }

    say(&format!("🔍 Loaded {} fix items from JSON.", all_fix_items.len()));

    let mut sprite_id_to_item_base_id: HashMap<i32, i32> = HashMap::new();
    let mapping: Result<()> = async {
        let mut connection = crate::config::connect().await?;
        let mut sprite_ids: Vec<i32> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for item in &all_fix_items {
            if seen.insert(item.id) {
                sprite_ids.push(item.id);
            }
        }
        let in_clause = sprite_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let result: Vec<Row> = connection.query(format!("SELECT id, sprite_id FROM items_base WHERE sprite_id IN ({in_clause})")).await?;
        for row in &result {
            let item_base_id = rows::get_i32(row, "id")?;
            let sprite_id = rows::get_i32(row, "sprite_id")?;
            sprite_id_to_item_base_id.entry(sprite_id).or_insert(item_base_id);
        }
        let _ = connection.disconnect().await;
        Ok(())
    }
    .await;
    if let Err(error) = mapping {
        say(&format!("Error mapping sprite_ids: {error}"));
        return Ok(());
    }

    let items_to_update: Vec<(i32, FixItem)> = all_fix_items
        .iter()
        .filter_map(|f| sprite_id_to_item_base_id.get(&f.id).map(|id| (*id, f.clone())))
        .collect();

    let total_to_update = items_to_update.len();
    if total_to_update == 0 {
        say("No matching items_base records found for any fix items.");
        return Ok(());
    }

    // New behaviour (not in the C# tool): back up items_base first; no backup, no changes.
    if !crate::backup::backup_before_change(&["items_base"], "fix_item_settings").await {
        return Ok(());
    }

    let total_batches = total_to_update.div_ceil(100);
    // Dropped on every return path: an error must not leave the progress timer printing forever.
    let timer = ProgressTimer::start(total_batches);

    let batches = partition(&items_to_update, 100);

    let mut connection = crate::config::connect().await?;
    for batch in &batches {
        if let Err(error) = connection.query_drop(batch_update_query(batch)).await {
            say(&format!("\nError updating batch: {error}"));
        }
        timer.increment();
    }
    let _ = connection.disconnect().await;

    timer.stop();
    drop(timer);

    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Final status line.
    outln!("{}", progress_line(total_batches, total_batches));
    outln!("\nFixItemSettings process completed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_query_text() {
        let room = FixItem { id: 10, canstandon: true, ..Default::default() };
        let wall = FixItem { id: 11, cansiton: true, canlayon: true, is_wall: true, ..Default::default() };
        let query = batch_update_query(&[(1, room), (2, wall)]);
        assert_eq!(
            query,
            "UPDATE items_base SET allow_walk = CASE id WHEN 1 THEN '1' WHEN 2 THEN '0' ELSE allow_walk END, \
allow_sit = CASE id WHEN 1 THEN '0' WHEN 2 THEN '1' ELSE allow_sit END, \
allow_lay = CASE id WHEN 1 THEN '0' WHEN 2 THEN '1' ELSE allow_lay END, \
width = CASE id WHEN 2 THEN '1' ELSE width END, \
length = CASE id WHEN 2 THEN '1' ELSE length END, \
stack_height = CASE id WHEN 2 THEN '0.00' ELSE stack_height END WHERE id IN (1,2)"
        );
    }

    #[test]
    fn batch_without_wall_items_is_valid_sql() {
        let room = FixItem { id: 10, canstandon: true, ..Default::default() };
        let query = batch_update_query(&[(1, room)]);
        assert!(!query.contains("CASE id ELSE"), "{query}");
        assert!(query.ends_with("width = width, length = length, stack_height = stack_height WHERE id IN (1)"), "{query}");
    }

    #[test]
    fn deserializes_like_stj() {
        let data: Option<FixFurnitureData> =
            serde_json::from_str(r#"{"roomitemtypes":{"furnitype":[{"id":1,"classname":"a","canstandon":true,"extra":5}]},"wallitemtypes":null}"#).unwrap();
        let data = data.unwrap();
        assert_eq!(data.roomitemtypes.unwrap().furnitype.unwrap()[0].id, 1);
        assert!(data.wallitemtypes.is_none());
        assert!(serde_json::from_str::<Option<FixFurnitureData>>(r#"{"roomitemtypes":{"furnitype":[{"id":"x"}]}}"#).is_err());
    }
}
