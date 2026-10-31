//! `AssetBundleWriter`: writes a converted asset as .nitro or .hab (picked by the Hotel Tools
//! option) into a nitro/ or hab/ folder under the tool's output folder. Every bundle is json +
//! a lossless WebP sheet; a PNG sheet is converted on the way. An Asset Workspace keeps its own
//! layout, so no subfolder there.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use anyhow::Result;
use habbo_core::outln;
use habbo_core::workspace::runtime;

use crate::{bundler::NitroBundler, hab, sheet_webp, FileEntry};

static EXTENSION: RwLock<&'static str> = RwLock::new(".hab");

/// ".nitro" or ".hab"; set by the Hotel Tools option before a conversion starts.
pub fn extension() -> &'static str {
    *EXTENSION.read().unwrap()
}

pub fn set_extension(extension: &str) {
    *EXTENSION.write().unwrap() = if extension.eq_ignore_ascii_case(".nitro") { ".nitro" } else { ".hab" };
}

pub fn label() -> String {
    format!("{} (json + WebP Lossless)", extension().trim_start_matches('.').to_uppercase())
}

/// The nitro/ or hab/ folder of an output folder.
pub fn folder(output_directory: &Path) -> PathBuf {
    folder_for(output_directory, extension())
}

pub fn folder_for(output_directory: &Path, extension: &str) -> PathBuf {
    if let Some(paths) = &runtime::router().paths {
        if is_inside(&paths.root, output_directory) {
            return output_directory.to_path_buf();
        }
    }
    output_directory.join(extension.trim_start_matches('.'))
}

pub fn output_path(output_directory: &Path, name: &str) -> PathBuf {
    folder(output_directory).join(format!("{name}{}", extension()))
}

/// True when this asset is already converted.
pub fn exists(output_directory: &Path, name: &str) -> bool {
    output_path(output_directory, name).is_file()
}

/// The bundle bytes; PNG sheets become lossless WebP first.
pub fn build(extension: &str, name: &str, files: Vec<FileEntry>) -> Result<Vec<u8>> {
    let webp_files = files
        .into_iter()
        .map(|file| sheet_webp::to_webp_entry(file, name))
        .collect::<Result<Vec<_>>>()?;

    if extension == ".hab" {
        return hab::write(name, &webp_files);
    }

    let mut bundler = NitroBundler::new();
    for (file_name, data) in webp_files {
        bundler.add_file(&file_name, data)?;
    }
    bundler.to_buffer()
}

/// Writes the bundle into the nitro/ or hab/ folder of `output_directory`; returns its size.
/// `write` replaces the plain file write (e.g. `habbo_core::workspace::write_output`).
pub fn write(
    output_directory: &Path,
    name: &str,
    files: Vec<FileEntry>,
    write: Option<&dyn Fn(&Path, &[u8]) -> Result<()>>,
) -> Result<u64> {
    let folder = folder(output_directory);
    std::fs::create_dir_all(&folder)?;

    let extension = extension();
    let data = build(extension, name, files)?;
    let path = folder.join(format!("{name}{extension}"));

    match write {
        Some(write) => write(&path, &data)?,
        None => std::fs::write(&path, &data)?,
    }

    outln!("📦 Generated {name}{extension} -> {}", folder.display());
    Ok(data.len() as u64)
}

/// The json and sheet of a converted asset, skipping the ones that are missing.
pub fn read_files(paths: &[PathBuf]) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push((habbo_core::paths::file_name(path), std::fs::read(path)?));
        }
    }
    Ok(files)
}

fn is_inside(root: &Path, path: &Path) -> bool {
    let full = |p: &Path| {
        let mut s = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().into_owned();
        while s.ends_with(['\\', '/']) {
            s.pop();
        }
        s.push(std::path::MAIN_SEPARATOR);
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    full(path).starts_with(&full(root))
}
