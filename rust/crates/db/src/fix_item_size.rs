//! `FixItemSize`: Generate/Update_Item_size_<date>.sql with width / length / stack_height the
//! way Generate SQL sets them (floor furni from logic.model.dimensions of their .hab, wall
//! furni always 1 x 1 x 0.00). The database itself is only read.

use anyhow::Result;
use habbo_core::outln;
use mysql_async::prelude::Queryable;
use mysql_async::Row;

use crate::dotnet::format_fixed;
use crate::hab_furni_scan::{self as scan, append_line};
use crate::rows;
use crate::sql_generator::dimensions_from_nitro_json;

struct ItemRow {
    id: i32,
    item_name: String,
    kind: String,
    width: i32,
    length: i32,
    stack_height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    pub width: i32,
    pub length: i32,
    pub stack_height: f64,
}

const CHECKED_ROWS: &str = "type s or i, no pets, no multiheight";

/// A wall furni has no tiles of its own: Generate SQL writes it as 1 x 1 x 0.00.
const WALL_SIZE: Size = Size { width: 1, length: 1, stack_height: 0.00 };

pub(crate) fn query() -> String {
    format!("SELECT id, item_name, type, width, length, stack_height FROM items_base WHERE {} ORDER BY id", scan::BASE_FILTER)
}

/// items_base keeps whole tiles and a stack height with 2 decimals (MidpointRounding.AwayFromZero).
pub(crate) fn size_from_dimensions((width, length, height): (f64, f64, f64)) -> Size {
    Size {
        width: (width.round() as i32).max(1),
        length: (length.round() as i32).max(1),
        stack_height: (height * 100.0).round() / 100.0,
    }
}

async fn read_rows() -> Result<Vec<ItemRow>> {
    let mut connection = crate::config::connect().await?;
    let result: Vec<Row> = connection.query(query()).await?;
    let mut out = Vec::with_capacity(result.len());
    for row in &result {
        out.push(ItemRow {
            id: rows::get_i32(row, 0usize)?,
            item_name: if rows::is_null(row, 1usize)? { String::new() } else { rows::get_string(row, 1usize)? },
            kind: if rows::is_null(row, 2usize)? { String::new() } else { rows::get_string(row, 2usize)? },
            width: if rows::is_null(row, 3usize)? { 1 } else { rows::get_i32(row, 3usize)? },
            length: if rows::is_null(row, 4usize)? { 1 } else { rows::get_i32(row, 4usize)? },
            stack_height: if rows::is_null(row, 5usize)? { 0.0 } else { rows::get_f64(row, 5usize)? },
        });
    }
    let _ = connection.disconnect().await;
    Ok(out)
}

pub(crate) fn update_line(id: i32, item_name: &str, old: Size, size: Size) -> String {
    let height = format_fixed(size.stack_height, 2);
    let old_height = format_fixed(old.stack_height, 2);
    format!(
        "UPDATE `items_base` SET `width` = {}, `length` = {}, `stack_height` = {height} WHERE `id` = {id} AND (`multiheight` IS NULL OR `multiheight` IN ('', '0')); -- {item_name}: {}x{}x{old_height} -> {}x{}x{height}",
        size.width, size.length, old.width, old.length, size.width, size.length
    )
}

/// `FixItemSize.RunAsync`.
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

    let floor_names: Vec<String> = rows.iter().filter(|r| r.kind == "s").map(|r| r.item_name.clone()).collect();
    outln!(
        "🔍 {} furni without multiheight (pets left out): {} floor, {} wall.",
        rows.len(),
        floor_names.len(),
        rows.len() - floor_names.len()
    );

    // Only floor furni need their bundle; wall furni always get the wall size.
    let sizes = scan::read_bundles(floor_names, |root| dimensions_from_nitro_json(root).map(size_from_dimensions)).await?;

    let mut updates = Vec::new();
    let mut missing = Vec::new();
    for row in &rows {
        let size = if row.kind == "i" { Some(WALL_SIZE) } else { sizes.result_for(&row.item_name) };
        let Some(size) = size else {
            missing.push(row.item_name.clone());
            continue;
        };
        if size.width != row.width || size.length != row.length || (size.stack_height - row.stack_height).abs() > 0.001 {
            updates.push((row, size));
        }
    }

    let (output_path, missing_path) = scan::output_paths("Update_Item_size")?;

    let mut sql = String::new();
    append_line(&mut sql, &format!("-- width / length / stack_height from Habbo_Default/hof_furni/hab, generated {}", scan::now()));
    append_line(&mut sql, "-- Only floor/wall furni (type s or i, no pets 'a0*') without multiheight; wall furni are 1 x 1 x 0.00; only rows that change.");
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
    for (row, size) in &updates {
        let old = Size { width: row.width, length: row.length, stack_height: row.stack_height };
        append_line(&mut sql, &update_line(row.id, &row.item_name, old, *size));
    }

    std::fs::write(&output_path, sql)?;

    if !missing.is_empty() {
        scan::write_missing_list(&missing_path, CHECKED_ROWS, &missing)?;
    }

    outln!("✅ {} item(s) get a new width / length / height; {} without a .hab.", updates.len(), missing.len());
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
    fn sizes_and_line() {
        assert_eq!(size_from_dimensions((2.5, 0.2, 1.005)), Size { width: 3, length: 1, stack_height: 1.0 });
        assert_eq!(size_from_dimensions((1.0, 2.0, 0.125)).stack_height, 0.13);
        let old = Size { width: 1, length: 1, stack_height: 0.0 };
        let new = Size { width: 2, length: 1, stack_height: 1.5 };
        assert_eq!(
            update_line(9, "sofa", old, new),
            "UPDATE `items_base` SET `width` = 2, `length` = 1, `stack_height` = 1.50 WHERE `id` = 9 AND (`multiheight` IS NULL OR `multiheight` IN ('', '0')); -- sofa: 1x1x0.00 -> 2x1x1.50"
        );
        assert!(query().starts_with("SELECT id, item_name, type, width, length, stack_height FROM items_base WHERE (multiheight"));
    }
}
