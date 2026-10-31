//! `DatabaseGeneralInfo`: server version, database sizes and users.

use anyhow::Result;
use habbo_core::console::{reset_color, set_color};
use habbo_core::{outln, Color};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};

use crate::rows;

const DB_SIZE_QUERY: &str = "
                            SELECT 
                                table_schema AS `Database`, 
                                ROUND(SUM(data_length + index_length) / 1024 / 1024, 2) AS `SizeMB`
                            FROM information_schema.TABLES
                            GROUP BY table_schema;
                        ";

/// `DatabaseGeneralInfo.ShowDatabaseVersionAsync`.
pub async fn show_database_version() -> Result<()> {
    if let Err(error) = run().await {
        set_color(Color::Red);
        outln!("An error occurred while connecting to the database:");
        outln!("{error}");
        reset_color();
    }
    Ok(())
}

async fn run() -> Result<()> {
    let mut connection = crate::config::connect().await?;

    let version: Option<Row> = connection.query_first("SELECT VERSION() AS Version").await?;
    let version = version
        .and_then(|row| row.as_ref(0).cloned())
        .filter(|v| *v != mysql_async::Value::NULL)
        .map(|v| rows::value_to_string(&v))
        .unwrap_or_else(|| "Unknown".into());
    outln!("Database Version: {version}");

    outln!("\nDatabases on Server (with size in MB):");
    if let Err(error) = sizes(&mut connection).await {
        set_color(Color::Red);
        outln!("Could not retrieve database sizes: {error}");
        reset_color();
    }

    outln!("\nUsers on the Server:");
    if let Err(error) = users(&mut connection).await {
        set_color(Color::Red);
        outln!("Could not retrieve user information: {error}");
        reset_color();
    }

    let _ = connection.disconnect().await;
    Ok(())
}

async fn sizes(connection: &mut Conn) -> Result<()> {
    let result: Vec<Row> = connection.query(DB_SIZE_QUERY).await?;
    for row in result {
        let db_name = rows::get_display(&row, "Database")?;
        let size_mb = rows::get_display(&row, "SizeMB")?;
        outln!("💾 {db_name}, Size: {size_mb} MB");
    }
    Ok(())
}

async fn users(connection: &mut Conn) -> Result<()> {
    let result: Vec<Row> = connection.query("SELECT DISTINCT User FROM mysql.user;").await?;
    for row in result {
        let user = rows::get_display(&row, "User")?;
        outln!("👤 Username: {user}");
    }
    Ok(())
}
