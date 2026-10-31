//! `FFDEC/FfdecExtractor`: exports a furniture SWF (native reader, FFDec fallback) and
//! renames the exported PNGs after their symbols.csv names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use habbo_core::outln;
use indexmap::IndexMap;

use crate::net::{self, lower_invariant, try_parse_int};
use crate::spritesheet::move_file;

/// `FfdecExtractor.ExtractSWFAsync`: clears old PNGs, exports `image,binarydata,symbolClass`
/// into `output_directory`, then rebuilds images/ from symbolClass/symbols.csv (the PNGs
/// that matched no CSV row stay in tmp/ for `ImageRestorer`).
pub async fn extract_swf(swf_file_path: &Path, output_directory: &Path) -> Result<()> {
    clear_output_directory(output_directory);

    let swf = swf_file_path.to_path_buf();
    habbo_swf::extract_with_fallback(
        swf_file_path,
        output_directory,
        move |dir: PathBuf| {
            let swf = swf.clone();
            async move { run_ffdec(&dir, &swf).await }
        },
        true,
    )
    .await?;

    let csv_path = output_directory.join("symbolClass").join("symbols.csv");
    rebuild_images_from_csv(output_directory, &csv_path)?;
    Ok(())
}

/// `-onerror ignore -export image,binarydata,symbolClass "{dir}" "{swf}"`, 60 s timeout.
async fn run_ffdec(dir: &Path, swf: &Path) -> Result<()> {
    let args: Vec<std::ffi::OsString> = vec![
        "-onerror".into(),
        "ignore".into(),
        "-export".into(),
        "image,binarydata,symbolClass".into(),
        dir.as_os_str().to_owned(),
        swf.as_os_str().to_owned(),
    ];
    // Exit code and output are ignored, like the C# version; a timeout kills the process.
    let _ = habbo_core::ffdec::run(args, Duration::from_millis(60000)).await?;
    Ok(())
}

fn clear_output_directory(output_directory: &Path) {
    if !output_directory.is_dir() {
        return;
    }
    for file in net::get_files_recursive(output_directory, "*.png").unwrap_or_default() {
        if let Err(error) = std::fs::remove_file(&file) {
            outln!("❌ Error deleting {}: {error}", file.display());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// `FfdecExtractor.RebuildImagesFromCsvAsync`: moves every exported PNG to `tmp/`, then
/// moves the one of each CSV id to `images/{name}.png` ("{id}.png" for ids with several
/// names, using the row marked source or the first one; "{id}_{name}.png" otherwise).
/// Returns CSV name → image path.
pub fn rebuild_images_from_csv(image_dir: &Path, csv_file_path: &Path) -> Result<HashMap<String, PathBuf>> {
    let mut output = HashMap::new();

    let tmp_dir = image_dir.join("tmp");
    if tmp_dir.is_dir() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    for file in net::get_files_recursive(image_dir, "*.png")? {
        let relative = file.strip_prefix(image_dir).unwrap_or(&file).to_path_buf();
        let destination = tmp_dir.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        move_file(&file, &destination)?;
    }

    let mut file_lookup: HashMap<String, PathBuf> = HashMap::new();
    for file in net::get_files_recursive(&tmp_dir, "*.png")? {
        let key = habbo_core::paths::file_stem(&file).replace('"', "");
        if file_lookup.contains_key(&key) {
            bail!("An item with the same key has already been added. Key: {key}");
        }
        file_lookup.insert(key, file);
    }

    let target_images_folder = image_dir.join("images");
    std::fs::create_dir_all(&target_images_folder)?;

    let mut groups: IndexMap<i32, Vec<CsvMapping>> = IndexMap::new();
    for mapping in parse_csv(csv_file_path)? {
        groups.entry(mapping.id).or_default().push(mapping);
    }

    for (id, mappings) in &groups {
        let lookup_key = if mappings.len() > 1 { id.to_string() } else { format!("{id}_{}", mappings[0].name) };
        let Some(original) = file_lookup.get(&lookup_key) else { continue };
        let ext = net::extension_with_dot(original);
        let source = if mappings.len() > 1 {
            mappings.iter().find(|m| m.kind == MappingType::Source).unwrap_or(&mappings[0])
        } else {
            &mappings[0]
        };
        let target_path = target_images_folder.join(format!("{}{ext}", source.name));
        if let Err(error) = move_file(original, &target_path) {
            outln!("❌ Failed to copy file for group with ID {id}: {error}");
            continue;
        }
        for mapping in mappings {
            output.insert(mapping.name.clone(), target_path.clone());
        }
    }
    Ok(output)
}

fn parse_csv(csv_file_path: &Path) -> Result<Vec<CsvMapping>> {
    let mut result = Vec::new();
    if !csv_file_path.is_file() {
        outln!("❌ CSV file not found: {}", csv_file_path.display());
        return Ok(result);
    }
    for line in net::read_all_lines(csv_file_path)? {
        if net::is_null_or_white_space(&line) {
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
        let lower = lower_invariant(&comment);
        if ["manifest", "assets", "logic", "visualization", "index"].iter().any(|w| lower.contains(w)) {
            continue;
        }
        let kind = if lower.contains("source") { MappingType::Source } else { MappingType::Main };
        result.push(CsvMapping { id, name, kind });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_images_from_csv() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path();
        std::fs::create_dir_all(out.join("images")).unwrap();
        std::fs::create_dir_all(out.join("symbolClass")).unwrap();
        for name in ["1.png", "2_chair_chair_64_b_0_0.png", "3_chair_chair_64_c_0_0.png", "9_other.png"] {
            std::fs::write(out.join("images").join(name), b"png").unwrap();
        }
        std::fs::write(
            out.join("symbolClass").join("symbols.csv"),
            "0;chair\n1;chair_chair_64_a_0_0\n1;chair_chair_64_a_2_0 <= source\n2;\"chair_chair_64_b_0_0\"\n4;chair_chair_assets <= assets\n",
        )
        .unwrap();

        let mapping = rebuild_images_from_csv(out, &out.join("symbolClass").join("symbols.csv")).unwrap();
        let images = out.join("images");
        assert!(images.join("chair_chair_64_a_2_0.png").is_file());
        assert!(images.join("chair_chair_64_b_0_0.png").is_file());
        assert!(!images.join("1.png").exists());
        assert_eq!(mapping["chair_chair_64_a_0_0"], images.join("chair_chair_64_a_2_0.png"));
        // Unmatched PNGs wait in tmp/ for ImageRestorer.
        assert!(out.join("tmp").join("images").join("3_chair_chair_64_c_0_0.png").is_file());
        assert!(out.join("tmp").join("images").join("9_other.png").is_file());

        let mut restore = HashMap::new();
        restore.insert("3".to_string(), "chair_chair_64_c_0_0".to_string());
        crate::spritesheet::restore_images_from_tmp(&out.join("tmp"), &images, &restore);
        assert!(images.join("chair_chair_64_c_0_0.png").is_file());
        assert!(out.join("tmp").join("images").join("9_other.png").is_file());
    }
}
