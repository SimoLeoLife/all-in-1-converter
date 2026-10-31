//! `SWF_Pets_Compiler`: pet SWF → .nitro / .hab. Reuses the furniture extractor, logic and
//! sprite-sheet mappers; has its own assets, index, palette and visualization mappers.

pub mod assets;
pub mod palette;
pub mod visualizations;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use habbo_core::outln;
use habbo_core::paths::HabboAssetFolder;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_nitro::writer;
use indexmap::IndexMap;

use crate::common::{self, LoopOptions};
use crate::index;
use crate::logic::logic_object_json;
use crate::net::{self, NULL_REFERENCE};
use crate::spritesheet;
use crate::stj::{FloatStyle, Obj, J};

use self::assets::Asset;
use self::visualizations::Visualization;

/// `OutputDirectory`: the workspace pets folder, else SWFCompiler/pets.
pub fn output_directory() -> PathBuf {
    runtime::router().asset_directory(WorkspaceAssetKind::Pets, Path::new("SWFCompiler").join("pets"))
}

/// `SWF_Pets_To_Nitro.ConvertSwfFilesAsync`.
pub async fn convert_pets() -> Result<()> {
    if let Err(error) = convert_inner().await {
        outln!("❌ Error during SWF conversion: {error}");
    }
    Ok(())
}

async fn convert_inner() -> Result<()> {
    outln!("Do you want (H) Habbo_Default pets or (I) Imported pets? (Default is H):");
    let import_directory = if habbo_core::console::read_line().trim().to_uppercase() == "I" {
        Path::new("SWFCompiler").join("import").join("pets")
    } else {
        HabboAssetFolder::PETS.swf_source()
    };
    std::fs::create_dir_all(&import_directory)?;
    outln!("✅ Converting SWF to Nitro from source {}", import_directory.display());

    let output_directory = output_directory();
    std::fs::create_dir_all(&output_directory)?;
    let swf_files = net::get_files(&import_directory, "*.swf")?;
    if swf_files.is_empty() {
        outln!("ℹ️ No SWF files found in {}.", import_directory.display());
        return Ok(());
    }
    outln!("✅ Found {} pet SWF files to convert.", swf_files.len());

    let out = output_directory.clone();
    common::run_loop(
        swf_files,
        output_directory,
        LoopOptions {
            process_title: "SWF Pets -> {label}".into(),
            max_parallelism: common::max_parallelism(0.8),
            progress_every: 10,
            progress_first: 5,
            counter_width: 4,
        },
        Arc::new(move |swf: &Path| Ok(process_swf_file_blocking(swf, &out))),
    )
    .await
}

/// `SWF_Pets_To_Nitro.ProcessSwfFileAsync`.
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

    let converted = match convert_one(swf_file, output_directory, &file_output_directory, &file_name) {
        Ok(converted) => converted,
        Err(error) => {
            outln!("❌ Error generating sprite sheet for {file_name}: {error}");
            false
        }
    };
    common::delete_directory(&file_output_directory);
    converted
}

/// `ShouldKeepSize32`: size-32 stays only when it is as complete as size-64.
pub fn should_keep_size32(visualizations: Option<&[Visualization]>, assets: Option<&IndexMap<String, Asset>>) -> bool {
    let (Some(visualizations), Some(assets)) = (visualizations, assets) else { return false };
    let size32 = visualizations.iter().find(|v| v.size == 32);
    let size64 = visualizations.iter().find(|v| v.size == 64);
    let (Some(size32), Some(size64)) = (size32, size64) else { return false };

    let anims32 = size32.animations.as_ref().map_or(0, |a| a.len());
    let anims64 = size64.animations.as_ref().map_or(0, |a| a.len());
    if anims32 < anims64 {
        return false;
    }
    let assets32 = assets.keys().filter(|k| k.contains("_32_")).count();
    let assets64 = assets.keys().filter(|k| k.contains("_64_")).count();
    if assets64 > 0 && (assets32 as i64) < (assets64 as f64 * 0.9) as i64 {
        return false;
    }
    true
}

fn convert_one(swf_file: &Path, output_directory: &Path, file_output_directory: &Path, file_name: &str) -> Result<bool> {
    let binary_output_path = file_output_directory.join(format!("{file_name}_binaryData"));
    common::block_on(crate::ffdec_extractor::extract_swf(swf_file, &binary_output_path))?;

    let binary_data = binary_output_path.join("binaryData");
    if !binary_data.is_dir() {
        return Ok(false);
    }

    let csv_path = binary_output_path.join("symbolClass").join("symbols.csv");
    let canonical_mapping = crate::assets::build_canonical_mapping(&csv_path);
    let image_sources = crate::assets::parse_debug_xml(&csv_path);

    let index_data = common::first_file(&binary_data, "*_index.bin")?.and_then(|f| index::parse_pets_index_file(&f));

    let assets_file = common::first_file(&binary_data, "*_assets.bin")?;
    let manifest_file = common::first_file(&binary_data, "*_manifest.bin")?;
    let parsed_assets = match (assets_file, manifest_file) {
        (Some(assets_file), Some(manifest_file)) => Some(assets::parse_assets_file(
            &assets_file,
            Some(&image_sources),
            &manifest_file,
            &csv_path,
            &habbo_core::paths::file_name(swf_file),
            file_output_directory,
        )),
        _ => None,
    };

    let logic_result = crate::furni::get_logic_data(&binary_output_path);
    let visualization_result = get_visualizations_data(&binary_output_path);
    let logic_data = logic_result?;
    let mut visualizations = visualization_result?;
    let palettes = palette::extract_palettes(&binary_output_path)?;

    let Some(index_data) = index_data else { return Ok(false) };

    let images_directory = binary_output_path.join("images");
    let tmp_directory = binary_output_path.join("tmp");
    let image_mapping = parsed_assets.as_ref().map(|p| p.image_mapping.clone()).unwrap_or_default();
    spritesheet::restore_images_from_tmp(&tmp_directory, &images_directory, &image_mapping);

    let mut asset_data = parsed_assets.map(|p| p.assets);
    let keep_size32 = should_keep_size32(visualizations.as_deref(), asset_data.as_ref());
    if !keep_size32 {
        if let Some(list) = visualizations.as_mut() {
            list.retain(|v| v.size != 32);
        }
        let Some(assets) = asset_data.as_mut() else { bail!("{NULL_REFERENCE}") };
        assets.retain(|key, _| !key.contains("_32_"));
    }

    let images = spritesheet::load_images_filtered(&images_directory, |name| !keep_size32 && name.contains("_32_"))?;
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
    // `logicData.Model` with no logic file throws in C#.
    let Some(logic_data) = logic_data else { bail!("{NULL_REFERENCE}") };

    let visible: Option<Vec<&Visualization>> = visualizations.as_ref().and_then(|list| {
        let kept: Vec<&Visualization> = list.iter().filter(|v| !v.is_empty()).collect();
        (!kept.is_empty()).then_some(kept)
    });

    let full_object = Obj::new()
        .opt("type", index_data.r#type.clone().map(J::Str))
        .opt("name", index_data.name.clone().map(J::Str))
        .opt("logicType", index_data.logic_type.clone().map(J::Str))
        .opt("visualizationType", index_data.visualization_type.clone().map(J::Str))
        .opt("assets", asset_data.as_ref().map(assets::assets_json))
        .put("palettes", palette::palettes_json(&palettes))
        .put("logic", logic_object_json(Some(&logic_data), FloatStyle::Default))
        .opt("visualizations", visible.map(|list| J::Arr(list.iter().map(|v| v.to_json()).collect())))
        .put("spritesheet", sprite_sheet_data.to_json(FloatStyle::Default))
        .build();

    let json_content = pets_json_text(&full_object)?;
    std::fs::write(&json_output_path, json_content)?;

    common::bundle_nitro_file(file_output_directory, file_name, output_directory, &sprite_sheet_path)?;
    Ok(true)
}

/// The pets json text: indented, then the C# `.Replace("[\n  ", "[").Replace("\n  ]", "]")
/// .Replace("\n    ", " ")` compaction (applied to the platform's line breaks as-is).
pub fn pets_json_text(value: &J) -> Result<String> {
    Ok(value.to_string_indented()?.replace("[\n  ", "[").replace("\n  ]", "]").replace("\n    ", " "))
}

fn get_visualizations_data(binary_output_path: &Path) -> Result<Option<Vec<Visualization>>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_like_the_csharp_replace_chain() {
        let j = Obj::new()
            .put("a", J::Arr(vec![J::int(1), J::int(2)]))
            .put("b", Obj::new().put("c", Obj::new().put("d", J::int(3)).build()).build())
            .build();
        let text = pets_json_text(&j).unwrap();
        let expected = if cfg!(windows) {
            "{\r\n  \"a\": [\r\n    1,\r\n    2\r\n  ],\r\n  \"b\": {\r\n    \"c\": {\r\n      \"d\": 3\r\n    }\r\n  }\r\n}"
                .replace("[\n  ", "[")
                .replace("\n  ]", "]")
                .replace("\n    ", " ")
        } else {
            "{\n  \"a\": [  1, 2],\n  \"b\": { \"c\": {   \"d\": 3 }\n  }\n}".to_string()
        };
        assert_eq!(text, expected);
    }

    #[test]
    fn size32_rules() {
        let vis = |size: i32, anims: usize| -> Visualization {
            let xml = format!(
                "<v size=\"{size}\"><animations>{}</animations></v>",
                (0..anims).map(|i| format!("<animation id=\"{i}\"/>")).collect::<String>()
            );
            let doc = crate::xml::parse(&xml).unwrap();
            Visualization::from_xml(doc.root_element()).unwrap()
        };
        let mut assets = IndexMap::new();
        for i in 0..10 {
            assets.insert(format!("dog_64_a_{i}"), Asset::default());
            assets.insert(format!("dog_32_a_{i}"), Asset::default());
        }
        let list = vec![vis(64, 3), vis(32, 3)];
        assert!(should_keep_size32(Some(&list), Some(&assets)));
        let fewer = vec![vis(64, 3), vis(32, 2)];
        assert!(!should_keep_size32(Some(&fewer), Some(&assets)));
        assets.shift_remove("dog_32_a_0");
        assets.shift_remove("dog_32_a_1");
        assert!(!should_keep_size32(Some(&list), Some(&assets)));
        assert!(!should_keep_size32(None, Some(&assets)));
    }
}
