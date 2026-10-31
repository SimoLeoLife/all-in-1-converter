//! `FfdecExtractorClothes` / `FfdecExtractorEffects` (identical in the C# code) and
//! `ImageRestorer` / `ImageRestorerClothes`: export the SWF (native reader, FFDec fallback),
//! then rename the exported PNGs after their symbolClass names.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use habbo_core::outln;
use indexmap::IndexMap;

use crate::symbols::CiMap;
use crate::util::{
    get_files_recursive, is_null_or_white_space, move_file, move_file_overwrite, read_all_lines, try_parse_int,
};

/// `ExtractSWFAsync`: clears old PNGs, exports `image,binarydata,symbolClass` into
/// `output_directory`, then rebuilds `images/` from `symbolClass/symbols.csv`.
pub async fn extract_swf(swf_file_path: &Path, output_directory: &Path) -> Result<()> {
    clear_output_directory(output_directory);

    let swf = swf_file_path.to_path_buf();
    habbo_swf::extract_with_fallback(
        swf_file_path,
        output_directory,
        |dir: PathBuf| {
            let swf = swf.clone();
            async move { run_ffdec_command(&dir, &swf).await }
        },
        true,
    )
    .await?;

    let csv_path = output_directory.join("symbolClass").join("symbols.csv");
    rebuild_images_from_csv(output_directory, &csv_path)?;
    Ok(())
}

/// `-onerror ignore -export image,binarydata,symbolClass "<dir>" "<swf>"`, output discarded,
/// killed after 60 seconds.
async fn run_ffdec_command(dir: &Path, swf: &Path) -> Result<()> {
    let args: Vec<std::ffi::OsString> = vec![
        "-onerror".into(),
        "ignore".into(),
        "-export".into(),
        "image,binarydata,symbolClass".into(),
        dir.as_os_str().to_owned(),
        swf.as_os_str().to_owned(),
    ];
    habbo_core::ffdec::run(args, Duration::from_secs(60)).await?;
    Ok(())
}

fn clear_output_directory(output_directory: &Path) {
    if !output_directory.is_dir() {
        return;
    }
    for file in get_files_recursive(output_directory, "*.png").unwrap_or_default() {
        if let Err(error) = std::fs::remove_file(&file) {
            outln!("❌ Error deleting {}: {error}", file.display());
        }
    }
}

/// `RebuildImagesFromCsvAsync`: moves every exported PNG into `tmp/`, then moves the one of
/// each symbol id to `images/<name>.png`. Returns name -> new path.
pub fn rebuild_images_from_csv(image_dir: &Path, csv_file_path: &Path) -> Result<IndexMap<String, PathBuf>> {
    let mut output_mappings = IndexMap::new();

    let tmp_dir = image_dir.join("tmp");
    if tmp_dir.is_dir() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    for file in get_files_recursive(image_dir, "*.png")? {
        let relative = file.strip_prefix(image_dir).unwrap_or(&file);
        let destination = tmp_dir.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        move_file(&file, &destination)?;
    }

    // ToDictionary: a duplicate key throws.
    let mut file_lookup: IndexMap<String, PathBuf> = IndexMap::new();
    for file in get_files_recursive(&tmp_dir, "*.png")? {
        let key = habbo_core::paths::file_stem(&file).replace('"', "");
        if file_lookup.contains_key(&key) {
            bail!("An item with the same key has already been added. Key: {key}");
        }
        file_lookup.insert(key, file);
    }

    let target_images_folder = image_dir.join("images");
    std::fs::create_dir_all(&target_images_folder)?;

    let csv_mappings = parse_csv(csv_file_path)?;
    let mut groups: IndexMap<i32, Vec<CsvMapping>> = IndexMap::new();
    for mapping in csv_mappings {
        groups.entry(mapping.id).or_default().push(mapping);
    }

    for (id, mappings) in groups {
        let lookup_key = if mappings.len() > 1 { id.to_string() } else { format!("{id}_{}", mappings[0].name) };
        let Some(original_file_path) = file_lookup.get(&lookup_key) else { continue };
        let ext = original_file_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();

        let source_mapping = if mappings.len() > 1 {
            mappings.iter().find(|m| m.kind == MappingType::Source).unwrap_or(&mappings[0])
        } else {
            &mappings[0]
        };

        let target_path = target_images_folder.join(format!("{}{ext}", source_mapping.name));
        if let Err(error) = move_file(original_file_path, &target_path) {
            outln!("❌ Failed to copy file for group with ID {id}: {error}");
            continue;
        }

        for mapping in &mappings {
            output_mappings.insert(mapping.name.clone(), target_path.clone());
        }
    }

    Ok(output_mappings)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MappingType {
    Source,
    Main,
}

#[derive(Debug, Clone)]
struct CsvMapping {
    id: i32,
    name: String,
    kind: MappingType,
}

fn parse_csv(csv_file_path: &Path) -> Result<Vec<CsvMapping>> {
    if !csv_file_path.is_file() {
        outln!("❌ CSV file not found: {}", csv_file_path.display());
        return Ok(Vec::new());
    }
    Ok(parse_csv_lines(&read_all_lines(csv_file_path)?))
}

fn parse_csv_lines(lines: &[String]) -> Vec<CsvMapping> {
    let mut result = Vec::new();
    for line in lines {
        if is_null_or_white_space(line) {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() < 2 {
            continue;
        }
        let Some(id) = try_parse_int(Some(parts[0].trim())) else { continue };
        if id == 0 {
            continue;
        }

        let name_part = parts[1].trim().replace('"', "");
        let (name, comment) = match name_part.find(" <=") {
            Some(index) => (name_part[..index].trim().to_string(), name_part[index + 3..].trim().to_string()),
            None => (name_part.clone(), String::new()),
        };

        let lower = comment.to_lowercase();
        if ["manifest", "assets", "logic", "visualization", "index"].iter().any(|w| lower.contains(w)) {
            continue;
        }
        let kind = if lower.contains("source") { MappingType::Source } else { MappingType::Main };
        result.push(CsvMapping { id, name, kind });
    }
    result
}

/// `ImageRestorer.RestoreImagesFromTmpAsync` (the one both C# compilers call): moves each
/// left-over tmp PNG whose id (text before the first `_`) is a mapping key to
/// `images/<mapped name>.png`, overwriting.
pub fn restore_images_from_tmp(tmp_directory: &Path, images_directory: &Path, image_mapping: &CiMap) -> Result<()> {
    restore(tmp_directory, images_directory, image_mapping, false)
}

/// `ImageRestorerClothes.RestoreImagesFromTmpAsync`: the same, with extra diagnostics when the
/// mapping is empty. (Not called by the C# clothes compiler, which uses `ImageRestorer`.)
pub fn restore_images_from_tmp_clothes(tmp_directory: &Path, images_directory: &Path, image_mapping: &CiMap) -> Result<()> {
    restore(tmp_directory, images_directory, image_mapping, true)
}

fn restore(tmp_directory: &Path, images_directory: &Path, image_mapping: &CiMap, verbose: bool) -> Result<()> {
    if !tmp_directory.is_dir() {
        outln!("❌ TMP directory not found: {}", tmp_directory.display());
        return Ok(());
    }
    if image_mapping.is_empty() {
        if verbose {
            outln!("tmpDirectory : {}", tmp_directory.display());
            outln!("imagesDirectory: {}", images_directory.display());
        }
        outln!("❌ In-memory image mapping is empty.");
        return Ok(());
    }

    std::fs::create_dir_all(images_directory)?;

    for tmp_file in get_files_recursive(tmp_directory, "*.png")? {
        let file_name = habbo_core::paths::file_stem(&tmp_file);
        let extension = tmp_file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let id = file_name.split('_').next().unwrap_or("");
        let Some(correct_name) = image_mapping.get(id) else { continue };

        let new_file_path = images_directory.join(format!("{correct_name}{extension}"));
        if let Err(error) = move_file_overwrite(&tmp_file, &new_file_path) {
            outln!("❌ Error moving {file_name} to {}: {error}", new_file_path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_lines_skip_metadata_and_tag_sources() {
        let lines = crate::util::split_lines(
            "0;\"hh_fx\"\n1;\"hh_fx_manifest <= manifest\"\n2;\"a <= source\"\n2;\"b <= main\"\nx;\"bad\"\n3;\"c\"\n",
        );
        let parsed = parse_csv_lines(&lines);
        let flat: Vec<(i32, &str, MappingType)> = parsed.iter().map(|m| (m.id, m.name.as_str(), m.kind)).collect();
        assert_eq!(
            flat,
            vec![(2, "a", MappingType::Source), (2, "b", MappingType::Main), (3, "c", MappingType::Main)]
        );
    }

    #[test]
    fn rebuild_moves_images_by_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::create_dir_all(root.join("symbolClass")).unwrap();
        std::fs::write(root.join("images").join("2.png"), b"two").unwrap();
        std::fs::write(root.join("images").join("3_c.png"), b"three").unwrap();
        std::fs::write(root.join("images").join("9_left.png"), b"nine").unwrap();
        let csv = root.join("symbolClass").join("symbols.csv");
        std::fs::write(&csv, "2;\"b\"\n2;\"a <= source\"\n3;\"c\"\n").unwrap();

        let mappings = rebuild_images_from_csv(root, &csv).unwrap();
        assert_eq!(std::fs::read(root.join("images").join("a.png")).unwrap(), b"two");
        assert_eq!(std::fs::read(root.join("images").join("c.png")).unwrap(), b"three");
        assert!(root.join("tmp").join("images").join("9_left.png").is_file());
        assert_eq!(mappings.len(), 3);

        let mut map = CiMap::new();
        map.insert("9", "restored".into());
        restore_images_from_tmp(&root.join("tmp"), &root.join("images"), &map).unwrap();
        assert_eq!(std::fs::read(root.join("images").join("restored.png")).unwrap(), b"nine");
    }
}
