//! `NitroFurnitureDownloader` (NitroFurniture.cs): a custom Nitro hotel's furniture (.nitro +
//! icons) from its FurnitureData.json into custom_downloads/nitro_furniture (or the Asset
//! Workspace furniture folder).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use habbo_core::workspace::{self, runtime, WorkspaceAssetKind};
use habbo_core::{outln, Color};
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::common::{self, colored, FetchError, NULL_REFERENCE};
use crate::resume;

const MAX_CONCURRENCY: usize = 100;

#[derive(Deserialize)]
pub struct Furnidata {
    #[serde(rename = "roomitemtypes")]
    pub room_item_types: Option<ItemTypes>,
    #[serde(rename = "wallitemtypes")]
    pub wall_item_types: Option<ItemTypes>,
}

#[derive(Deserialize)]
pub struct ItemTypes {
    #[serde(rename = "furnitype")]
    pub furni_type: Option<Vec<FurniType>>,
}

#[derive(Deserialize)]
pub struct FurniType {
    pub classname: Option<String>,
}

/// The .nitro file name (base classname) and icon name ("*" -> "_") of a classname.
pub fn nitro_names(classname: &str) -> (String, String) {
    let base = classname.split('*').next().unwrap_or("").to_string();
    (base, format!("{}_icon.png", classname.replace('*', "_")))
}

type FileLocks = Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>;

/// `NitroFurnitureDownloader.DownloadFurnitureAsync`.
pub async fn download_nitro_furniture() -> Result<()> {
    let config = common::load_config()?;
    let furnidata_url = common::cfg(&config, "AppSettings:nitro_furnidataJSON")?;
    let furniture_url = common::cfg(&config, "AppSettings:nitro_furnitureurl")?;
    let furniture_icon_url = common::cfg(&config, "AppSettings:nitro_furniture_icon_url")?;
    let workspace = runtime::router();
    let furniture_directory = workspace.asset_directory(WorkspaceAssetKind::Furniture, "./custom_downloads/nitro_furniture");
    let icon_directory = PathBuf::from("./custom_downloads/nitro_furniture/icons");

    std::fs::create_dir_all("./temp")?;
    let furnidata_json_path = "./temp/furnidata.json";

    outln!("Downloading furnidata JSON file...");
    if let Err(error) = download_file(&furnidata_url, Path::new(furnidata_json_path), "furnidata.json", &resume::Options::FRESH).await {
        colored(Color::Red, &format!("Error downloading furnidata: {error}"));
        return Ok(());
    }
    outln!("Furnidata downloaded successfully.");

    // One lock per file of this run.
    let file_locks: FileLocks = Mutex::new(HashMap::new());

    let result: Result<()> = async {
        let furnidata_content = common::decode_text(&std::fs::read(furnidata_json_path)?);
        if workspace.is_configured() {
            let target = workspace.game_data_file("FurnitureData.json", furnidata_json_path);
            workspace::write_output(&target, furnidata_content.as_bytes())?;
            outln!("Workspace gamedata updated: {}", target.display());
        }
        let furnidata: Option<Furnidata> = serde_json::from_str(&furnidata_content)?;

        let (mut nitro_count, mut icon_count) = (0, 0);
        for section in furnidata.iter().flat_map(|f| [&f.room_item_types, &f.wall_item_types]) {
            if let Some(furni_types) = section.as_ref().and_then(|s| s.furni_type.as_ref()) {
                let (nitro, icons) = process_furni_types(
                    furni_types,
                    &furniture_url,
                    &furniture_icon_url,
                    &furniture_directory,
                    &icon_directory,
                    &file_locks,
                )
                .await?;
                nitro_count += nitro;
                icon_count += icons;
            }
        }

        habbo_core::console::set_color(Color::Green);
        outln!("Downloading furniture completed!");
        outln!("Downloaded {nitro_count} .nitro files and {icon_count} .png icons.");
        habbo_core::console::set_color(Color::Gray);
        Ok(())
    }
    .await;

    // finally
    let cleanup = common::clean_temp();
    result?;
    cleanup?;
    Ok(())
}

async fn process_furni_types(
    furni_types: &[FurniType],
    furniture_url: &str,
    furniture_icon_url: &str,
    furniture_directory: &Path,
    icon_directory: &Path,
    file_locks: &FileLocks,
) -> Result<(usize, usize)> {
    let nitro_count = AtomicUsize::new(0);
    let icon_count = AtomicUsize::new(0);
    let global = Semaphore::new(MAX_CONCURRENCY);

    let tasks = furni_types.iter().map(|furni| {
        let (nitro_count, icon_count, global) = (&nitro_count, &icon_count, &global);
        async move {
            let classname = furni.classname.as_deref().ok_or_else(|| anyhow!(NULL_REFERENCE))?;
            let (base, icon_name) = nitro_names(classname);
            let nitro_path = furniture_directory.join(format!("{base}.nitro"));
            let icon_path = icon_directory.join(&icon_name);
            let nitro_url = format!("{furniture_url}/{base}.nitro");
            let icon_url = format!("{furniture_icon_url}/{icon_name}");

            download_once(&nitro_path, &nitro_url, &format!("{base}.nitro"), file_locks, global, nitro_count).await;
            download_once(&icon_path, &icon_url, &icon_name, file_locks, global, icon_count).await;
            anyhow::Ok(())
        }
    });

    // Task.WhenAll: every task runs to the end; the first failure is rethrown afterwards.
    let results = futures::future::join_all(tasks).await;
    if let Some(error) = results.into_iter().find_map(Result::err) {
        return Err(error);
    }
    Ok((nitro_count.load(Ordering::SeqCst), icon_count.load(Ordering::SeqCst)))
}

async fn download_once(
    path: &Path,
    url: &str,
    file_name: &str,
    file_locks: &FileLocks,
    global: &Semaphore,
    counter: &AtomicUsize,
) {
    if path.is_file() {
        return;
    }
    let lock = file_locks.lock().unwrap().entry(path.to_path_buf()).or_default().clone();
    let _file_guard = lock.lock().await;
    if path.is_file() {
        return;
    }
    let _permit = global.acquire().await.expect("semaphore");
    colored(Color::Green, &format!("Downloading: {file_name}"));
    match download_file(url, path, file_name, &resume::Options::default()).await {
        Ok(()) => {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        Err(error) => colored(Color::Red, &format!("Downloading {file_name} => Failed: {error}")),
    }
}

/// `options`: `FRESH` for the furnidata (it changes between runs), resumable for .nitro and icons.
async fn download_file(url: &str, file_path: &Path, file_name: &str, options: &resume::Options) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = resume::download_to_workspace(common::default_client(), url, file_path, options).await;
    match &result {
        Ok(()) => colored(Color::Green, &format!("Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnidata_json_and_names() {
        let json = r#"{"roomitemtypes":{"furnitype":[{"id":1,"classname":"chair*3","extra":true}]},"wallitemtypes":null}"#;
        let furnidata: Furnidata = serde_json::from_str(json).unwrap();
        let room = furnidata.room_item_types.unwrap().furni_type.unwrap();
        assert_eq!(room[0].classname.as_deref(), Some("chair*3"));
        assert!(furnidata.wall_item_types.is_none());
        assert_eq!(nitro_names("chair*3"), ("chair".to_string(), "chair_3_icon.png".to_string()));
        assert_eq!(nitro_names("table"), ("table".to_string(), "table_icon.png".to_string()));
        let null: Option<Furnidata> = serde_json::from_str("null").unwrap();
        assert!(null.is_none());
    }
}
