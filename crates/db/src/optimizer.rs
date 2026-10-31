//! `DatabaseOptimizer`: `OPTIMIZE TABLE` on every table of the configured database.

use anyhow::Result;
use habbo_core::console::{reset_color, set_color};
use habbo_core::{outln, Color};
use mysql_async::prelude::Queryable;
use mysql_async::Row;

use crate::rows;

/// `DatabaseOptimizer.OptimizeDatabaseTablesAsync`.
pub async fn optimize_database_tables() -> Result<()> {
    if let Err(error) = run().await {
        set_color(Color::Red);
        outln!("An error occurred during database optimization:");
        outln!("{error}");
        reset_color();
    }
    Ok(())
}

async fn run() -> Result<()> {
    let mut connection = crate::config::connect().await?;

    let result: Vec<Row> = connection.query("SHOW TABLES;").await?;
    let mut tables = Vec::with_capacity(result.len());
    for row in &result {
        tables.push(rows::get_string(row, 0usize)?);
    }

    if tables.is_empty() {
        outln!("No tables found in the database.");
        return Ok(());
    }

    outln!("Optimizing {} table(s) in database '{}':", tables.len(), crate::config::get().database_name);

    for table in &tables {
        // ExecuteNonQuery reads (and discards) the result set OPTIMIZE TABLE returns.
        connection.query_drop(format!("OPTIMIZE TABLE `{table}`;")).await?;
        outln!("Optimized table: {table}");
    }

    outln!("Database optimization complete.");
    let _ = connection.disconnect().await;
    Ok(())
}
