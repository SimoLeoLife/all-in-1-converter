//! "Import new Habbo furniture": the furni Habbo added since the hotel's last import, in one go.
//!
//! 1. Downloads the current FurnitureData (the furnidata downloader).
//! 2. Finds the new furni: furnidata classnames that are not an `items_base.item_name` in the
//!    configured database (case-insensitive). A colour variant ("name*3") is its own
//!    items_base row (the SQL generator and FixSpriteID use the full classname), so it is
//!    compared by its full classname; its bundle is the one of "name". Without a database the
//!    hof_furni files can be used instead: then a furni is new when no .swf/.hab of its base
//!    name is in Habbo_Default/hof_furni.
//! 3. Asks before importing, downloads only those bundles (and icons) like the furniture
//!    downloader, copies them into Generate/default_furniture and FurnitureData.json into
//!    Generate/Furnidata, and runs the SQL generator (Standaard) for just those classnames.
//!
//! The database is only read; the SQL is written to Generate/Output_SQL for the owner to import.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use anyhow::Result;
use habbo_core::console::{self, n0};
use habbo_core::paths::{self, hof_furni, HabboAssetFolder};
use habbo_core::{outc, outln, settings, Color};
use habbo_db::sql_generator::{self, FurnitureSqlTarget};
use habbo_downloads::{asset_downloader, common, furnidata, furniture, xml::Document};
use mysql_async::prelude::Queryable;

/// How many new names the summary lists before "... and N more".
pub const SUMMARY_NAMES: usize = 30;

/// The library (bundle) name of a furnidata classname: "name*3" -> "name".
pub fn base_name(classname: &str) -> &str {
    classname.split('*').next().unwrap_or("")
}

/// The classnames, first spelling kept, without names that differ only in case.
fn distinct_ignore_case<'a>(classnames: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    classnames.into_iter().filter(|name| !name.is_empty() && seen.insert(name.to_lowercase())).map(str::to_string).collect()
}

/// The furni to import: their classnames (furnidata order) and the bundles they need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewFurniture {
    /// Furnidata classnames, variants included ("name", "name*1", ...).
    pub classnames: Vec<String>,
    /// The distinct base names of [`Self::classnames`]: one .swf/.hab each.
    pub libraries: Vec<String>,
}

impl NewFurniture {
    fn from_classnames(classnames: Vec<String>) -> Self {
        let libraries = distinct_ignore_case(classnames.iter().map(|c| base_name(c)));
        Self { classnames, libraries }
    }

    /// The classnames in lower case, for the SQL generator filter.
    pub fn classname_set(&self) -> HashSet<String> {
        self.classnames.iter().map(|c| c.to_lowercase()).collect()
    }
}

/// The furnidata classnames that are not an items_base `item_name` (both compared case-insensitively).
pub fn new_by_item_names<'a>(classnames: &[String], item_names: impl IntoIterator<Item = &'a str>) -> NewFurniture {
    let existing: HashSet<String> = item_names.into_iter().map(|n| n.trim().to_lowercase()).collect();
    let new = distinct_ignore_case(classnames.iter().map(String::as_str)).into_iter().filter(|c| !existing.contains(&c.to_lowercase())).collect();
    NewFurniture::from_classnames(new)
}

/// The furnidata classnames whose bundle (base name) is not among these file stems.
pub fn new_by_file_stems<'a>(classnames: &[String], file_stems: impl IntoIterator<Item = &'a str>) -> NewFurniture {
    let existing: HashSet<String> = file_stems.into_iter().map(str::to_lowercase).collect();
    let new = distinct_ignore_case(classnames.iter().map(String::as_str))
        .into_iter()
        .filter(|c| !existing.contains(&base_name(c).to_lowercase()))
        .collect();
    NewFurniture::from_classnames(new)
}

/// The suggested starting id for the generated rows: one past the highest id of items_base and
/// catalog_items (the generator gives an items_base row and its catalog row the same id).
pub fn suggest_start_id(max_items_base_id: Option<i64>, max_catalog_items_id: Option<i64>) -> i64 {
    max_items_base_id.unwrap_or(0).max(max_catalog_items_id.unwrap_or(0)).max(0) + 1
}

/// The bundle files of these libraries among `available` (a match on the file stem, ignoring
/// case; .swf and .hab only), and the libraries without any file.
pub fn select_bundle_files(libraries: &[String], available: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let wanted: HashSet<String> = libraries.iter().map(|l| l.to_lowercase()).collect();
    let mut found = HashSet::new();
    let mut files = Vec::new();
    for file in available {
        if !(paths::has_ext(file, "swf") || paths::has_ext(file, "hab")) {
            continue;
        }
        let stem = paths::file_stem(file).to_lowercase();
        if wanted.contains(&stem) {
            found.insert(stem);
            files.push(file.clone());
        }
    }
    let missing = libraries.iter().filter(|l| !found.contains(&l.to_lowercase())).cloned().collect();
    (files, missing)
}

/// The classnames without an items_base row in the generated SQL text.
pub fn classnames_missing_from_sql(classnames: &[String], sql: &str) -> Vec<String> {
    classnames.iter().filter(|c| !sql.contains(&format!(", '{c}', '{c}', "))).cloned().collect()
}

/// "Y" (any case, spaces ignored) is yes, everything else no.
pub fn is_yes(answer: &str) -> bool {
    answer.trim().eq_ignore_ascii_case("y")
}

fn ask_yes_no(question: &str) -> bool {
    outln!("❓ {question} (Y/N): ");
    is_yes(&console::read_line())
}

fn print_names(names: &[String]) {
    for name in names.iter().take(SUMMARY_NAMES) {
        outln!("   • {name}");
    }
    if names.len() > SUMMARY_NAMES {
        outln!("   ... and {} more", n0(names.len() as i64 - SUMMARY_NAMES as i64));
    }
}

/// What the configured database holds.
#[derive(Debug, Clone, Default)]
pub struct DatabaseSnapshot {
    pub item_names: Vec<String>,
    pub max_items_base_id: Option<i64>,
    pub max_catalog_items_id: Option<i64>,
}

/// Reads the item names and highest ids (read-only queries).
pub async fn read_database() -> Result<DatabaseSnapshot> {
    let mut connection = habbo_db::config::connect().await?;
    let result = async {
        let item_names: Vec<Option<String>> = connection.query("SELECT item_name FROM items_base").await?;
        let max_items_base_id: Option<Option<i64>> = connection.query_first("SELECT MAX(id) FROM items_base").await?;
        // A hotel without catalog_items still gets a suggestion from items_base.
        let max_catalog_items_id: Option<Option<i64>> = connection.query_first("SELECT MAX(id) FROM catalog_items").await.unwrap_or(None);
        anyhow::Ok(DatabaseSnapshot {
            item_names: item_names.into_iter().flatten().collect(),
            max_items_base_id: max_items_base_id.flatten(),
            max_catalog_items_id: max_catalog_items_id.flatten(),
        })
    }
    .await;
    let _ = connection.disconnect().await;
    result
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The stems of the .swf/.hab files in hof_furni (swf/, hab/ and a download from before them).
fn hof_furni_stems() -> Vec<String> {
    let mut stems = Vec::new();
    for (dir, ext) in [(hof_furni::swf(), "swf"), (hof_furni::hab(), "hab"), (hof_furni::root(), "swf"), (hof_furni::root(), "hab")] {
        stems.extend(paths::files_with_ext(&dir, ext).iter().map(|f| paths::file_stem(f)));
    }
    stems
}

/// The downloaded bundles in the configured download_format.
fn downloaded_bundles() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if settings::download_swf() {
        files.extend(paths::files_with_ext(&hof_furni::swf(), "swf"));
    }
    if settings::download_hab() {
        files.extend(paths::files_with_ext(&hof_furni::hab(), "hab"));
    }
    files
}

fn clear_folder(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// "Import new Habbo furniture": downloads the furni Habbo added since the last import and
/// writes their SQL (Generate/Output_SQL). The database itself is never modified.
pub async fn import_new_furniture() -> Result<()> {
    outc!(Color::Cyan, "🚀 Import new Habbo furniture");
    outln!("This downloads the furni Habbo added since your last import and writes their SQL. Your database is only read.");
    outln!();

    // 1. The current FurnitureData.
    let xml_path = Path::new(furnidata::XML_PATH);
    let json_path = furnidata::furniture_data_json_path();
    // The start-up folders, in case this runs before them (the downloader expects them).
    for dir in [xml_path.parent(), json_path.parent()].into_iter().flatten() {
        std::fs::create_dir_all(dir)?;
    }
    let before = (modified(xml_path), modified(&json_path));
    furnidata::download_furnidata().await?;
    let after = (modified(xml_path), modified(&json_path));
    if after.0.is_none() || after.1.is_none() || after.0 == before.0 || after.1 == before.1 {
        outc!(Color::Red, "❌ The current FurnitureData could not be downloaded from Habbo; nothing was imported.");
        return Ok(());
    }
    let Some(entries) = furniture::furniture_entries(&Document::load(xml_path)?)? else {
        outc!(Color::Red, "❌ Error: Invalid furnidata XML format; nothing was imported.");
        return Ok(());
    };
    let classnames: Vec<String> = entries.iter().map(|(c, _)| c.clone()).collect();
    outln!();

    // 2. Which furni are new.
    let database = habbo_db::config::get();
    outln!("🔌 Reading items_base from the database {}:{}/{}...", database.database_server, database.port, database.database_name);
    let (snapshot, new) = match read_database().await {
        Ok(snapshot) => {
            outc!(Color::Green, "✅ Loaded {} items from items_base.", n0(snapshot.item_names.len() as i64));
            let new = new_by_item_names(&classnames, snapshot.item_names.iter().map(String::as_str));
            (Some(snapshot), new)
        }
        Err(error) => {
            outc!(Color::Red, "❌ Could not read items_base from the database: {error}");
            if !ask_yes_no("Compare with the furni files already in Habbo_Default/hof_furni instead?") {
                outln!("❎ Nothing was imported.");
                return Ok(());
            }
            let stems = hof_furni_stems();
            outc!(Color::Green, "✅ Found {} furni files in Habbo_Default/hof_furni.", n0(stems.len() as i64));
            (None, new_by_file_stems(&classnames, stems.iter().map(String::as_str)))
        }
    };

    // 3. Summary and confirmation.
    let all_libraries = distinct_ignore_case(classnames.iter().map(|c| base_name(c)));
    outln!();
    outc!(
        Color::Cyan,
        "📊 FurnitureData: {} furni ({} bundles). New: {} furni ({} bundles to download).",
        n0(distinct_ignore_case(classnames.iter().map(String::as_str)).len() as i64),
        n0(all_libraries.len() as i64),
        n0(new.classnames.len() as i64),
        n0(new.libraries.len() as i64)
    );
    if new.classnames.is_empty() {
        outc!(Color::Green, "🎉 No new furniture: your hotel already has every furni in Habbo's FurnitureData.");
        return Ok(());
    }
    print_names(&new.classnames);
    if !ask_yes_no(&format!("Import {} new furniture?", n0(new.classnames.len() as i64))) {
        outln!("❎ Import cancelled; nothing was downloaded.");
        return Ok(());
    }
    outln!();

    // 4. Download only those bundles (and their icons), like the furniture downloader.
    let config = common::load_config()?;
    let furniture_url = common::cfg(&config, "AppSettings:furnitureurl")?.trim_end_matches('/').to_string();
    outln!("Furniture download format: {} (config.ini download_format)", settings::download_format());
    std::fs::create_dir_all(hof_furni::icons())?;
    asset_downloader::prepare_folder(HabboAssetFolder::FURNITURE).await?;

    let wanted_libraries: HashSet<String> = new.libraries.iter().map(|l| l.to_lowercase()).collect();
    // Every entry of a new bundle, so its URL uses the group's highest revision.
    let library_entries: Vec<(String, i32)> = entries.iter().filter(|(c, _)| wanted_libraries.contains(&base_name(c).to_lowercase())).cloned().collect();
    let libraries = furniture::furniture_libraries(&library_entries, &furniture_url);
    outln!("📥 Downloading {} furni bundles into {}...", n0(libraries.len() as i64), hof_furni::root().display());
    let counts = asset_downloader::download_libraries(HabboAssetFolder::FURNITURE, libraries).await?;
    let wanted_classnames = new.classname_set();
    let icon_entries: Vec<(String, i32)> = entries.iter().filter(|(c, _)| wanted_classnames.contains(&c.to_lowercase())).cloned().collect();
    let icons = furniture::download_icons(&icon_entries, &furniture_url).await?;
    outc!(Color::Green, "{}", asset_downloader::summary(&format!("furniture files and {icons} new icons"), &counts));

    let (files, missing) = select_bundle_files(&new.libraries, &downloaded_bundles());
    if !missing.is_empty() {
        outc!(Color::Yellow, "⚠️ {} furni bundles could not be downloaded (they get no SQL):", n0(missing.len() as i64));
        print_names(&missing);
    }
    if files.is_empty() {
        outc!(Color::Red, "❌ None of the new furni could be downloaded; no SQL was generated.");
        return Ok(());
    }

    // Into Generate/ for the SQL generator.
    let generate_dir = std::env::current_dir()?.join("Generate");
    std::fs::create_dir_all(&generate_dir)?;
    sql_generator::move_legacy_furniture_folder(&generate_dir)?;
    let furniture_dir = generate_dir.join(sql_generator::furniture_folder(FurnitureSqlTarget::Standard));
    std::fs::create_dir_all(&furniture_dir)?;
    let existing = std::fs::read_dir(&furniture_dir)?.count();
    if existing > 0 {
        if ask_yes_no(&format!("Generate/default_furniture/ already holds {} files or folders. Clear it before copying the new furni?", n0(existing as i64))) {
            clear_folder(&furniture_dir)?;
            outln!("🧹 Cleared Generate/default_furniture/.");
        } else {
            outln!("ℹ️ The files already there stay; only the new furni get SQL rows.");
        }
    }
    for file in &files {
        std::fs::copy(file, furniture_dir.join(paths::file_name(file)))?;
    }
    let furnidata_dir = generate_dir.join("Furnidata");
    std::fs::create_dir_all(&furnidata_dir)?;
    std::fs::copy(&json_path, furnidata_dir.join(habbo_core::json::FURNIDATA_FILE))?;
    outc!(Color::Green, "📁 Copied {} bundles into Generate/default_furniture/ and FurnitureData.json into Generate/Furnidata/.", n0(files.len() as i64));
    outln!();

    // 5. The SQL, for the new classnames only.
    match &snapshot {
        Some(snapshot) => outc!(
            Color::Cyan,
            "💡 Suggested starting ID: {} (MAX(id) of items_base is {}, of catalog_items {}, + 1).",
            suggest_start_id(snapshot.max_items_base_id, snapshot.max_catalog_items_id),
            snapshot.max_items_base_id.map_or("none".to_string(), |id| id.to_string()),
            snapshot.max_catalog_items_id.map_or("none".to_string(), |id| id.to_string())
        ),
        None => outc!(Color::Yellow, "💡 Use MAX(id) + 1 of items_base and catalog_items as the starting ID (the database could not be read)."),
    }
    let Some(sql_path) = sql_generator::generate_sql_only(FurnitureSqlTarget::Standard, Some(&wanted_classnames)).await? else {
        outc!(Color::Red, "❌ No SQL was generated (see the messages above).");
        return Ok(());
    };

    // 6. What was done.
    let sql = std::fs::read_to_string(&sql_path).unwrap_or_default();
    let without_rows = classnames_missing_from_sql(&new.classnames, &sql);
    let rows = new.classnames.len() - without_rows.len();
    outln!();
    outc!(Color::Green, "🎉 Import of new Habbo furniture finished!");
    outln!(
        "   📥 Downloaded {} new .swf, {} new .hab and {icons} icons into {}",
        counts.swf.load(Ordering::SeqCst),
        counts.hab.load(Ordering::SeqCst),
        hof_furni::root().display()
    );
    outln!("   📁 {} bundles in Generate/default_furniture/, FurnitureData.json in Generate/Furnidata/", n0(files.len() as i64));
    outln!("   📦 SQL for {} of {} new furni: {}", n0(rows as i64), n0(new.classnames.len() as i64), sql_path.display());
    if !without_rows.is_empty() {
        outc!(Color::Yellow, "⚠️ No SQL row for {} new furni:", n0(without_rows.len() as i64));
        print_names(&without_rows);
    }
    outc!(Color::Yellow, "ℹ️ Your database was not changed: check the SQL file and import it into your database yourself.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn base_names() {
        assert_eq!(base_name("chair*3"), "chair");
        assert_eq!(base_name("chair"), "chair");
        assert_eq!(base_name("odd*"), "odd");
    }

    #[test]
    fn diff_with_item_names_is_per_classname_and_ignores_case() {
        let furnidata = names(&["chair", "chair*1", "Chair*2", "table", "poster*5", "Lamp", "lamp"]);
        let new = new_by_item_names(&furnidata, ["CHAIR", "chair*1", "Table ", "poster*4"]);
        // chair*2 is a new colour of a furni the hotel has; Lamp/lamp is one furni.
        assert_eq!(new.classnames, names(&["Chair*2", "poster*5", "Lamp"]));
        assert_eq!(new.libraries, names(&["Chair", "poster", "Lamp"]));
        assert_eq!(new.classname_set(), ["chair*2", "poster*5", "lamp"].iter().map(|s| s.to_string()).collect());

        let none = new_by_item_names(&names(&["a", "a*1"]), ["A", "A*1"]);
        assert_eq!(none, NewFurniture::default());
        let all = new_by_item_names(&names(&["a*1", "a*2"]), std::iter::empty());
        assert_eq!(all.classnames, names(&["a*1", "a*2"]));
        assert_eq!(all.libraries, names(&["a"]));
    }

    #[test]
    fn diff_with_files_is_per_bundle() {
        let furnidata = names(&["chair", "chair*1", "table", "poster*5", "poster*6"]);
        let new = new_by_file_stems(&furnidata, ["CHAIR", "rug"]);
        assert_eq!(new.classnames, names(&["table", "poster*5", "poster*6"]));
        assert_eq!(new.libraries, names(&["table", "poster"]));
    }

    #[test]
    fn start_id_suggestion() {
        assert_eq!(suggest_start_id(Some(100), Some(250)), 251);
        assert_eq!(suggest_start_id(Some(300), Some(250)), 301);
        assert_eq!(suggest_start_id(Some(7), None), 8);
        assert_eq!(suggest_start_id(None, None), 1);
        assert_eq!(suggest_start_id(Some(-5), None), 1);
    }

    #[test]
    fn bundle_selection() {
        let available: Vec<PathBuf> = ["hab/Chair.hab", "swf/chair.swf", "hab/table.hab", "hab/poster.hab.part", "hab/rug.hab", "x/poster.txt"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let (files, missing) = select_bundle_files(&names(&["chair", "poster", "table"]), &available);
        assert_eq!(files, ["hab/Chair.hab", "swf/chair.swf", "hab/table.hab"].iter().map(PathBuf::from).collect::<Vec<_>>());
        assert_eq!(missing, names(&["poster"]));
    }

    #[test]
    fn sql_row_check_and_answers() {
        let sql = "INSERT INTO `items_base` (...) VALUES\n(5, 9, 'chair*2', 'chair*2', 1, 1, 0, '0');";
        assert_eq!(classnames_missing_from_sql(&names(&["chair*2", "chair"]), sql), names(&["chair"]));
        assert!(is_yes(" y "));
        assert!(is_yes("Y"));
        assert!(!is_yes("yes"));
        assert!(!is_yes(""));
    }
}
