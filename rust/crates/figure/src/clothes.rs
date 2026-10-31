//! `SWF_clothes_To_Nitro`: clothing SWFs -> .nitro / .hab.

use std::path::{Path, PathBuf};

use anyhow::Result;
use habbo_core::paths::HabboAssetFolder;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_core::{console, outln};
use habbo_nitro::writer;

use crate::assets::{assets_node, parse_clothes_assets_file, Assets};
use crate::driver::{self, LoopOptions};
use crate::extract::{extract_swf, restore_images_from_tmp};
use crate::spritesheet::generate_sprite_sheet;
use crate::stj::{to_string_indented, Node, ObjectBuilder};
use crate::symbols::{get_image_mapping, parse_debug_xml};

fn output_directory() -> PathBuf {
    runtime::router().asset_directory(WorkspaceAssetKind::Clothing, Path::new("SWFCompiler").join("clothes"))
}

/// `SWF_clothes_To_Nitro.ConvertSwfFilesAsync`.
pub async fn convert_clothes() -> Result<()> {
    if let Err(error) = convert_swf_files().await {
        outln!("❌ Error during SWF conversion: {error}");
    }
    Ok(())
}

async fn convert_swf_files() -> Result<()> {
    outln!("Do you want (H) Hof_Furni or (I) Imported clothes? (Default is H):");
    let input = console::read_line().trim().to_uppercase();

    let import_directory = if input == "I" {
        Path::new("SWFCompiler").join("import").join("clothes")
    } else {
        HabboAssetFolder::CLOTHES.swf_source()
    };

    outln!("✅ Converting SWF to Nitro from source {}", import_directory.display());

    let output_directory = output_directory();
    std::fs::create_dir_all(&output_directory)?;
    let swf_files = crate::util::get_files(&import_directory, "*.swf")?;

    if swf_files.is_empty() {
        outln!("ℹ️ No SWF files found in the import (SWFCompiler\\import\\clothes) directory.");
        return Ok(());
    }

    outln!("✅ Found {} clothing SWF files to convert.", swf_files.len());

    let out = output_directory.clone();
    driver::run(
        swf_files,
        output_directory,
        LoopOptions { title: "SWF Clothes", skip_human_fx: true, progress_every: 50, counter_width: 5 },
        move |swf_file| {
            let out = out.clone();
            async move { process_swf_file(&swf_file, &out).await }
        },
    )
    .await
}

/// `ProcessSwfFileAsync`: `Ok(true)` when the bundle was written.
pub async fn process_swf_file(swf_file: &Path, output_directory: &Path) -> Result<bool> {
    if driver::is_human_fx(swf_file) {
        return Ok(false);
    }
    let file_name = habbo_core::paths::file_stem(swf_file);
    if writer::exists(output_directory, &file_name) {
        return Ok(false);
    }

    let file_output_directory = output_directory.join(&file_name);
    std::fs::create_dir_all(&file_output_directory)?;
    let binary_output_path = file_output_directory.join(format!("{file_name}_binaryData"));

    let result = convert(swf_file, &file_name, &file_output_directory, &binary_output_path, output_directory).await;
    let converted = match result {
        Ok(converted) => converted,
        Err(error) => {
            outln!("❌ Error generating sprite sheet for {file_name}: {error}");
            false
        }
    };
    driver::delete_directory(&file_output_directory);
    Ok(converted)
}

async fn convert(
    swf_file: &Path,
    file_name: &str,
    file_output_directory: &Path,
    binary_output_path: &Path,
    output_directory: &Path,
) -> Result<bool> {
    extract_swf(swf_file, binary_output_path).await?;

    if !binary_output_path.join("binaryData").is_dir() {
        return Ok(false);
    }

    let csv_path = binary_output_path.join("symbolClass").join("symbols.csv");
    let _image_sources = parse_debug_xml(&csv_path);

    let clothes_mapping = get_image_mapping(&csv_path);
    if clothes_mapping.is_empty() {
        return Ok(false);
    }

    let (library_name, assets) = get_asset_data(binary_output_path, &csv_path)?;

    let images_directory = binary_output_path.join("images");
    let tmp_directory = binary_output_path.join("tmp");
    restore_images_from_tmp(&tmp_directory, &images_directory, &clothes_mapping)?;

    let images = driver::load_images(&images_directory)?;
    if images.is_empty() {
        return Ok(false);
    }

    // NOTE(port): the C# frame name is `ClothesAssetsMapper.LatestImageMapping[key] ?? key`, and
    // that mapping always ends up as asset name -> itself (or empty), so it is the key.
    let Some((sprite_sheet_path, sprite_sheet_data)) =
        generate_sprite_sheet(&images, file_output_directory, file_name, 14000, 14000)?
    else {
        return Ok(false);
    };

    let json = clothes_json(&assets, &library_name, Some(sprite_sheet_data.to_node()));
    std::fs::write(file_output_directory.join(format!("{file_name}.json")), json)?;
    driver::bundle_nitro_file(file_output_directory, file_name, output_directory, Some(&sprite_sheet_path))?;
    Ok(true)
}

fn get_asset_data(binary_output_path: &Path, csv_path: &Path) -> Result<(String, Assets)> {
    let Some(manifest) = driver::find_manifest(binary_output_path)? else {
        return Ok((String::new(), Assets::new()));
    };
    Ok(parse_clothes_assets_file(&manifest, csv_path))
}

/// `new { assets, name, spritesheet }` serialised with System.Text.Json.
pub fn clothes_json(assets: &Assets, library_name: &str, spritesheet: Option<Node>) -> String {
    let node = ObjectBuilder::new()
        .put("assets", assets_node(assets))
        .put("name", Node::str(library_name))
        .opt("spritesheet", spritesheet)
        .build();
    to_string_indented(&node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Asset;

    #[test]
    fn json_root_order() {
        let mut assets = Assets::new();
        assets.insert("a".into(), Asset { x: 1, y: -2, source: None });
        let json = clothes_json(&assets, "hh_lib", None).replace("\r\n", "\n");
        assert_eq!(json, "{\n  \"assets\": {\n    \"a\": {\n      \"x\": 1,\n      \"y\": -2\n    }\n  },\n  \"name\": \"hh_lib\"\n}");
    }
}
