//! `SWF_Furni_To_Nitro`: furniture SWF → .nitro / .hab.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use habbo_core::outln;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_nitro::writer;

use crate::assets::{self, assets_json};
use crate::common::{self, LoopOptions};
use crate::index;
use crate::logic::{self, logic_object_json};
use crate::net;
use crate::spritesheet;
use crate::stj::{FloatStyle, Obj, J};
use crate::visualizations::{self, visualizations_json};

/// `OutputDirectory`: the workspace furniture folder, else SWFCompiler/furniture.
pub fn output_directory() -> PathBuf {
    runtime::router().asset_directory(WorkspaceAssetKind::Furniture, Path::new("SWFCompiler").join("furniture"))
}

/// `SWF_Furni_To_Nitro.ConvertSwfFilesAsync`.
pub async fn convert_furniture() -> Result<()> {
    if let Err(error) = convert_inner().await {
        outln!("❌ Error during SWF conversion: {error}");
    }
    Ok(())
}

async fn convert_inner() -> Result<()> {
    outln!("Do you want (H) Hof_Furni or (I) Imported furniture? (Default is H):");
    let input = habbo_core::console::read_line().trim().to_uppercase();
    let import_directory =
        if input == "I" { Path::new("SWFCompiler").join("import").join("furniture") } else { habbo_core::paths::hof_furni::swf_source() };

    outln!("✅ Converting SWF to Nitro from source {}", import_directory.display());

    let output_directory = output_directory();
    std::fs::create_dir_all(&output_directory)?;
    let swf_files = net::get_files(&import_directory, "*.swf")?;
    if swf_files.is_empty() {
        outln!("No SWF files found in the import directory.");
        return Ok(());
    }
    outln!("✅ Found {} SWF files to convert.", swf_files.len());

    let out = output_directory.clone();
    common::run_loop(
        swf_files,
        output_directory,
        LoopOptions {
            process_title: "SWF Furniture -> {label}".into(),
            max_parallelism: common::max_parallelism(0.9),
            progress_every: 50,
            progress_first: 10,
            counter_width: 5,
        },
        Arc::new(move |swf: &Path| Ok(process_swf_file_blocking(swf, &out))),
    )
    .await
}

/// `SWF_Furni_To_Nitro.ProcessSwfFileAsync`: converts one SWF into the furniture output
/// folder; `false` when it was skipped or failed (the reason is printed).
pub async fn process_swf_file(swf_file: &Path) -> bool {
    let swf = swf_file.to_path_buf();
    let out = output_directory();
    tokio::task::spawn_blocking(move || process_swf_file_blocking(&swf, &out)).await.unwrap_or(false)
}

/// [`process_swf_file`] on the calling (blocking) thread, into `output_directory`.
pub fn process_swf_file_blocking(swf_file: &Path, output_directory: &Path) -> bool {
    let file_name = habbo_core::paths::file_stem(swf_file);
    if writer::exists(output_directory, &file_name) {
        return false;
    }
    let file_output_directory = output_directory.join(&file_name);
    let _ = std::fs::create_dir_all(&file_output_directory);

    let result = convert_one(swf_file, output_directory, &file_output_directory, &file_name);
    let converted = match result {
        Ok(converted) => converted,
        Err(error) => {
            outln!("❌ Error generating sprite sheet for {file_name}: {error}");
            false
        }
    };
    common::delete_directory(&file_output_directory);
    converted
}

fn convert_one(swf_file: &Path, output_directory: &Path, file_output_directory: &Path, file_name: &str) -> Result<bool> {
    let binary_output_path = file_output_directory.join(format!("{file_name}_binaryData"));
    common::block_on(crate::ffdec_extractor::extract_swf(swf_file, &binary_output_path))?;

    let binary_data = binary_output_path.join("binaryData");
    if !binary_data.is_dir() {
        return Ok(false);
    }

    let csv_path = binary_output_path.join("symbolClass").join("symbols.csv");
    let canonical_mapping = assets::build_canonical_mapping(&csv_path);
    let image_sources = assets::parse_debug_xml(&csv_path);

    // Index, assets, logic and visualizations (Task.WhenAll: the first failure in this order wins).
    let index_data = common::first_file(&binary_data, "*_index.bin")?.and_then(|f| index::parse_index_file(&f));

    let assets_file = common::first_file(&binary_data, "*_assets.bin")?;
    let manifest_file = common::first_file(&binary_data, "*_manifest.bin")?;
    let parsed_assets = match (assets_file, manifest_file) {
        (Some(assets_file), Some(manifest_file)) => Some(assets::parse_assets_file_scoped(
            &assets_file,
            Some(&image_sources),
            &manifest_file,
            &csv_path,
            file_output_directory,
        )),
        _ => None,
    };

    let logic_result = get_logic_data(&binary_output_path);
    let visualization_result = get_visualizations_data(&binary_output_path);
    let logic_data = logic_result?;
    let visualizations = visualization_result?;

    let Some(index_data) = index_data else { return Ok(false) };

    let images_directory = binary_output_path.join("images");
    let tmp_directory = binary_output_path.join("tmp");
    let empty = Default::default();
    let image_mapping = parsed_assets.as_ref().map(|p| &p.image_mapping).unwrap_or(&empty);
    spritesheet::restore_images_from_tmp(&tmp_directory, &images_directory, image_mapping);

    let images = spritesheet::load_images(&images_directory)?;
    if images.is_empty() {
        return Ok(false);
    }

    let Some((sprite_sheet_path, sprite_sheet_data)) =
        spritesheet::generate_sprite_sheet(&images, file_output_directory, file_name, &canonical_mapping, false, 10, 14000, 14000)?
    else {
        return Ok(false);
    };
    drop(images);

    let json_output_path = file_output_directory.join(format!("{file_name}.json"));
    let full_object = Obj::new()
        .opt("name", index_data.name.clone().map(J::Str))
        .opt("logicType", index_data.logic_type.clone().map(J::Str))
        .opt("visualizationType", index_data.visualization_type.clone().map(J::Str))
        .opt("assets", parsed_assets.as_ref().map(|p| assets_json(&p.assets)))
        .put("logic", logic_object_json(logic_data.as_ref(), FloatStyle::Fixed))
        .opt("visualizations", visualizations.as_ref().map(|v| visualizations_json(v)))
        .put("spritesheet", sprite_sheet_data.to_json(FloatStyle::Fixed))
        .build();
    let json_content = full_object.to_string_indented()?;
    std::fs::write(&json_output_path, json_content)?;

    common::bundle_nitro_file(file_output_directory, file_name, output_directory, &sprite_sheet_path)?;
    Ok(true)
}

/// `GetLogicDataAsync` (shared with the pets converter).
pub(crate) fn get_logic_data(binary_output_path: &Path) -> Result<Option<logic::AssetLogicData>> {
    match common::first_file(&binary_output_path.join("binaryData"), "*_logic.bin")? {
        Some(file) => {
            let text = net::read_all_text(&file)?;
            Ok(Some(logic::parse_logic_text(&text)?))
        }
        None => {
            common::warn_missing("*_logic.bin", binary_output_path, "logic");
            Ok(None)
        }
    }
}

fn get_visualizations_data(binary_output_path: &Path) -> Result<Option<Vec<visualizations::Visualization>>> {
    match common::first_file(&binary_output_path.join("binaryData"), "*_visualization.bin")? {
        Some(file) => {
            let text = net::read_all_text(&file)?;
            Ok(Some(visualizations::parse_visualizations_text(&text)?))
        }
        None => {
            common::warn_missing("*_visualization.bin", binary_output_path, "visualization");
            Ok(None)
        }
    }
}
