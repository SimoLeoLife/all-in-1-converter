//! `FurnidataDownloader` (Furnidata.cs): furnidata.txt + furnidata_xml.xml, and FurnitureData.json
//! converted from the xml (into the Asset Workspace gamedata folder when one is configured).

use std::path::PathBuf;

use anyhow::Result;
use habbo_core::workspace::runtime;
use habbo_core::{outln, Color};

use crate::common::{self, colored, FetchError};
use crate::model;

const XML_PATH: &str = "./Habbo_Default/files/txt/furnidata_xml.xml";

/// `FurnidataDownloader.DownloadFurnidata`.
pub async fn download_furnidata() -> Result<()> {
    let config = common::load_config()?;
    let furnidata_txt = common::cfg(&config, "AppSettings:furnidataTXT")?;
    let furnidata_xml = common::cfg(&config, "AppSettings:furnidataXML")?;

    outln!("🚀 Saving Starting the furnidata download");

    let result: Result<(), FetchError> = async {
        colored(Color::Cyan, &format!("📡 Downloading furnidata.txt from: {furnidata_txt}"));

        download_file(&furnidata_txt, "./Habbo_Default/files/txt/furnidata.txt", "furnidata.txt").await?;
        download_file(&furnidata_xml, XML_PATH, "furnidata_xml.xml").await?;

        let workspace = runtime::router();
        let legacy_output = "./Habbo_Default/files/json/FurnitureData.json";
        let target_output: PathBuf = workspace.game_data_file("FurnitureData.json", legacy_output);

        let xml = common::decode_text(&std::fs::read(XML_PATH)?);
        let json = model::furniture_data_json(&xml)?;
        if workspace.is_configured() {
            habbo_core::workspace::write_output(&target_output, json.as_bytes())?;
        } else {
            std::fs::write(&target_output, json)?;
        }

        colored(Color::Green, &format!("📦 Converted furnidata_xml to {}", target_output.display()));
        outln!("🎉 Furnidata completed successfully!");
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {}
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("❌ Error downloading furnidata: {message}")),
        Err(FetchError::Other(error)) => colored(Color::Red, &format!("❌ Unexpected error downloading furnidata: {error}")),
    }
    Ok(())
}

async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = async {
        let bytes = common::get_bytes(common::default_client(), url).await?;
        std::fs::write(file_path, bytes)?;
        Ok(())
    }
    .await;
    match &result {
        Ok(()) => colored(Color::Green, &format!("✅ Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("❌ Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}
