//! `NitroClothesDownloader` (NitroClothes.cs): a custom Nitro hotel's FigureMap.json /
//! FigureData.json and every clothes .nitro into custom_downloads/clothes (or the Asset
//! Workspace clothing folder).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use futures::StreamExt;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_core::{console, outln, Color};
use serde_json::Value;

use crate::common::{self, colored, FetchError, NULL_REFERENCE};
use crate::resume::{self, PartFile};

const NITRO_CLOTHES_DIR_KEY: &str = "AppSettings:nitro_clothes_dir";
const NITRO_FIGURE_DATA_KEY: &str = "AppSettings:nitro_figuredata";
const NITRO_FIGURE_MAP_KEY: &str = "AppSettings:nitro_figuremap";

/// `JToken.ToString()` of a library id (`None` = the key is missing).
fn token_text(token: Option<&Value>) -> Result<String> {
    match token {
        None => Err(anyhow!(NULL_REFERENCE)),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Null) => Ok(String::new()),
        Some(Value::Bool(b)) => Ok(if *b { "True" } else { "False" }.to_string()),
        Some(other) => Ok(other.to_string()),
    }
}

/// `figureMap["libraries"]` as a list to iterate.
fn library_list(figure_map: &Value) -> Result<&Vec<Value>> {
    let libraries = figure_map.get("libraries").filter(|l| !l.is_null()).ok_or_else(|| anyhow!(NULL_REFERENCE))?;
    libraries.as_array().ok_or_else(|| anyhow!("Cannot access child value on {}.", kind(libraries)))
}

/// The library ids of FigureMap.json, in order.
pub fn library_ids(figure_map: &Value) -> Result<Vec<String>> {
    library_list(figure_map)?.iter().map(|library| token_text(library.get("id"))).collect()
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "Newtonsoft.Json.Linq.JObject",
        _ => "Newtonsoft.Json.Linq.JValue",
    }
}

/// `NitroClothesDownloader.DownloadCustomClothesAsync`.
pub async fn download_custom_clothes() -> Result<()> {
    let config = common::load_config()?;
    let (Some(nitro_clothes_dir), Some(nitro_figure_data), Some(nitro_figure_map)) = (
        config.get(NITRO_CLOTHES_DIR_KEY),
        config.get(NITRO_FIGURE_DATA_KEY),
        config.get(NITRO_FIGURE_MAP_KEY),
    ) else {
        colored(Color::Red, "Error! Missing required keys in config.ini.");
        return Ok(());
    };

    let result: Result<()> = async {
        let router = runtime::router();
        let current_directory = std::env::current_dir()?;
        let download_directory =
            router.asset_directory(WorkspaceAssetKind::Clothing, current_directory.join("./custom_downloads/clothes"));
        std::fs::create_dir_all(&download_directory)?;

        let figure_map_file_path = router.game_data_file("FigureMap.json", download_directory.join("FigureMap.json"));
        let figure_data_file_path = router.game_data_file("FigureData.json", download_directory.join("FigureData.json"));

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        download_file(&format!("{nitro_figure_map}?timestamp={timestamp}"), &figure_map_file_path, "FigureMap.json").await?;
        download_file(&format!("{nitro_figure_data}?timestamp={timestamp}"), &figure_data_file_path, "FigureData.json").await?;

        let processed_result: Result<()> = async {
            let json_content = common::decode_text(&std::fs::read(&figure_map_file_path)?);
            let figure_map: Value = serde_json::from_str(&json_content)?;
            if !figure_map.is_object() {
                return Err(anyhow!("Error reading JObject from JsonReader. Current JsonReader item is not an object."));
            }
            let libraries = library_list(&figure_map)?;

            // Not in the C# version: config.ini download_parallelism files are fetched at once, but
            // every library is handled in order (same messages, same counts) like the one-by-one loop.
            let mut seen = HashSet::new();
            let steps: Vec<Result<(String, PathBuf, bool)>> = libraries
                .iter()
                .map(|library| {
                    let id = token_text(library.get("id"))?;
                    let file_path = download_directory.join(format!("{id}.nitro"));
                    let first = seen.insert(common::path_key(&file_path));
                    Ok((id, file_path, first))
                })
                .collect();
            let client = common::default_client();
            let nitro_clothes_dir = &nitro_clothes_dir;
            let results = futures::stream::iter(steps)
                .map(|step| async move {
                    let fetched = match &step {
                        Ok((id, file_path, true)) if !is_skipped_library(id) && !file_path.is_file() => {
                            Some(fetch(client, nitro_clothes_dir, id, file_path).await)
                        }
                        _ => None,
                    };
                    (step, fetched)
                })
                .buffered(common::download_parallelism());
            let mut results = std::pin::pin!(results);

            let (mut processed, mut downloaded, mut skipped) = (0, 0, 0);
            while let Some((step, fetched)) = results.next().await {
                let (id, file_path, first) = step?;
                processed += 1;
                if is_skipped_library(&id) {
                    continue;
                }
                let fetched = match fetched {
                    Some(fetched) => fetched,
                    // Already there (or saved by an earlier library with the same id).
                    None if first || file_path.is_file() => {
                        skipped += 1;
                        continue;
                    }
                    // An earlier library with the same id failed: try again, like the C# loop.
                    None => fetch(client, nitro_clothes_dir, &id, &file_path).await,
                };
                match finish(fetched, &format!("{id}.nitro")) {
                    Ok(()) => {
                        colored(Color::Green, &format!("Downloaded: {id}.nitro"));
                        downloaded += 1;
                    }
                    Err(error) => colored(Color::Red, &format!("Error downloading {id}.nitro: {error}")),
                }
            }

            console::set_color(Color::Cyan);
            outln!("
--- Summary ---");
            outln!("Total files processed: {processed}");
            outln!("Total files downloaded: {downloaded}");
            outln!("Total files skipped: {skipped}");
            outln!("---");
            console::set_color(Color::Gray);

            if downloaded > 0 {
                colored(Color::Green, &format!("Downloaded {downloaded} new clothes!"));
            } else {
                colored(Color::Green, "You have the latest clothes!");
            }
            Ok(())
        }
        .await;
        if let Err(error) = processed_result {
            colored(Color::Red, &format!("Error reading or parsing {}: {error}", figure_map_file_path.display()));
        }

        outln!();
        outln!("All has been done!");
        outln!();
        Ok(())
    }
    .await;

    if let Err(error) = result {
        colored(Color::Red, &format!("Error downloading custom clothes: {error}"));
    }
    Ok(())
}

fn is_skipped_library(id: &str) -> bool {
    id == "hh_human_fx" || id == "hh_pets"
}

async fn fetch(client: &reqwest::Client, nitro_clothes_dir: &str, id: &str, file_path: &Path) -> Result<PartFile, FetchError> {
    let url = format!("{nitro_clothes_dir}/{id}.nitro");
    resume::fetch_ok(|| client.get(&url), file_path, &resume::Options::default()).await
}

/// The end of `DownloadFileAsync`: the .part goes through the workspace output, and HTTP failures are printed.
fn finish(fetched: Result<PartFile, FetchError>, file_name: &str) -> Result<(), FetchError> {
    let result = fetched.and_then(|part| part.commit_to_workspace().map_err(FetchError::from));
    match &result {
        Ok(()) => colored(Color::Green, &format!("Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}

async fn download_file(url: &str, file_path: &Path, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = resume::download_to_workspace(common::default_client(), url, file_path, &resume::Options::FRESH).await;
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
    use serde_json::json;

    #[test]
    fn figure_map_ids() {
        let map = json!({"libraries": [{"id": "hh_human_body", "revision": 1}, {"id": 7}, {"id": null}, {"id": true}]});
        assert_eq!(library_ids(&map).unwrap(), ["hh_human_body", "7", "", "True"]);
        assert_eq!(library_ids(&json!({})).unwrap_err().to_string(), NULL_REFERENCE);
        assert!(library_ids(&json!({"libraries": [{"revision": 1}]})).is_err());
    }
}
