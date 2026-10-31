//! Real round trip against a throwaway MySQL / MariaDB server (ignored by default):
//! fixture tables with tricky values -> `backup_tables` -> wipe / alter the rows -> restore the
//! file with the `mysql` / `mariadb` client -> every row must be identical.
//!
//! ```text
//! DBX_PORT=3398 DBX_PASSWORD=test DBX_CLIENT=E:/laragon/bin/mysql/mariadb-12.3.3-winx64/bin/mariadb.exe \
//!   cargo test -p habbo-db --test backup_roundtrip -- --ignored --nocapture
//! ```
//! Settings: DBX_HOST (127.0.0.1), DBX_PORT (3306), DBX_USER (root), DBX_PASSWORD (empty),
//! DBX_CLIENT (`mysql`). The test creates and finally drops the database `arcturus_dbx`.

use std::process::{Command, Stdio};

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};

const FIXTURE: &str = include_str!("fixtures/backup_roundtrip.sql");
const DATABASE: &str = "arcturus_dbx";

struct Settings {
    host: String,
    port: u16,
    user: String,
    password: String,
    client: String,
}

fn settings() -> Settings {
    let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_string());
    Settings {
        host: var("DBX_HOST", "127.0.0.1"),
        port: var("DBX_PORT", "3306").parse().expect("DBX_PORT"),
        user: var("DBX_USER", "root"),
        password: var("DBX_PASSWORD", ""),
        client: var("DBX_CLIENT", "mysql"),
    }
}

async fn connect(s: &Settings, database: Option<&str>) -> Conn {
    let opts = OptsBuilder::default()
        .ip_or_hostname(s.host.clone())
        .tcp_port(s.port)
        .user(Some(s.user.clone()))
        .pass(Some(s.password.clone()).filter(|p| !p.is_empty()))
        .db_name(database.map(str::to_string));
    Conn::new(opts).await.expect("connect")
}

/// Every row of the table as raw values (text protocol: exact bytes), ordered by id.
async fn snapshot(conn: &mut Conn, table: &str) -> Vec<Vec<Value>> {
    conn.query_drop("SET time_zone = '+00:00'").await.unwrap();
    let rows: Vec<Row> = conn.query(format!("SELECT * FROM `{table}` ORDER BY id")).await.unwrap();
    rows.into_iter().map(|r| r.unwrap()).collect()
}

fn restore(s: &Settings, file: &std::path::Path) {
    let input = std::fs::File::open(file).unwrap();
    let mut command = Command::new(&s.client);
    command.args(["-h", &s.host, "-P", &s.port.to_string(), "-u", &s.user, "--default-character-set=utf8mb4", DATABASE]);
    if !s.password.is_empty() {
        command.arg(format!("-p{}", s.password));
    }
    let output = command.stdin(Stdio::from(input)).output().expect("run the mysql client");
    assert!(output.status.success(), "restore failed: {}", String::from_utf8_lossy(&output.stderr));
}

#[tokio::test]
#[ignore = "needs a throwaway MySQL / MariaDB server (see the file header)"]
async fn backup_and_restore_round_trip() {
    let s = settings();
    let mut root = connect(&s, None).await;
    root.query_drop(FIXTURE).await.expect("load the fixture");
    drop(root);

    let mut conn = connect(&s, Some(DATABASE)).await;
    let items_before = snapshot(&mut conn, "items_base").await;
    let catalog_before = snapshot(&mut conn, "catalog_items").await;
    assert_eq!(items_before.len(), 1205);
    assert_eq!(catalog_before.len(), 1204);

    // The session time zone must not shift TIMESTAMP values, and is put back afterwards.
    conn.query_drop("SET time_zone = '+05:00'").await.unwrap();
    let folder = tempfile::tempdir().unwrap();
    // DBX_BACKUP_DIR keeps a copy of the backup for a manual restore with the mysql client.
    let keep = std::env::var("DBX_BACKUP_DIR").ok().map(std::path::PathBuf::from);
    let file = habbo_db::backup::backup_tables_to(&mut conn, &["items_base", "catalog_items"], "roundtrip", keep.as_deref().unwrap_or(folder.path()))
        .await
        .expect("backup");
    let zone: Option<String> = conn.query_first("SELECT @@SESSION.time_zone").await.unwrap();
    assert_eq!(zone.as_deref(), Some("+05:00"));
    let text = std::fs::read_to_string(&file).unwrap();
    println!("backup: {} ({} bytes)", file.display(), text.len());
    assert!(!text.contains("DELETE FROM"));
    assert_eq!(text.matches("REPLACE INTO `items_base`").count(), 3, "1205 rows / 500 per batch");

    // Damage: change values in every column kind, delete rows, wipe one table entirely.
    conn.query_drop(
        "UPDATE items_base SET public_name = 'changed', customparams = NULL, blobby = X'00', flags = b'1', price = 1, created_at = NOW(), sprite_id = sprite_id + 1;
         DELETE FROM items_base WHERE id IN (0, 3, 150);
         DELETE FROM catalog_items;",
    )
    .await
    .unwrap();
    assert_ne!(snapshot(&mut conn, "items_base").await, items_before);

    restore(&s, &file);

    let items_after = snapshot(&mut conn, "items_base").await;
    let catalog_after = snapshot(&mut conn, "catalog_items").await;
    assert_eq!(items_after.len(), items_before.len());
    for (before, after) in items_before.iter().zip(&items_after) {
        assert_eq!(before, after);
    }
    assert_eq!(catalog_after, catalog_before);

    // Restoring twice is harmless.
    restore(&s, &file);
    assert_eq!(snapshot(&mut conn, "items_base").await, items_before);

    // A missing table makes the backup fail without leaving a file behind.
    let empty = tempfile::tempdir().unwrap();
    let error = habbo_db::backup::backup_tables_to(&mut conn, &["no_such_table"], "fail", empty.path()).await.unwrap_err();
    println!("missing table: {error:#}");
    let leftovers: Vec<_> = std::fs::read_dir(empty.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // test_connection
    let port = s.port.to_string();
    let ok = habbo_db::test_connection(&s.host, &port, &s.user, &s.password, DATABASE).await.unwrap();
    println!("{ok}");
    assert!(ok.starts_with("Connected: "));
    assert!(ok.ends_with(" · database arcturus_dbx · 1,205 items_base rows"), "{ok}");
    let wrong_password = habbo_db::test_connection(&s.host, &port, &s.user, "definitely-wrong", DATABASE).await.unwrap_err();
    println!("{wrong_password}");
    assert!(wrong_password.to_string().contains("wrong user name or password"));
    let unknown = habbo_db::test_connection(&s.host, &port, &s.user, &s.password, "no_such_db_dbx").await.unwrap_err();
    println!("{unknown}");
    assert!(unknown.to_string().starts_with("Unknown database 'no_such_db_dbx'"));
    let no_table = habbo_db::test_connection(&s.host, &port, &s.user, &s.password, "mysql").await.unwrap();
    println!("{no_table}");
    assert!(no_table.ends_with("no items_base table"));
    let closed = habbo_db::test_connection("127.0.0.1", "1", &s.user, &s.password, DATABASE).await.unwrap_err();
    println!("{closed}");
    let unreachable = habbo_db::test_connection("10.255.255.1", &port, &s.user, &s.password, DATABASE).await.unwrap_err();
    println!("{unreachable}");

    conn.query_drop(format!("DROP DATABASE `{DATABASE}`")).await.unwrap();
    let _ = conn.disconnect().await;
}

/// The three operations back up before changing anything, and change nothing when the backup
/// cannot be written. Runs in a temporary working directory (config.ini, FurnitureData.json).
#[tokio::test]
#[ignore = "needs a throwaway MySQL / MariaDB server (see the file header)"]
async fn operations_back_up_first() {
    const DB2: &str = "arcturus_dbx_ops";
    let s = settings();
    let mut root = connect(&s, None).await;
    root.query_drop(FIXTURE.replace(DATABASE, DB2)).await.expect("load the fixture");

    let work = tempfile::tempdir().unwrap();
    std::env::set_current_dir(work.path()).unwrap();
    std::fs::write(
        "config.ini",
        format!(
            "[AppSettings]\nDATABASESERVER={}\nDATABASEPORT={}\nDATABASEUSER={}\nDATABASEPASSWORD={}\nDATABASENAME={DB2}\n",
            s.host, s.port, s.user, s.password
        ),
    )
    .unwrap();
    std::fs::create_dir_all("Database/Variables").unwrap();
    std::fs::write(
        "Database/Variables/FurnitureData.json",
        r#"{"roomitemtypes":{"furnitype":[{"id":3001,"classname":"chair_norja","offerid":-1,"canstandon":true,"cansiton":false},{"id":4242,"classname":"table_multi","offerid":555}]},"wallitemtypes":{"furnitype":[{"id":3002,"classname":"wall_poster_dbx"}]}}"#,
    )
    .unwrap();

    assert_eq!(walk(&mut root, DB2).await, "0");

    // The backup folder cannot be created (a file is in the way): nothing may change.
    std::fs::write("Database/Backups", "not a folder").unwrap();
    habbo_db::fix_item_settings::run().await.unwrap();
    assert_eq!(walk(&mut root, DB2).await, "0", "changed without a backup");
    std::fs::remove_file("Database/Backups").unwrap();

    habbo_db::fix_item_settings::run().await.unwrap();
    assert_eq!(walk(&mut root, DB2).await, "1");
    habbo_db::fix_sprite_id_settings::run().await.unwrap();
    habbo_db::set_offer_id::run().await.unwrap();

    let mut names: Vec<String> = std::fs::read_dir("Database/Backups").unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    println!("{names:?}");
    assert_eq!(names.len(), 3);
    assert!(names[0].starts_with("fix_item_settings_"));
    assert!(names[1].starts_with("fix_sprite_id_settings_"));
    assert!(names[2].starts_with("set_offer_id_"));
    let sprite = std::fs::read_to_string(format!("Database/Backups/{}", names[1])).unwrap();
    assert!(sprite.contains("REPLACE INTO `items_base`") && sprite.contains("REPLACE INTO `catalog_items`"));
    let offer = std::fs::read_to_string(format!("Database/Backups/{}", names[2])).unwrap();
    assert!(!offer.contains("REPLACE INTO `items_base`") && offer.contains("REPLACE INTO `catalog_items`"));

    // Restoring the first backup undoes fix_item_settings.
    let file = work.path().join("Database/Backups").join(&names[0]);
    let input = std::fs::File::open(&file).unwrap();
    let mut command = Command::new(&s.client);
    command.args(["-h", &s.host, "-P", &s.port.to_string(), "-u", &s.user, DB2]);
    if !s.password.is_empty() {
        command.arg(format!("-p{}", s.password));
    }
    let output = command.stdin(Stdio::from(input)).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(walk(&mut root, DB2).await, "0");

    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    root.query_drop(format!("DROP DATABASE `{DB2}`")).await.unwrap();
    let _ = root.disconnect().await;
}

async fn walk(conn: &mut Conn, database: &str) -> String {
    let query = format!("SELECT CAST(allow_walk AS CHAR) FROM `{database}`.items_base WHERE sprite_id = 3001");
    conn.query_first::<String, _>(query).await.unwrap().unwrap()
}
