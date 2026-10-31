//! `FixInteractionCount`: Generate/Update_Interaction_count_<date>.sql with the
//! interaction_modes_count each default floor / wall furni gets from its .hab. The database
//! itself is only read.

use anyhow::Result;
use habbo_core::outln;
use mysql_async::prelude::Queryable;
use mysql_async::Row;

use crate::hab_furni_scan::{self as scan, append_line};
use crate::rows;
use crate::sql_generator::interaction_modes_count_from_nitro_json;

struct ItemRow {
    id: i32,
    item_name: String,
    current_count: i32,
}

const CHECKED_ROWS: &str = "type s or i, no pets, interaction_type 'default', no multiheight";

pub(crate) fn query() -> String {
    format!(
        "SELECT id, item_name, interaction_modes_count FROM items_base WHERE interaction_type = 'default' AND {} ORDER BY id",
        scan::BASE_FILTER
    )
}

async fn read_rows() -> Result<Vec<ItemRow>> {
    let mut connection = crate::config::connect().await?;
    let result: Vec<Row> = connection.query(query()).await?;
    let mut rows_out = Vec::with_capacity(result.len());
    for row in &result {
        rows_out.push(ItemRow {
            id: rows::get_i32(row, 0usize)?,
            item_name: if rows::is_null(row, 1usize)? { String::new() } else { rows::get_string(row, 1usize)? },
            current_count: if rows::is_null(row, 2usize)? { 0 } else { rows::get_i32(row, 2usize)? },
        });
    }
    let _ = connection.disconnect().await;
    Ok(rows_out)
}

/// The UPDATE line of one changed row.
pub(crate) fn update_line(id: i32, item_name: &str, current: i32, new_count: i32) -> String {
    format!(
        "UPDATE `items_base` SET `interaction_modes_count` = {new_count} WHERE `id` = {id} AND `interaction_type` = 'default'; -- {item_name}: {current} -> {new_count}"
    )
}

/// `FixInteractionCount.RunAsync`.
pub async fn run() -> Result<()> {
    if !scan::check_hab_folder() {
        return Ok(());
    }

    let rows = match read_rows().await {
        Ok(rows) => rows,
        Err(error) => {
            outln!("❌ Could not read items_base: {error}");
            return Ok(());
        }
    };

    outln!("🔍 {} floor and wall furni with interaction_type 'default' and no multiheight (pets left out).", rows.len());

    let counts = scan::read_bundles(rows.iter().map(|r| r.item_name.clone()).collect(), |root| {
        Some(interaction_modes_count_from_nitro_json(root))
    })
    .await?;

    let mut updates = Vec::new();
    let mut missing = Vec::new();
    for row in &rows {
        match counts.result_for(&row.item_name) {
            None => missing.push(row.item_name.clone()),
            Some(count) if count != row.current_count => updates.push((row, count)),
            Some(_) => {}
        }
    }

    let (output_path, missing_path) = scan::output_paths("Update_Interaction_count")?;

    let mut sql = String::new();
    append_line(&mut sql, &format!("-- interaction_modes_count from Habbo_Default/hof_furni/hab, generated {}", scan::now()));
    append_line(&mut sql, "-- Only floor/wall furni (type s or i, no pets 'a0*') with interaction_type = 'default' and no multiheight; only rows whose count changes.");
    append_line(
        &mut sql,
        &format!(
            "-- Checked {} rows, {} change, {} have no readable .hab (see {}).",
            rows.len(),
            updates.len(),
            missing.len(),
            habbo_core::paths::file_name(&missing_path)
        ),
    );
    append_line(&mut sql, "");
    for (row, new_count) in &updates {
        append_line(&mut sql, &update_line(row.id, &row.item_name, row.current_count, *new_count));
    }

    std::fs::write(&output_path, sql)?;

    if !missing.is_empty() {
        scan::write_missing_list(&missing_path, CHECKED_ROWS, &missing)?;
    }

    outln!("✅ {} item(s) get a new interaction_modes_count; {} without a .hab.", updates.len(), missing.len());
    outln!("📄 {}", output_path.display());
    if !missing.is_empty() {
        outln!("📄 {}", missing_path.display());
    }
    outln!("   Check the file, then run it on your database and restart the emulator (or reload the items).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_line() {
        assert_eq!(
            query(),
            "SELECT id, item_name, interaction_modes_count FROM items_base WHERE interaction_type = 'default' AND (multiheight IS NULL OR multiheight IN ('', '0')) AND type IN ('s', 'i') AND item_name NOT LIKE 'a0%' ORDER BY id"
        );
        assert_eq!(
            update_line(4, "chair*2", 1, 3),
            "UPDATE `items_base` SET `interaction_modes_count` = 3 WHERE `id` = 4 AND `interaction_type` = 'default'; -- chair*2: 1 -> 3"
        );
    }
}
