//! Automatic backup of the tables an operation is about to change (new behaviour, not in the C#
//! tool): `Database/Backups/<operation>_<yyyy-MM-dd_HH-mm-ss>.sql`, written with mysql_async
//! only (no mysqldump). The file is a restore script made of
//! `REPLACE INTO `table` (cols…) VALUES (…),(…);` batches: running it puts every backed-up row
//! back to its saved values (rows added after the backup are left alone; nothing is deleted).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use habbo_core::console::{reset_color, set_color};
use habbo_core::{outln, Color};
use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Conn, Value};

/// Folder the backups go to, relative to the working directory.
pub const BACKUP_FOLDER: &str = "Database/Backups";

/// Rows per `REPLACE INTO` statement.
pub const BATCH_ROWS: usize = 500;

/// A statement is also closed once it gets this long, so huge rows stay far below the server's
/// `max_allowed_packet` (16 MB by default on MariaDB, 64 MB on MySQL 8) when restoring.
pub const BATCH_BYTES: usize = 1024 * 1024;

/// The binary pseudo character set (`BINARY`, `VARBINARY`, `BLOB` columns).
const BINARY_CHARSET: u16 = 63;

/// Dumps `tables` to a new `Database/Backups/<label>_<yyyy-MM-dd_HH-mm-ss>.sql` restore script and
/// returns its path. The file only appears once it is complete (it is written as `.partial`
/// first), so a failed backup never leaves a file that looks valid.
pub async fn backup_tables(conn: &mut Conn, tables: &[&str], label: &str) -> Result<PathBuf> {
    backup_tables_to(conn, tables, label, Path::new(BACKUP_FOLDER)).await
}

/// [`backup_tables`] into another folder.
pub async fn backup_tables_to(conn: &mut Conn, tables: &[&str], label: &str, folder: &Path) -> Result<PathBuf> {
    if tables.is_empty() {
        bail!("no tables to back up");
    }
    std::fs::create_dir_all(folder).with_context(|| format!("cannot create the folder {}", folder.display()))?;
    // An absolute path, so the printed restore command works from any folder.
    let folder = std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf());
    let folder = folder.as_path();
    let now = chrono::Local::now().naive_local();
    let path = unique_path(folder, &backup_file_name(label, now));
    let partial = path.with_extension("sql.partial");

    let result = write_backup(conn, tables, label, now, &partial).await;
    match result {
        Ok(()) => {
            std::fs::rename(&partial, &path).with_context(|| format!("cannot rename {}", partial.display()))?;
            Ok(path)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            Err(error)
        }
    }
}

async fn write_backup(conn: &mut Conn, tables: &[&str], label: &str, now: chrono::NaiveDateTime, path: &Path) -> Result<()> {
    // The file says SET NAMES utf8mb4, so the text it contains must be read the same way.
    // TIMESTAMP values are read (and restored) in UTC, so the server's time zone cannot shift them.
    conn.query_drop("SET NAMES utf8mb4").await?;
    let old_time_zone: Option<String> = conn.query_first("SELECT @@SESSION.time_zone").await?;
    conn.query_drop("SET time_zone = '+00:00'").await?;
    let result = write_backup_file(conn, tables, label, now, path).await;
    if let Some(zone) = old_time_zone {
        let _ = conn.query_drop(format!("SET time_zone = {}", quote_string(&zone))).await;
    }
    result
}

async fn write_backup_file(conn: &mut Conn, tables: &[&str], label: &str, now: chrono::NaiveDateTime, path: &Path) -> Result<()> {
    let (server, port, user, database) = {
        let opts = conn.opts();
        (opts.ip_or_hostname().to_string(), opts.tcp_port(), opts.user().unwrap_or("root").to_string(), opts.db_name().unwrap_or("").to_string())
    };
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;

    let file = File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut out = BufWriter::new(file);
    let header = Header {
        label,
        created: now,
        server: &server,
        port,
        user: &user,
        database: &database,
        version: version.as_deref().unwrap_or("?"),
        tables,
        file_name: path.file_name().and_then(|n| n.to_str()).map(|n| n.trim_end_matches(".partial")).unwrap_or(""),
    };
    out.write_all(header.render().as_bytes())?;

    for table in tables {
        let mut result = conn.query_iter(format!("SELECT * FROM {}", quote_ident(table))).await.with_context(|| format!("cannot read `{table}`"))?;
        let columns = result.columns().map(|c| c.to_vec()).unwrap_or_default();
        let kinds: Vec<ColumnKind> = columns.iter().map(ColumnKind::of).collect();
        let names: Vec<String> = columns.iter().map(|c| c.name_str().into_owned()).collect();

        writeln!(out, "\n-- Table {}", quote_ident(table))?;
        let mut writer = InsertWriter::new(&mut out, table, &names);
        while let Some(row) = result.next().await? {
            let mut values = Vec::with_capacity(kinds.len());
            for (index, kind) in kinds.iter().enumerate() {
                values.push(sql_value(row.as_ref(index).unwrap_or(&Value::NULL), *kind));
            }
            writer.push_row(&values)?;
        }
        let count = writer.finish()?;
        writeln!(out, "-- {count} row(s) in {}", quote_ident(table))?;
    }

    out.write_all(FOOTER.as_bytes())?;
    out.flush()?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(())
}

/// Backs up `tables` before an operation changes them and prints where the file went and how to
/// restore it. On failure it prints why and returns `false`: the caller must stop without
/// changing anything.
pub(crate) async fn backup_before_change(tables: &[&str], label: &str) -> bool {
    let result: Result<PathBuf> = async {
        let mut connection = crate::config::connect().await?;
        let path = backup_tables(&mut connection, tables, label).await;
        let _ = connection.disconnect().await;
        path
    }
    .await;
    match result {
        Ok(path) => {
            outc_line(Color::Green, &format!("💾 Backup of {} saved to: {}", tables.join(", "), path.display()));
            outln!("   To undo the changes, restore it with: {}", restore_command(&path));
            true
        }
        Err(error) => {
            outc_line(Color::Red, &format!("❌ Backup failed, nothing was changed: {}", error_line(&error)));
            false
        }
    }
}

/// `context: root cause` without the duplicated middle of mysql_async's error chain.
pub(crate) fn error_line(error: &anyhow::Error) -> String {
    let top = error.to_string();
    let root = error.root_cause().to_string();
    if error.chain().count() > 1 && !top.contains(&root) {
        format!("{top}: {root}")
    } else {
        top
    }
}

fn outc_line(color: Color, text: &str) {
    let _guard = crate::progress::CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_color(color);
    habbo_core::console::writeln(text);
    reset_color();
}

/// The `mysql … < file` command line that restores `path` with the configured settings.
pub fn restore_command(path: &Path) -> String {
    let config = crate::config::get();
    restore_command_for(&config.database_server, &config.port, &config.username, &config.database_name, path)
}

pub(crate) fn restore_command_for(server: &str, port: &str, user: &str, database: &str, path: &Path) -> String {
    format!("mysql -h {server} -P {port} -u {user} -p {database} < \"{}\"", path.display())
}

/// `<label>_<yyyy-MM-dd_HH-mm-ss>.sql`, with the label reduced to file-name-safe characters.
pub fn backup_file_name(label: &str, at: chrono::NaiveDateTime) -> String {
    let label: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let label = if label.is_empty() { "backup".to_string() } else { label };
    format!("{label}_{}.sql", at.format("%Y-%m-%d_%H-%M-%S"))
}

/// `folder/name`, or `folder/<stem>_2.sql`, `_3`, … when a backup with that name already exists
/// (two backups in the same second).
pub(crate) fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".sql").unwrap_or(name);
    (2..)
        .map(|n| folder.join(format!("{stem}_{n}.sql")))
        .find(|p| !p.exists())
        .expect("an unused file name")
}

struct Header<'a> {
    label: &'a str,
    created: chrono::NaiveDateTime,
    server: &'a str,
    port: u16,
    user: &'a str,
    database: &'a str,
    version: &'a str,
    tables: &'a [&'a str],
    file_name: &'a str,
}

impl Header<'_> {
    fn render(&self) -> String {
        let restore = restore_command_for(self.server, &self.port.to_string(), self.user, self.database, Path::new(self.file_name));
        format!(
            "-- Habbo all-in-1 converter: automatic backup before \"{label}\"\n\
-- Created:  {created}\n\
-- Server:   {server}:{port} ({version})\n\
-- Database: {database}\n\
-- Tables:   {tables}\n\
--\n\
-- Restore:  {restore}\n\
-- Every backed-up row is written back with REPLACE INTO (same primary key = overwritten).\n\
-- Rows added after the backup are kept; nothing is deleted.\n\
\n\
SET NAMES utf8mb4;\n\
SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0;\n\
SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0;\n\
SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n\
SET @OLD_TIME_ZONE=@@TIME_ZONE, TIME_ZONE='+00:00';\n",
            label = self.label.replace(['\n', '\r'], " "),
            created = self.created.format("%Y-%m-%d %H:%M:%S"),
            server = self.server,
            port = self.port,
            version = self.version,
            database = self.database,
            tables = self.tables.join(", "),
        )
    }
}

const FOOTER: &str = "\nSET TIME_ZONE=@OLD_TIME_ZONE;\n\
SET SQL_MODE=@OLD_SQL_MODE;\n\
SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS;\n\
SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS;\n\
-- End of backup\n";

/// How a column's values are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnKind {
    /// Integer / decimal / float / year: written bare when the text is a plain number.
    Number,
    /// BINARY / VARBINARY / BLOB / BIT / GEOMETRY: written as a hex literal.
    Binary,
    /// Everything else (text, enum, set, dates, json): an escaped string literal.
    Text,
}

impl ColumnKind {
    fn of(column: &Column) -> Self {
        use ColumnType::*;
        match column.column_type() {
            MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_INT24 | MYSQL_TYPE_FLOAT
            | MYSQL_TYPE_DOUBLE | MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL | MYSQL_TYPE_YEAR => ColumnKind::Number,
            MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => ColumnKind::Binary,
            MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB | MYSQL_TYPE_STRING
            | MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_VARCHAR
                if column.character_set() == BINARY_CHARSET =>
            {
                ColumnKind::Binary
            }
            _ => ColumnKind::Text,
        }
    }
}

/// A value as SQL literal text.
pub(crate) fn sql_value(value: &Value, kind: ColumnKind) -> String {
    match value {
        Value::NULL => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) if f.is_finite() => f.to_string(),
        Value::Double(f) if f.is_finite() => f.to_string(),
        Value::Float(_) | Value::Double(_) => "NULL".to_string(),
        Value::Bytes(bytes) => match kind {
            ColumnKind::Number if is_plain_number(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            ColumnKind::Binary => hex_literal(bytes),
            _ => match std::str::from_utf8(bytes) {
                Ok(text) => quote_string(text),
                // Not valid UTF-8 in a text column: keep the exact bytes.
                Err(_) => hex_literal(bytes),
            },
        },
        Value::Date(y, m, d, h, mi, s, us) => {
            if *us > 0 {
                format!("'{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}'")
            } else {
                format!("'{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}'")
            }
        }
        Value::Time(negative, days, h, mi, s, us) => {
            let sign = if *negative { "-" } else { "" };
            let hours = u64::from(*days) * 24 + u64::from(*h);
            if *us > 0 {
                format!("'{sign}{hours:02}:{mi:02}:{s:02}.{us:06}'")
            } else {
                format!("'{sign}{hours:02}:{mi:02}:{s:02}'")
            }
        }
    }
}

fn is_plain_number(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.iter().any(u8::is_ascii_digit)
        && bytes.iter().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E'))
}

/// `X'…'` (an empty value is `''`, which is the empty binary string too).
pub(crate) fn hex_literal(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "''".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2 + 3);
    out.push_str("X'");
    for b in bytes {
        out.push_str(&format!("{b:02X}"));
    }
    out.push('\'');
    out
}

/// A single-quoted string literal escaped like `mysql_real_escape_string` (backslash escapes;
/// the restore script turns NO_BACKSLASH_ESCAPES off through its SQL_MODE line).
pub(crate) fn quote_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x1a' => out.push_str("\\Z"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// `` `name` `` with backticks doubled.
pub(crate) fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// Writes rows as `REPLACE INTO` statements of at most [`BATCH_ROWS`] rows / about
/// [`BATCH_BYTES`] bytes.
pub(crate) struct InsertWriter<'a, W: Write> {
    out: &'a mut W,
    prefix: String,
    rows_in_statement: usize,
    bytes_in_statement: usize,
    total: usize,
    max_rows: usize,
    max_bytes: usize,
}

impl<'a, W: Write> InsertWriter<'a, W> {
    pub(crate) fn new(out: &'a mut W, table: &str, columns: &[String]) -> Self {
        Self::with_limits(out, table, columns, BATCH_ROWS, BATCH_BYTES)
    }

    pub(crate) fn with_limits(out: &'a mut W, table: &str, columns: &[String], max_rows: usize, max_bytes: usize) -> Self {
        let columns: Vec<String> = columns.iter().map(|c| quote_ident(c)).collect();
        let prefix = format!("REPLACE INTO {} ({}) VALUES\n", quote_ident(table), columns.join(","));
        Self { out, prefix, rows_in_statement: 0, bytes_in_statement: 0, total: 0, max_rows: max_rows.max(1), max_bytes }
    }

    pub(crate) fn push_row(&mut self, values: &[String]) -> std::io::Result<()> {
        let row = format!("({})", values.join(","));
        if self.rows_in_statement == 0 {
            self.out.write_all(self.prefix.as_bytes())?;
            self.bytes_in_statement = self.prefix.len();
        } else {
            self.out.write_all(b",\n")?;
        }
        self.out.write_all(row.as_bytes())?;
        self.rows_in_statement += 1;
        self.bytes_in_statement += row.len() + 2;
        self.total += 1;
        if self.rows_in_statement >= self.max_rows || self.bytes_in_statement >= self.max_bytes {
            self.end_statement()?;
        }
        Ok(())
    }

    fn end_statement(&mut self) -> std::io::Result<()> {
        if self.rows_in_statement > 0 {
            self.out.write_all(b";\n")?;
            self.rows_in_statement = 0;
            self.bytes_in_statement = 0;
        }
        Ok(())
    }

    /// Closes the open statement and returns the number of rows written.
    pub(crate) fn finish(mut self) -> std::io::Result<usize> {
        self.end_statement()?;
        Ok(self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_escaping() {
        assert_eq!(quote_string("plain"), "'plain'");
        assert_eq!(quote_string("it's"), r"'it\'s'");
        assert_eq!(quote_string(r"a\b\"), r"'a\\b\\'");
        assert_eq!(quote_string(r#"say "hi""#), r#"'say \"hi\"'"#);
        assert_eq!(quote_string("l1\nl2\r\n"), r"'l1\nl2\r\n'");
        assert_eq!(quote_string("nul\0end\x1a"), r"'nul\0end\Z'");
        assert_eq!(quote_string("Ünïcødé ☕ 🪑"), "'Ünïcødé ☕ 🪑'");
        assert_eq!(quote_string(""), "''");
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
    }

    #[test]
    fn values() {
        let b = |s: &[u8]| Value::Bytes(s.to_vec());
        assert_eq!(sql_value(&Value::NULL, ColumnKind::Text), "NULL");
        assert_eq!(sql_value(&Value::NULL, ColumnKind::Number), "NULL");
        assert_eq!(sql_value(&b(b"42"), ColumnKind::Number), "42");
        assert_eq!(sql_value(&b(b"-1.50"), ColumnKind::Number), "-1.50");
        assert_eq!(sql_value(&b(b"1e-5"), ColumnKind::Number), "1e-5");
        // Anything odd in a numeric column is still kept, just quoted.
        assert_eq!(sql_value(&b(b"1;DROP"), ColumnKind::Number), "'1;DROP'");
        assert_eq!(sql_value(&b(b"42"), ColumnKind::Text), "'42'");
        assert_eq!(sql_value(&b(b"0"), ColumnKind::Text), "'0'");
        assert_eq!(sql_value(&b(b"\x00\xffA'"), ColumnKind::Binary), "X'00FF4127'");
        assert_eq!(sql_value(&b(b""), ColumnKind::Binary), "''");
        // Invalid UTF-8 in a text column keeps its bytes.
        assert_eq!(sql_value(&b(b"\xe9t\xe9"), ColumnKind::Text), "X'E974E9'");
        assert_eq!(sql_value(&Value::Int(-7), ColumnKind::Number), "-7");
        assert_eq!(sql_value(&Value::UInt(u64::MAX), ColumnKind::Number), "18446744073709551615");
        assert_eq!(sql_value(&Value::Double(0.1), ColumnKind::Number), "0.1");
        assert_eq!(sql_value(&Value::Date(2024, 1, 2, 3, 4, 5, 0), ColumnKind::Text), "'2024-01-02 03:04:05'");
        assert_eq!(sql_value(&Value::Date(2024, 1, 2, 3, 4, 5, 60), ColumnKind::Text), "'2024-01-02 03:04:05.000060'");
        assert_eq!(sql_value(&Value::Time(true, 1, 2, 3, 4, 0), ColumnKind::Text), "'-26:03:04'");
    }

    #[test]
    fn column_kinds() {
        let col = |t: ColumnType, cs: u16| Column::new(t).with_character_set(cs);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_LONG, 63)), ColumnKind::Number);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_NEWDECIMAL, 63)), ColumnKind::Number);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_VAR_STRING, 45)), ColumnKind::Text);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_BLOB, 45)), ColumnKind::Text);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_BLOB, 63)), ColumnKind::Binary);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_STRING, 63)), ColumnKind::Binary);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_BIT, 63)), ColumnKind::Binary);
        assert_eq!(ColumnKind::of(&col(ColumnType::MYSQL_TYPE_DATETIME, 63)), ColumnKind::Text);
    }

    #[test]
    fn batching() {
        let mut out = Vec::new();
        let columns = vec!["id".to_string(), "name".to_string()];
        let mut writer = InsertWriter::with_limits(&mut out, "items_base", &columns, 2, usize::MAX);
        for i in 1..=5 {
            writer.push_row(&[i.to_string(), quote_string(&format!("n{i}"))]).unwrap();
        }
        assert_eq!(writer.finish().unwrap(), 5);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "REPLACE INTO `items_base` (`id`,`name`) VALUES\n(1,'n1'),\n(2,'n2');\n\
REPLACE INTO `items_base` (`id`,`name`) VALUES\n(3,'n3'),\n(4,'n4');\n\
REPLACE INTO `items_base` (`id`,`name`) VALUES\n(5,'n5');\n"
        );

        // Byte limit closes statements early; an empty table writes nothing.
        let mut out = Vec::new();
        let mut writer = InsertWriter::with_limits(&mut out, "t", &columns, 500, 10);
        writer.push_row(&["1".into(), "'a'".into()]).unwrap();
        writer.push_row(&["2".into(), "'b'".into()]).unwrap();
        writer.finish().unwrap();
        assert_eq!(String::from_utf8(out).unwrap().matches("REPLACE INTO").count(), 2);
        let mut out = Vec::new();
        assert_eq!(InsertWriter::new(&mut out, "t", &columns).finish().unwrap(), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn default_batch_is_500_rows() {
        let mut out = Vec::new();
        let columns = vec!["id".to_string()];
        let mut writer = InsertWriter::new(&mut out, "t", &columns);
        for i in 0..1001 {
            writer.push_row(&[i.to_string()]).unwrap();
        }
        writer.finish().unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("REPLACE INTO").count(), 3);
        assert_eq!(text.matches(";\n").count(), 3);
    }

    #[test]
    fn file_names() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap().and_hms_opt(9, 5, 7).unwrap();
        assert_eq!(backup_file_name("fix_item_settings", at), "fix_item_settings_2026-10-31_09-05-07.sql");
        assert_eq!(backup_file_name("set offer/id", at), "set_offer_id_2026-10-31_09-05-07.sql");
        assert_eq!(backup_file_name("", at), "backup_2026-10-31_09-05-07.sql");

        let dir = tempfile::tempdir().unwrap();
        let name = backup_file_name("op", at);
        assert_eq!(unique_path(dir.path(), &name), dir.path().join(&name));
        std::fs::write(dir.path().join(&name), "").unwrap();
        assert_eq!(unique_path(dir.path(), &name), dir.path().join("op_2026-10-31_09-05-07_2.sql"));
        std::fs::write(dir.path().join("op_2026-10-31_09-05-07_2.sql"), "").unwrap();
        assert_eq!(unique_path(dir.path(), &name), dir.path().join("op_2026-10-31_09-05-07_3.sql"));
    }

    #[test]
    fn header_and_restore_command() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap().and_hms_opt(9, 5, 7).unwrap();
        let header = Header {
            label: "set_offer_id",
            created: at,
            server: "127.0.0.1",
            port: 3306,
            user: "root",
            database: "arcturus",
            version: "12.3.3-MariaDB",
            tables: &["catalog_items"],
            file_name: "set_offer_id_2026-10-31_09-05-07.sql",
        }
        .render();
        assert!(header.starts_with("-- Habbo all-in-1 converter: automatic backup before \"set_offer_id\"\n"));
        assert!(header.contains("-- Restore:  mysql -h 127.0.0.1 -P 3306 -u root -p arcturus < \"set_offer_id_2026-10-31_09-05-07.sql\"\n"));
        assert!(header.contains("SET NAMES utf8mb4;\n"));
        assert!(header.contains("SQL_MODE='NO_AUTO_VALUE_ON_ZERO'"));
        assert!(!header.contains("DELETE"));
        assert!(!FOOTER.contains("DELETE"));
    }
}
