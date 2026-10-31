//! `SetOfferID`: catalog_items.offer_id from FurnitureData.json (by catalog_name = classname);
//! furni without an offer id get a new unique one (> 60000), saved back into the json.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use habbo_core::outln;
use mysql_async::prelude::Queryable;
use mysql_async::Row;
use serde::{Deserialize, Serialize};

use crate::fix_item_settings::{furniture_data_path, read_all_text};
use crate::progress::{partition, progress_line, ProgressTimer, CONSOLE_LOCK};
use crate::rows;

/// The C# `FurnitureData` model: (de)serialized with its own property order, so the saved json
/// keeps only these fields, in this order.
#[derive(Deserialize, Serialize, Default)]
pub struct FurnitureData {
    #[serde(default)]
    pub roomitemtypes: Option<ItemTypes>,
    #[serde(default)]
    pub wallitemtypes: Option<ItemTypes>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct ItemTypes {
    #[serde(default)]
    pub furnitype: Option<Vec<FurnitureItem>>,
}

fn minus_one() -> i32 {
    -1
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FurnitureItem {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub classname: Option<String>,
    #[serde(default = "minus_one")]
    pub offerid: i32,
    #[serde(default)]
    pub revision: i32,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub defaultdir: i32,
    #[serde(default)]
    pub xdim: i32,
    #[serde(default)]
    pub ydim: i32,
    #[serde(default)]
    pub partcolors: Option<PartColors>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub adurl: Option<String>,
    #[serde(default)]
    pub buyout: bool,
    #[serde(default)]
    pub rentofferid: i32,
    #[serde(default)]
    pub rentbuyout: bool,
    #[serde(default)]
    pub bc: bool,
    #[serde(default)]
    pub excludeddynamic: bool,
    #[serde(default)]
    pub customparams: Option<String>,
    #[serde(default)]
    pub specialtype: i32,
    #[serde(default)]
    pub canstandon: bool,
    #[serde(default)]
    pub cansiton: bool,
    #[serde(default)]
    pub canlayon: bool,
    #[serde(default)]
    pub furniline: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub rare: bool,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct PartColors {
    #[serde(default)]
    pub color: Option<Vec<Option<String>>>,
}

/// `JsonSerializer.Serialize(furnitureData, WriteIndented = true)`.
pub fn serialize_indented(data: &FurnitureData) -> Result<String> {
    Ok(crate::dotnet::stj_indented(&serde_json::to_value(data)?))
}

/// `new Random().Next(60001, int.MaxValue)`.
fn next_offer_id() -> i32 {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let random = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let span = (i32::MAX as u64) - 60001;
    (60001 + random % span) as i32
}

/// Groups the items by classname (first appearance order), gives every group whose first
/// item has no offer id (-1) a new unused one, and returns classname -> offer id.
pub(crate) fn assign_offer_ids(
    items: &mut [&mut FurnitureItem],
    used_offer_ids: &mut HashSet<i32>,
    mut next: impl FnMut() -> i32,
) -> Result<(HashMap<String, i32>, usize)> {
    let mut order: Vec<Option<String>> = Vec::new();
    let mut groups: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let key = item.classname.clone();
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(index);
    }

    let mut mapping = HashMap::new();
    let mut generated = 0;
    for key in order {
        let members = &groups[&key];
        let mut offer_id = items[members[0]].offerid;
        if offer_id == -1 {
            let mut new_id;
            loop {
                new_id = next();
                if !used_offer_ids.contains(&new_id) {
                    break;
                }
            }
            used_offer_ids.insert(new_id);
            offer_id = new_id;
            generated += 1;
            for index in members {
                items[*index].offerid = new_id;
            }
        }
        let Some(key) = key else {
            // offerMapping[null] threw in C#.
            bail!("Value cannot be null. (Parameter 'key')");
        };
        mapping.insert(key, offer_id);
    }
    Ok((mapping, generated))
}

pub(crate) fn offer_id_batch_query(batch: &[(i32, i32)]) -> String {
    let mut case = String::from("CASE id ");
    for (catalog_id, new_offer_id) in batch {
        case.push_str(&format!("WHEN {catalog_id} THEN {new_offer_id} "));
    }
    case.push_str("ELSE offer_id END");
    let ids: Vec<String> = batch.iter().map(|(id, _)| id.to_string()).collect();
    format!("UPDATE catalog_items SET offer_id = {case} WHERE id IN ({})", ids.join(","))
}

fn say(text: &str) {
    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    habbo_core::console::writeln(text);
}

fn save_json(path: &std::path::Path, data: &FurnitureData) {
    match serialize_indented(data).and_then(|json| Ok(std::fs::write(path, json)?)) {
        Ok(()) => outln!("Updated JSON saved with new offer IDs."),
        Err(error) => outln!("Error saving updated JSON: {error}"),
    }
}

/// `SetOfferID.RunAsync`.
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

    let furniture_data: Option<FurnitureData> = match serde_json::from_str(&json_content) {
        Ok(data) => data,
        Err(error) => {
            say(&format!("Error deserializing JSON: {error}"));
            return Ok(());
        }
    };
    let Some(mut furniture_data) = furniture_data else {
        bail!("Object reference not set to an instance of an object.");
    };

    let item_count = [&furniture_data.roomitemtypes, &furniture_data.wallitemtypes]
        .into_iter()
        .flatten()
        .filter_map(|t| t.furnitype.as_ref())
        .map(Vec::len)
        .sum::<usize>();
    if item_count == 0 {
        outln!("No furniture items found in the JSON file.");
        return Ok(());
    }

    outln!("🔍 {item_count} furniture items loaded from JSON.");

    // Fetch catalog_items from the database to get existing offer_ids.
    let mut catalog_items: Vec<(i32, String, i32)> = Vec::new();
    let mut used_offer_ids: HashSet<i32> = HashSet::new();
    let fetched: Result<()> = async {
        let mut connection = crate::config::connect().await?;
        let result: Vec<Row> = connection.query("SELECT id, catalog_name, offer_id FROM catalog_items").await?;
        for row in &result {
            let offer_id = rows::get_i32(row, "offer_id")?;
            catalog_items.push((rows::get_i32(row, "id")?, rows::get_string(row, "catalog_name")?, offer_id));
            if offer_id > 0 {
                used_offer_ids.insert(offer_id);
            }
        }
        let _ = connection.disconnect().await;
        Ok(())
    }
    .await;
    if let Err(error) = fetched {
        say(&format!("Error fetching catalog_items: {error}"));
        return Ok(());
    }

    if catalog_items.is_empty() {
        outln!("No items found in catalog_items.");
        return Ok(());
    }

    let (offer_mapping, generated_count) = {
        let mut all_items: Vec<&mut FurnitureItem> = Vec::new();
        for types in [&mut furniture_data.roomitemtypes, &mut furniture_data.wallitemtypes].into_iter().flatten() {
            if let Some(items) = types.furnitype.as_mut() {
                all_items.extend(items.iter_mut());
            }
        }
        assign_offer_ids(&mut all_items, &mut used_offer_ids, next_offer_id)?
    };

    if generated_count > 0 {
        outln!("🔢 Generated {generated_count} new unique offer IDs (>60000).");
    }

    let items_to_update: Vec<(i32, i32)> = catalog_items
        .iter()
        .filter_map(|(catalog_id, catalog_name, current)| {
            offer_mapping.get(catalog_name).filter(|offer| **offer != *current).map(|offer| (*catalog_id, *offer))
        })
        .collect();

    if items_to_update.is_empty() {
        outln!("No catalog_items need offer_id updates.");
        save_json(&json_file_path, &furniture_data);
        return Ok(());
    }

    outln!("🔄 {} catalog_items need offer_id updates.", items_to_update.len());

    let batches = partition(&items_to_update, 100);
    let total_batches = batches.len();
    let timer = ProgressTimer::start(total_batches);

    let updated: Result<()> = async {
        let mut connection = crate::config::connect().await?;
        for batch in &batches {
            if let Err(error) = connection.query_drop(offer_id_batch_query(batch)).await {
                outln!("\nError updating batch: {error}");
            }
            timer.increment();
        }
        let _ = connection.disconnect().await;
        Ok(())
    }
    .await;
    if let Err(error) = updated {
        outln!("Error updating catalog_items: {error}");
    }

    timer.stop();
    drop(timer);

    save_json(&json_file_path, &furniture_data);

    let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    outln!("{}", progress_line(total_batches, total_batches));
    outln!("\nSetOfferID process completed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_ids_and_json() {
        let json = r##"{"roomitemtypes":{"furnitype":[
            {"id":1,"classname":"chair","offerid":-1,"unknown":true,"partcolors":{"color":["#fff"]}},
            {"id":2,"classname":"chair"},
            {"id":3,"classname":"table","offerid":44}
        ]}}"##;
        let mut data: FurnitureData = serde_json::from_str(json).unwrap();
        let mut used = HashSet::from([70000]);
        let mut sequence = vec![70001, 70000].into_iter();
        let (mapping, generated) = {
            let mut items: Vec<&mut FurnitureItem> = data.roomitemtypes.as_mut().unwrap().furnitype.as_mut().unwrap().iter_mut().collect();
            assign_offer_ids(&mut items, &mut used, || sequence.next().unwrap()).unwrap()
        };
        assert_eq!(generated, 1);
        assert_eq!(mapping["chair"], 70000 + 1);
        assert_eq!(mapping["table"], 44);
        let items = data.roomitemtypes.as_ref().unwrap().furnitype.as_ref().unwrap();
        assert_eq!(items[1].offerid, 70001);

        let text = serialize_indented(&data).unwrap();
        let nl = habbo_core::paths::NEWLINE;
        assert!(text.starts_with(&format!("{{{nl}  \"roomitemtypes\": {{{nl}    \"furnitype\": [{nl}      {{{nl}        \"id\": 1,{nl}        \"classname\": \"chair\",{nl}        \"offerid\": 70001,{nl}        \"revision\": 0,")));
        assert!(text.contains(&format!("\"partcolors\": {{{nl}          \"color\": [{nl}            \"#fff\"{nl}          ]{nl}        }},")));
        assert!(text.contains("\"partcolors\": null"));
        assert!(text.ends_with(&format!("\"wallitemtypes\": null{nl}}}")));
        assert!(!text.contains("unknown"));
        assert_eq!(offer_id_batch_query(&[(1, 2)]), "UPDATE catalog_items SET offer_id = CASE id WHEN 1 THEN 2 ELSE offer_id END WHERE id IN (1)");
    }
}
