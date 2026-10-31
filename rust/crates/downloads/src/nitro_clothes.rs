//! `NitroClothesDownloader` (NitroClothes.cs): a custom Nitro hotel's FigureMap.json /
//! FigureData.json and every clothes .nitro into custom_downloads/clothes (or the Asset
//! Workspace clothing folder).

use std::path::Path;

use anyhow::{anyhow, Result};
use habbo_core::workspace::{self, runtime, WorkspaceAssetKind};
use habbo_core::{console, outln, Color};
use serde_json::Value;

use crate::common::{self, colored, FetchError, NULL_REFERENCE};

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

            let (mut processed, mut downloaded, mut skipped) = (0, 0, 0);
            for library in libraries {
                let id = token_text(library.get("id"))?;
                processed += 1;
                if id == "hh_human_fx" || id == "hh_pets" {
                    continue;
                }
                let file_path = download_directory.join(format!("{id}.nitro"));
                if file_path.is_file() {
                    skipped += 1;
                    continue;
                }
                let url = format!("{nitro_clothes_dir}/{id}.nitro");
                match download_file(&url, &file_path, &format!("{id}.nitro")).await {
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

async fn download_file(url: &str, file_path: &Path, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = async {
        let content = common::get_bytes(common::default_client(), url).await?;
        workspace::write_output(file_path, &content)?;
        Ok(())
    }
    .await;
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
