//! `TextsDownloader` (Flash_Texts.cs): external_flash_texts -> ExternalTexts.txt + ExternalTexts.json.

use std::path::Path;

use anyhow::Result;
use habbo_core::Color;
use serde_json::{Map, Value};

use crate::common::{self, colored, FetchError};
use crate::resume;
use crate::jsonw::{to_string_indented, Escaping};

/// `key=value` lines -> an ordered map (blank and `#` lines skipped, later keys overwrite in place).
/// Shared by the flash texts and the external variables conversions.
pub fn parse_key_values<'a>(lines: impl IntoIterator<Item = &'a str>) -> Map<String, Value> {
    let mut map = Map::new();
    for line in lines {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some(separator) = line.find('=') else { continue };
        if separator == 0 {
            continue;
        }
        let key = line[..separator].trim().to_string();
        let value = line[separator + 1..].trim().to_string();
        map.insert(key, Value::String(value));
    }
    map
}

/// The JSON text of a key/value file (relaxed encoder, indented).
pub fn key_values_json(text: &str) -> String {
    to_string_indented(&Value::Object(parse_key_values(common::split_lines(text))), Escaping::Relaxed)
}

/// `TextsDownloader.DownloadTextsAsync`.
pub async fn download_texts() -> Result<()> {
    let config = common::load_config()?;
    let external_text_url = common::cfg(&config, "AppSettings:externaltexturl")?;

    let result: Result<()> = async {
        let txt_path = "./Habbo_Default/files/txt/ExternalTexts.txt";
        download_file(&external_text_url, txt_path, "ExternalTexts.txt").await?;
        colored(Color::Green, "External Flash Texts Saved");
        convert_texts_to_json(txt_path);
        Ok(())
    }
    .await;
    if let Err(error) = result {
        colored(Color::Red, &format!("Error downloading external flash texts: {error}"));
    }
    Ok(())
}

fn convert_texts_to_json(txt_path: &str) {
    let result = (|| -> Result<()> {
        let text = common::decode_text(&std::fs::read(txt_path)?);
        let json_path = Path::new("./Habbo_Default/files/json/ExternalTexts.json");
        std::fs::create_dir_all(json_path.parent().unwrap())?;
        std::fs::write(json_path, key_values_json(&text))?;
        Ok(())
    })();
    match result {
        Ok(()) => colored(Color::Green, "External Flash Texts JSON Saved"),
        Err(error) => colored(Color::Red, &format!("Error converting texts to JSON: {error}")),
    }
}

/// `DownloadFileAsync` (prints, then rethrows an HTTP failure).
pub(crate) async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = resume::download_to(common::default_client(), url, Path::new(file_path), &resume::Options { create_dir: true, ..resume::Options::FRESH }).await;
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
    fn key_values_keep_first_position() {
        let map = parse_key_values(["# c", "  ", "a = 1 ", "=x", "noeq", "b=x=y", "a=2", "  #x=1"]);
        assert_eq!(Value::Object(map).to_string(), r#"{"a":"2","b":"x=y"}"#);
        let text = "k=<b>é\"</b>\r\n";
        let json = crate::jsonw::to_string_with_newline(
            &Value::Object(parse_key_values(common::split_lines(text))),
            Escaping::Relaxed,
            "\n",
        );
        assert_eq!(json, "{\n  \"k\": \"<b>é\\\"</b>\"\n}");
    }
}
