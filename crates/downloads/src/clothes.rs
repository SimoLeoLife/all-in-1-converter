//! `ClothesDownloader` (Clothes.cs): figuredata/figuremap -> FigureData.json / FigureMap.json,
//! then every clothes library into Habbo_Default/clothes/{swf,hab}.

use std::path::Path;

use anyhow::anyhow;
use habbo_core::{console, outln, settings, Color};
use habbo_core::paths::HabboAssetFolder;

use crate::asset_downloader;
use crate::common::{self, colored, log, FetchError, INDEX_OUT_OF_RANGE, NULL_REFERENCE};
use crate::resume;
use crate::model;
use crate::xml::Document;

const GORDON_DIRECTORY: &str = "https://images.habbo.com/gordon/";
const FIGUREDATA_URL: &str = "http://habbo.com/gamedata/figuredata/1";

/// `ClothesDownloader.DownloadClothesAsync`.
pub async fn download_clothes() -> anyhow::Result<()> {
    if let Err(error) = run().await {
        colored(Color::Red, &format!("Error: {error}"));
    }
    Ok(())
}

async fn run() -> anyhow::Result<()> {
    let source = common::get_string(common::default_client(), "https://www.habbo.com/gamedata/external_variables/1").await?;

    let release = match common::release_from_line_drop_last(&source) {
        Some(Ok(release)) => {
            outln!("We are going to download from release: {release}");
            release
        }
        Some(Err(_)) => return Err(anyhow!(INDEX_OUT_OF_RANGE)),
        None => String::new(),
    };
    if release.is_empty() {
        colored(Color::Red, "Error: Could not determine the release version.");
        return Ok(());
    }

    outln!("👉 We can disable all the sellable setting in the JSON,");
    outln!("if this is disabled all clothes will be visible if not then only clothes that are not sellable 👈");
    outln!("❓ Do you want to disable all sellable? (Y/N): ");
    let answer = console::read_line_opt().ok_or_else(|| anyhow!(NULL_REFERENCE))?;
    let disable_sellable = answer.trim().to_uppercase() == "Y";

    let current_directory = std::env::current_dir()?;
    let xml_directory = current_directory.join("Habbo_Default").join("files").join("xml");
    let json_directory = current_directory.join("Habbo_Default").join("files").join("json");

    let figuremap_url = format!("{GORDON_DIRECTORY}{release}/figuremap.xml");

    download_file(FIGUREDATA_URL, &xml_directory.join("figuredata.xml"), "figuredata.xml").await?;
    download_file(&figuremap_url, &xml_directory.join("figuremap.xml"), "figuremap.xml").await?;

    // Convert XML to JSON.
    let figuredata_file = xml_directory.join("figuredata.xml");
    convert_figure_data(&figuredata_file, &json_directory.join("FigureData.json"), disable_sellable);

    let figuremap_file = xml_directory.join("figuremap.xml");
    convert_figure_map(&figuremap_file, &json_directory.join("FigureMap.json"));

    if !figuremap_file.is_file() {
        colored(Color::Red, "Error: figuremap.xml does not exist.");
        return Ok(());
    }
    let lib_ids = model::figure_map_lib_ids(&Document::load(&figuremap_file)?);

    outln!(
        "Found {} clothes libraries; download format: {} (config.ini download_format)",
        lib_ids.len(),
        settings::download_format()
    );

    let folder = HabboAssetFolder::CLOTHES;
    asset_downloader::prepare_folder(folder).await?;

    let libraries = asset_downloader::distinct_ignore_case(lib_ids)
        .into_iter()
        .map(|id| {
            let url = format!("{GORDON_DIRECTORY}{release}/{id}");
            (id, url)
        })
        .collect();
    let counts = asset_downloader::download_libraries(folder, libraries).await?;

    colored(Color::Green, &asset_downloader::summary("clothes files", &counts));

    outln!();
    outln!("All has been done!");
    outln!();
    Ok(())
}

/// `DownloadFileAsync`: an HTTP failure is printed and reported as `false`; other errors escape.
async fn download_file(url: &str, file_path: &Path, file_name: &str) -> anyhow::Result<bool> {
    let result: Result<(), FetchError> = resume::download_to(common::default_client(), url, file_path, &resume::Options::FRESH).await;
    match result {
        Ok(()) => {
            log(Color::Green, &format!("📁 Downloaded: {file_name}"));
            Ok(true)
        }
        Err(FetchError::Http(message)) => {
            log(Color::Red, &format!("Error downloading {file_name}: {message}"));
            Ok(false)
        }
        Err(FetchError::Other(error)) => Err(error),
    }
}

fn convert_figure_data(xml_file_path: &Path, json_output_path: &Path, disable_sellable: bool) {
    if !xml_file_path.is_file() {
        colored(Color::Red, "Error: figuredata.xml does not exist.");
        return;
    }
    let result = (|| -> anyhow::Result<()> {
        let doc = Document::load(xml_file_path)?;
        let value = model::figure_data(&doc, disable_sellable)?;
        std::fs::write(json_output_path, crate::jsonw::to_string_indented(&value, crate::jsonw::Escaping::Default))?;
        Ok(())
    })();
    match result {
        Ok(()) => log(Color::Green, &format!("🔄 Converted: {}", json_output_path.display())),
        Err(error) => log(Color::Red, &format!("Error during conversion: {error}")),
    }
}

fn convert_figure_map(xml_file_path: &Path, json_output_path: &Path) {
    if !xml_file_path.is_file() {
        colored(Color::Red, "Error: figuremap.xml does not exist.");
        return;
    }
    let result = (|| -> anyhow::Result<()> {
        let doc = Document::load(xml_file_path)?;
        let value = model::figure_map(&doc);
        std::fs::write(json_output_path, crate::jsonw::to_string_indented(&value, crate::jsonw::Escaping::Default))?;
        Ok(())
    })();
    match result {
        Ok(()) => log(Color::Green, &format!("🔄 Converted: {}", json_output_path.display())),
        Err(error) => log(Color::Red, &format!("Error during figuremap conversion: {error}")),
    }
}
