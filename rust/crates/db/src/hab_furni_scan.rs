//! `HabFurniScan`: the shared part of the Database fixes that compare items_base with
//! Habbo_Default/hof_furni/hab: which rows they check, reading each furni's .hab json once,
//! and the list of .hab files that are missing or unreadable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use habbo_core::outln;
use habbo_core::paths::NEWLINE;
use rayon::prelude::*;
use serde_json::Value;

use crate::dotnet::{cmp_ignore_case, ignore_case_key, try_parse_int};

/// Floor (s) and wall (i) furni without multiheight; pets ("a0 pet0" ...) have no furni bundle.
pub const BASE_FILTER: &str = "(multiheight IS NULL OR multiheight IN ('', '0')) AND type IN ('s', 'i') AND item_name NOT LIKE 'a0%'";

pub fn hab_dir() -> PathBuf {
    habbo_core::paths::hof_furni::hab()
}

/// False (with a message) when the hab folder is not there.
pub fn check_hab_folder() -> bool {
    if hab_dir().is_dir() {
        return true;
    }
    outln!("⚠️ {} not found. Download the furniture as .hab first (Habbo Original -> Download furniture).", hab_dir().display());
    false
}

/// The bundle a classname draws from: the part before '*' (colour variants).
pub fn bundle_name(item_name: &str) -> String {
    item_name.split('*').next().unwrap_or("").trim().to_string()
}

/// What each bundle gave, keyed case-insensitively; `None` for a missing / unreadable bundle.
pub struct BundleResults<T> {
    results: HashMap<String, Option<T>>,
}

impl<T: Clone> BundleResults<T> {
    /// What a bundle gave for this item name, or `None` when it has no readable bundle.
    pub fn result_for(&self, item_name: &str) -> Option<T> {
        let name = bundle_name(item_name);
        if name.is_empty() {
            return None;
        }
        self.results.get(&ignore_case_key(&name)).cloned().flatten()
    }
}

/// Reads every bundle the item names need, once each (colour variants share their base
/// furni's bundle), in parallel over all cores, and keeps what `read` takes from its json.
pub async fn read_bundles<T, F>(item_names: Vec<String>, read: F) -> Result<BundleResults<T>>
where
    T: Send + 'static,
    F: Fn(&Value) -> Option<T> + Send + Sync + 'static,
{
    let mut seen = HashSet::new();
    let bundle_names: Vec<String> = item_names
        .iter()
        .map(|n| bundle_name(n))
        .filter(|n| !n.is_empty() && seen.insert(ignore_case_key(n)))
        .collect();

    let results = tokio::task::spawn_blocking(move || {
        let total = bundle_names.len();
        let done = AtomicUsize::new(0);
        let dir = hab_dir();
        bundle_names
            .par_iter()
            .map(|name| {
                let value = read_bundle(&dir.join(format!("{name}.hab")), &read);
                let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
                if finished.is_multiple_of(500) {
                    outln!("   {finished}/{total} bundles read...");
                }
                (ignore_case_key(name), value)
            })
            .collect::<HashMap<_, _>>()
    })
    .await?;
    Ok(BundleResults { results })
}

/// Only the json is needed: the bundle is read in memory, not unpacked to disk.
fn read_bundle<T>(hab_path: &Path, read: &impl Fn(&Value) -> Option<T>) -> Option<T> {
    if !hab_path.is_file() {
        return None;
    }
    let data = std::fs::read(hab_path).ok()?;
    if !habbo_nitro::hab::is_hab(&data) {
        return None;
    }
    let (_, files) = habbo_nitro::hab::read(&data).ok()?;
    let (_, content) = files.iter().find(|(name, _)| name.to_lowercase().ends_with(".json"))?;
    let content = content.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(content);
    let root: Value = serde_json::from_slice(content).ok()?;
    read(&root)
}

fn stamp(format: &str) -> String {
    chrono::Local::now().format(format).to_string()
}

/// Generate/<prefix>_<date>.sql and Generate/Missing_Hab_files_<date>.txt, with the same date.
pub fn output_paths(sql_prefix: &str) -> Result<(PathBuf, PathBuf)> {
    let generate_dir = std::env::current_dir()?.join("Generate");
    std::fs::create_dir_all(&generate_dir)?;
    let stamp = stamp("%Y-%m-%d_%H-%M-%S");
    Ok((generate_dir.join(format!("{sql_prefix}_{stamp}.sql")), generate_dir.join(format!("Missing_Hab_files_{stamp}.txt"))))
}

pub fn now() -> String {
    stamp("%Y-%m-%d %H:%M:%S")
}

/// `StringBuilder.AppendLine`.
pub(crate) fn append_line(text: &mut String, line: &str) {
    text.push_str(line);
    text.push_str(NEWLINE);
}

/// One line per .hab file that is needed, sorted: first the files not in the hab folder, then
/// the ones there but unreadable. Each line names the items_base rows that use the file.
pub fn missing_list_text(hab_dir: &Path, checked_rows: &str, item_names: &[String], now: &str) -> String {
    // GroupBy (case-insensitive, first spelling is the key), then a stable OrderBy.
    let mut keys: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for item_name in item_names {
        let file = format!("{}.hab", bundle_name(item_name));
        let folded = ignore_case_key(&file);
        groups
            .entry(folded)
            .or_insert_with(|| {
                keys.push(file);
                Vec::new()
            })
            .push(item_name.clone());
    }
    keys.sort_by(|a, b| cmp_ignore_case(a, b));
    let by_file: Vec<(String, &Vec<String>)> = keys.into_iter().map(|k| {
        let members = &groups[&ignore_case_key(&k)];
        (k, members)
    }).collect();

    let (not_found, unreadable): (Vec<_>, Vec<_>) = by_file.iter().partition(|(key, _)| !hab_dir.join(key).is_file());

    let mut text = String::new();
    append_line(&mut text, &format!("# .hab files needed by items_base ({checked_rows}), {now}"));
    let full = std::path::absolute(hab_dir).unwrap_or_else(|_| hab_dir.to_path_buf());
    append_line(&mut text, &format!("# Folder: {}", full.display()));
    append_line(
        &mut text,
        &format!("# {} missing, {} unreadable; {} items_base rows in total.", not_found.len(), unreadable.len(), item_names.len()),
    );
    append_line(&mut text, "");

    append_line(&mut text, &format!("## Missing ({})", not_found.len()));
    for (key, names) in &not_found {
        append_line(&mut text, &file_line(key, names));
    }

    if !unreadable.is_empty() {
        append_line(&mut text, "");
        append_line(&mut text, &format!("## In the folder but unreadable, or without the data this fix needs ({})", unreadable.len()));
        for (key, names) in &unreadable {
            append_line(&mut text, &file_line(key, names));
        }
    }
    text
}

pub fn write_missing_list(path: &Path, checked_rows: &str, item_names: &[String]) -> Result<()> {
    std::fs::write(path, missing_list_text(&hab_dir(), checked_rows, item_names, &now()))?;
    Ok(())
}

/// "name.hab" alone, or with the classnames when colour variants use it ("name.hab  (name*1, name*2)").
fn file_line(key: &str, group: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut names: Vec<&String> = group.iter().filter(|n| seen.insert(ignore_case_key(n))).collect();
    names.sort_by(|a, b| variant_number(a).cmp(&variant_number(b)).then_with(|| cmp_ignore_case(a, b)));
    let only_itself = names.len() == 1 && cmp_ignore_case(&format!("{}.hab", names[0]), key).is_eq();
    if only_itself {
        key.to_string()
    } else {
        format!("{key}  ({})", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
    }
}

/// The colour number of "name*3" (3), -1 for the plain name, so variants list in number order.
fn variant_number(item_name: &str) -> i32 {
    match item_name.find('*') {
        Some(star) => try_parse_int(&item_name[star + 1..]).unwrap_or(-1),
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_names() {
        assert_eq!(bundle_name("chair*3"), "chair");
        assert_eq!(bundle_name(" chair "), "chair");
        assert_eq!(bundle_name("*1"), "");
    }

    #[test]
    fn missing_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.hab"), b"x").unwrap();
        let names: Vec<String> = ["table", "chair*10", "chair*2", "Chair", "broken", "chair*2"].iter().map(|s| s.to_string()).collect();
        let text = missing_list_text(dir.path(), "rows", &names, "2026-01-02 03:04:05");
        let nl = NEWLINE;
        let full = std::path::absolute(dir.path()).unwrap();
        let expected = [
            "# .hab files needed by items_base (rows), 2026-01-02 03:04:05".to_string(),
            format!("# Folder: {}", full.display()),
            "# 2 missing, 1 unreadable; 6 items_base rows in total.".into(),
            String::new(),
            "## Missing (2)".into(),
            "chair.hab  (Chair, chair*2, chair*10)".into(),
            "table.hab".into(),
            String::new(),
            "## In the folder but unreadable, or without the data this fix needs (1)".into(),
            "broken.hab".into(),
            String::new(),
        ]
        .join(nl);
        assert_eq!(text, expected);
    }
}
