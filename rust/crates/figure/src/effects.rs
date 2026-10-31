//! `SWF_Effects_To_Nitro`: avatar effect SWFs -> .nitro / .hab.

use std::path::{Path, PathBuf};

use anyhow::Result;
use habbo_core::paths::HabboAssetFolder;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_core::{console, outln};
use habbo_nitro::writer;

use crate::animation::{animations_node, parse_animation_file, Animations};
use crate::assets::{aliases_node, assets_node, parse_effect_assets_file, Aliases, Asset, AssetData, Assets};
use crate::driver::{self, LoopOptions};
use crate::extract::{extract_swf, restore_images_from_tmp};
use crate::spritesheet::{generate_sprite_sheet, SpriteSheetData};
use crate::stj::{to_string_indented, Node, ObjectBuilder};
use crate::symbols::{get_image_mapping, parse_debug_xml};

fn output_directory() -> PathBuf {
    runtime::router().asset_directory(WorkspaceAssetKind::Effects, Path::new("SWFCompiler").join("effects"))
}

/// `SWF_Effects_To_Nitro.ConvertSwfFilesAsync`.
pub async fn convert_effects() -> Result<()> {
    if let Err(error) = convert_swf_files().await {
        outln!("❌ Error during SWF conversion: {error}");
    }
    Ok(())
}

async fn convert_swf_files() -> Result<()> {
    outln!("Do you want (H) Habbo_Default effects or (I) Imported effects? (Default is H):");
    let import_directory = if console::read_line().trim().to_uppercase() == "I" {
        Path::new("SWFCompiler").join("import").join("effects")
    } else {
        HabboAssetFolder::EFFECTS.swf_source()
    };
    std::fs::create_dir_all(&import_directory)?;

    outln!("✅ Converting SWF to Nitro from source {}", import_directory.display());

    let output_directory = output_directory();
    std::fs::create_dir_all(&output_directory)?;
    let swf_files = crate::util::get_files(&import_directory, "*.swf")?;

    if swf_files.is_empty() {
        outln!("ℹ️ No SWF files found in {}.", import_directory.display());
        return Ok(());
    }

    outln!("✅ Found {} effect SWF files to convert.", swf_files.len());

    let out = output_directory.clone();
    driver::run(
        swf_files,
        output_directory,
        LoopOptions { title: "SWF Effects", skip_human_fx: false, progress_every: 25, counter_width: 4 },
        move |swf_file| {
            let out = out.clone();
            async move { process_swf_file(&swf_file, &out).await }
        },
    )
    .await
}

/// `ProcessSwfFileAsync`: `Ok(true)` when the bundle was written.
pub async fn process_swf_file(swf_file: &Path, output_directory: &Path) -> Result<bool> {
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
            outln!("❌ Error generating Nitro file for {file_name}: {error}");
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
    let effects_mapping = get_image_mapping(&csv_path);

    let asset_data = get_asset_data(binary_output_path, &csv_path)?;
    let animations = parse_animation_file(&binary_output_path.join("binaryData"))?;

    let images_directory = binary_output_path.join("images");
    let tmp_directory = binary_output_path.join("tmp");
    restore_images_from_tmp(&tmp_directory, &images_directory, &effects_mapping)?;
    let images = driver::load_images(&images_directory)?;

    let mut sprite_sheet: Option<(PathBuf, SpriteSheetData)> = None;
    if !images.is_empty() {
        // The C# code swallows any sheet error (`catch { }`) and writes the json without it.
        if let Ok(result) = generate_sprite_sheet(&images, file_output_directory, file_name, 14000, 14000) {
            sprite_sheet = result;
        }
    }

    let assets = effect_assets(asset_data.assets, sprite_sheet.as_ref().map(|(_, d)| d), file_name);

    let json = effects_json(
        assets.as_ref(),
        &asset_data.aliases,
        &animations,
        file_name,
        sprite_sheet.as_ref().map(|(_, d)| d.to_node()),
    );
    std::fs::write(file_output_directory.join(format!("{file_name}.json")), json)?;
    driver::bundle_nitro_file(
        file_output_directory,
        file_name,
        output_directory,
        sprite_sheet.as_ref().map(|(p, _)| p.as_path()),
    )?;
    Ok(true)
}

fn get_asset_data(binary_output_path: &Path, csv_path: &Path) -> Result<AssetData> {
    let Some(manifest) = driver::find_manifest(binary_output_path)? else {
        return Ok(AssetData::default());
    };
    Ok(parse_effect_assets_file(&manifest, csv_path))
}

/// The manifest assets; when there are none, one `{x:0,y:0}` asset per sheet frame (with the
/// `<fileName>_` prefix removed). `None` (omitted from the json) when still empty.
pub fn effect_assets(assets: Assets, sheet: Option<&SpriteSheetData>, file_name: &str) -> Option<Assets> {
    let mut assets = assets;
    if assets.is_empty() {
        if let Some(sheet) = sheet.filter(|s| !s.frames.is_empty()) {
            let prefix = format!("{file_name}_");
            let mut derived = Assets::new();
            for frame_name in sheet.frames.keys() {
                let lookup = frame_name.strip_prefix(&prefix).unwrap_or(frame_name);
                derived.entry(lookup.to_string()).or_insert(Asset { x: 0, y: 0, source: None });
            }
            if !derived.is_empty() {
                assets = derived;
            }
        }
    }
    (!assets.is_empty()).then_some(assets)
}

/// `new { assets, aliases, animations, name, spritesheet }` serialised with System.Text.Json
/// (null `assets` / `spritesheet` omitted).
pub fn effects_json(
    assets: Option<&Assets>,
    aliases: &Aliases,
    animations: &Animations,
    name: &str,
    spritesheet: Option<Node>,
) -> String {
    let node = ObjectBuilder::new()
        .opt("assets", assets.map(assets_node))
        .put("aliases", aliases_node(aliases))
        .put("animations", animations_node(animations))
        .put("name", Node::str(name))
        .opt("spritesheet", spritesheet)
        .build();
    to_string_indented(&node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spritesheet::{FrameData, RectData};

    #[test]
    fn assets_fall_back_to_sheet_frames() {
        let mut frames = indexmap::IndexMap::new();
        for name in ["Dance1_h_std_a", "other", "h_std_a"] {
            frames.insert(name.to_string(), FrameData { frame: RectData { x: 0, y: 0, w: 1, h: 1 } });
        }
        let sheet = SpriteSheetData { frames, image: "Dance1.png".into(), width: 1, height: 1 };
        let assets = effect_assets(Assets::new(), Some(&sheet), "Dance1").unwrap();
        assert_eq!(assets.keys().collect::<Vec<_>>(), vec!["h_std_a", "other"]);
        assert!(effect_assets(Assets::new(), None, "Dance1").is_none());
    }

    #[test]
    fn json_omits_null_assets_and_sheet() {
        let json = effects_json(None, &Aliases::new(), &Animations::new(), "fx", None).replace("\r\n", "\n");
        assert_eq!(json, "{\n  \"aliases\": {},\n  \"animations\": {},\n  \"name\": \"fx\"\n}");
    }
}
