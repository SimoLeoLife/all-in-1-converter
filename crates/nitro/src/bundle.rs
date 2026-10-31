//! `NitroBundle`: reads a .nitro (or .hab) bundle, saves every entry under
//! NitroCompiler/extracted/furni/<name>/ and keeps the json and the sheet.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine;
use flate2::read::{DeflateDecoder, GzDecoder};
use habbo_core::outln;
use serde_json::Value;

use crate::{hab, FileEntry};

pub struct NitroBundle {
    extracted_base_path: PathBuf,
    json_file: Option<Value>,
    base_texture: Option<String>,
    texture_extension: String,
}

impl NitroBundle {
    /// Parses and extracts into NitroCompiler/extracted/furni (relative to the working directory).
    pub fn new(buffer: &[u8]) -> Self {
        let base = std::env::current_dir().unwrap_or_default().join("NitroCompiler").join("extracted").join("furni");
        Self::with_extract_dir(buffer, base)
    }

    pub fn with_extract_dir(buffer: &[u8], extracted_base_path: PathBuf) -> Self {
        let mut bundle = Self { extracted_base_path, json_file: None, base_texture: None, texture_extension: ".png".into() };
        bundle.parse(buffer);
        bundle
    }

    pub fn json_file(&self) -> Option<&Value> {
        self.json_file.as_ref()
    }

    /// The sheet as base64 (as the C# version kept it).
    pub fn base_texture(&self) -> Option<&str> {
        self.base_texture.as_deref()
    }

    pub fn texture_extension(&self) -> &str {
        &self.texture_extension
    }

    pub fn parse(&mut self, buffer: &[u8]) {
        if hab::is_hab(buffer) {
            match hab::read(buffer) {
                Ok((_, files)) => {
                    for (name, data) in files {
                        self.add_entry(&name, data);
                    }
                }
                Err(error) => outln!("❌ Failed to read .hab bundle: {error}"),
            }
            self.warn_without_texture();
            return;
        }

        match read_nitro_entries(buffer) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok((name, data)) => self.add_entry(&name, data),
                        Err(message) => outln!("{message}"),
                    }
                }
            }
            Err(message) => outln!("{message}"),
        }
        self.warn_without_texture();
    }

    fn warn_without_texture(&self) {
        if self.base_texture.as_deref().unwrap_or("").is_empty() {
            outln!("⚠️ Warning: Base64 image data is null or empty.");
        }
    }

    fn add_entry(&mut self, file_name: &str, data: Vec<u8>) {
        let lower = file_name.to_lowercase();
        if lower.ends_with(".json") {
            self.json_file = serde_json::from_slice(&data).ok();
        } else if lower.ends_with(".png") {
            self.texture_extension = ".png".into();
            self.base_texture = Some(base64::engine::general_purpose::STANDARD.encode(&data));
        } else if lower.ends_with(".webp") {
            self.texture_extension = ".webp".into();
            self.base_texture = Some(base64::engine::general_purpose::STANDARD.encode(&data));
        }

        if let Err(error) = self.save_extracted_file(file_name, &data) {
            outln!("❌ Failed to save {file_name}: {error}");
            return;
        }
        outln!("✅ Extracted: {file_name}");
    }

    fn save_extracted_file(&self, file_name: &str, data: &[u8]) -> Result<()> {
        let base_name = habbo_core::paths::file_stem(Path::new(file_name));
        let directory = self.extracted_base_path.join(base_name);
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join(file_name), data)?;
        Ok(())
    }
}

/// The entries of a .nitro container, each decompressed. The outer error is a corrupt
/// container (stops reading, like the C# `return`); an inner error is one entry that failed.
pub fn read_nitro_entries(buffer: &[u8]) -> std::result::Result<Vec<std::result::Result<FileEntry, String>>, String> {
    let mut reader = crate::reader::BinaryReader::new(buffer);
    let mut out = Vec::new();
    let mut file_count = reader.read_short().map_err(|e| format!("❌ Error: {e}"))?;

    while file_count > 0 {
        let name_length = reader.read_short().map_err(|e| format!("❌ Error: {e}"))?;
        if name_length <= 0 || name_length as usize > reader.remaining() {
            return Err("❌ Error: Corrupted fileNameLength detected!".into());
        }
        let name = String::from_utf8_lossy(reader.read_bytes(name_length as usize).unwrap()).into_owned();

        let file_length = reader.read_int().map_err(|e| format!("❌ Error: {e}"))?;
        if file_length < 0 || file_length as usize > reader.remaining() {
            return Err(format!("❌ Error: Invalid file length ({file_length}) for {name}"));
        }
        let data = reader.read_bytes(file_length as usize).unwrap();
        if file_length > 0 {
            out.push(
                detect_and_decompress(data)
                    .map(|bytes| (name.clone(), bytes))
                    .map_err(|e| format!("❌ Failed to process file {name}: {e}")),
            );
        }
        file_count -= 1;
    }
    Ok(out)
}

/// Every entry of a .nitro or .hab, decompressed, without extracting anything to disk.
pub fn read_entries(buffer: &[u8]) -> Result<Vec<FileEntry>> {
    if hab::is_hab(buffer) {
        return Ok(hab::read(buffer)?.1);
    }
    let entries = read_nitro_entries(buffer).map_err(anyhow::Error::msg)?;
    entries.into_iter().map(|e| e.map_err(anyhow::Error::msg)).collect()
}

/// Entries starting 78 9C have their zlib header skipped and are raw-inflated; otherwise
/// raw deflate is tried, then gzip (the order the C# code used).
pub fn detect_and_decompress(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > 2 && data[0] == 0x78 && data[1] == 0x9C {
        return inflate_raw(&data[2..]).context("❌ Custom Zlib decompression failed");
    }
    if let Ok(bytes) = inflate_raw(data) {
        return Ok(bytes);
    }
    match inflate_gzip(data) {
        Ok(bytes) => Ok(bytes),
        Err(error) => bail!("❌ Decompression failed using both Zlib and GZip: {error}"),
    }
}

fn inflate_raw(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

fn inflate_gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    GzDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundler::NitroBundler;

    #[test]
    fn nitro_roundtrip_and_extract() {
        let mut bundler = NitroBundler::new();
        bundler.add_file("chair.json", br#"{"name":"chair"}"#.to_vec()).unwrap();
        bundler.add_file("chair.png", vec![1, 2, 3]).unwrap();
        let buffer = bundler.to_buffer().unwrap();

        let entries = read_entries(&buffer).unwrap();
        assert_eq!(entries[0].0, "chair.json");
        assert_eq!(entries[1].1, vec![1, 2, 3]);

        let dir = tempfile::tempdir().unwrap();
        let bundle = NitroBundle::with_extract_dir(&buffer, dir.path().to_path_buf());
        assert_eq!(bundle.json_file().unwrap()["name"], "chair");
        assert_eq!(bundle.texture_extension(), ".png");
        assert!(dir.path().join("chair").join("chair.png").is_file());
    }
}
