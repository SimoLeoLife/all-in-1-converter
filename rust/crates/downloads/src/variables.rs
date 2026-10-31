//! `VariablesDownloader` (Variables.cs): external_variables -> external_variables.txt + .json,
//! with 3 attempts on HTTP failures.

use std::path::Path;

use anyhow::Result;
use habbo_core::Color;

use crate::common::{self, colored, FetchError};
use crate::flash_texts::key_values_json;

/// `VariablesDownloader.DownloadVariablesAsync`.
pub async fn download_variables() -> Result<()> {
    let config = common::load_config()?;

    let result: Result<()> = async {
        let url = common::cfg(&config, "AppSettings:externalvarsurl")?;
        if url.is_empty() {
            colored(Color::Red, "Error: External Variables URL is not configured.");
            return Ok(());
        }
        match reqwest::Url::parse(&url) {
            Ok(uri) if uri.scheme() == "http" || uri.scheme() == "https" => {}
            _ => {
                colored(Color::Red, "Error: Invalid External Variables URL.");
                return Ok(());
            }
        }

        let mut retry_count = 3;
        while retry_count > 0 {
            let txt_path = Path::new("./Habbo_Default/files/txt").join("external_variables.txt");
            match download_file(&url, &txt_path, "external_variables.txt").await {
                Ok(()) => {
                    colored(Color::Green, "External Variables Saved");
                    convert_variables_to_json(&txt_path);
                    return Ok(());
                }
                Err(FetchError::Http(message)) => {
                    colored(
                        Color::Yellow,
                        &format!("Error downloading external variables: {message}. Retries left: {}", retry_count - 1),
                    );
                    if retry_count == 1 {
                        colored(Color::Red, "Failed to download external variables after 3 attempts.");
                    }
                }
                Err(FetchError::Other(error)) => return Err(error),
            }
            retry_count -= 1;
        }
        Ok(())
    }
    .await;

    if let Err(error) = result {
        colored(Color::Red, &format!("Error downloading external variables: {error}"));
    }
    Ok(())
}

fn convert_variables_to_json(txt_path: &Path) {
    let result = (|| -> Result<()> {
        let text = common::decode_text(&std::fs::read(txt_path)?);
        let json_path = Path::new("./Habbo_Default/files/json/external_variables.json");
        std::fs::create_dir_all(json_path.parent().unwrap())?;
        std::fs::write(json_path, key_values_json(&text))?;
        Ok(())
    })();
    match result {
        Ok(()) => colored(Color::Green, "External Variables JSON Saved"),
        Err(error) => colored(Color::Red, &format!("Error converting variables to JSON: {error}")),
    }
}

async fn download_file(url: &str, file_path: &Path, file_name: &str) -> Result<(), FetchError> {
    let bytes = common::get_bytes(common::default_client(), url).await?;
    if let Some(directory) = file_path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(file_path, bytes)?;
    colored(Color::Green, &format!("Downloaded: {file_name}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::flash_texts::parse_key_values;

    #[test]
    fn external_variables_parse() {
        let text = "flash.client.url=https://images.habbo.com/gordon/flash-assets-PRODUCTION-1/\r\n#comment\r\nimage.library.url=${url.prefix}/c_images/\r\nempty=\r\n";
        let map = parse_key_values(crate::common::split_lines(text));
        assert_eq!(map["flash.client.url"], "https://images.habbo.com/gordon/flash-assets-PRODUCTION-1/");
        assert_eq!(map["image.library.url"], "${url.prefix}/c_images/");
        assert_eq!(map["empty"], "");
        assert_eq!(map.len(), 3);
    }
}
