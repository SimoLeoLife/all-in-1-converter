//! `JsonReadHelper` and the FurnitureData / ProductData / FigureData / FigureMap IO:
//! one strict JSON file in, one compact JSON file out. Key order is preserved
//! (serde_json `preserve_order`), like Newtonsoft's JObject.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

pub const FURNIDATA_FILE: &str = "FurnitureData.json";
pub const PRODUCTDATA_FILE: &str = "ProductData.json";
pub const FIGUREDATA_FILE: &str = "FigureData.json";
pub const FIGUREMAP_FILE: &str = "FigureMap.json";

pub type JObject = Map<String, Value>;

/// Loads one strict JSON file whose root is an object (no comments, no trailing commas).
pub fn load_object(path: impl AsRef<Path>) -> Result<JObject> {
    let path = path.as_ref();
    validate_file_path(path)?;
    let raw = std::fs::read_to_string(path)?;
    parse_object(&raw, path)
}

pub async fn load_object_async(path: impl AsRef<Path>) -> Result<JObject> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || load_object(path)).await?
}

/// Writes the object compactly (Newtonsoft `Formatting.None`), creating the folder.
pub fn save_object(data: &JObject, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    validate_destination(path)?;
    let full = std::path::absolute(path)?;
    if let Some(directory) = full.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(path, serde_json::to_string(data)?)?;
    Ok(())
}

pub async fn save_object_async(data: &JObject, path: impl AsRef<Path>) -> Result<()> {
    save_object(data, path)
}

fn parse_object(raw: &str, path: &Path) -> Result<JObject> {
    // A UTF-8 BOM is not JSON, but .NET's File.ReadAllText dropped it silently.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => bail!("Invalid strict JSON file: {} (the root is not an object)", path.display()),
        Err(error) => Err(error).with_context(|| format!("Invalid strict JSON file: {}", path.display())),
    }
}

fn validate_file_path(path: &Path) -> Result<()> {
    validate_destination(path)?;
    if path.is_dir() {
        bail!("Directory and split-layout inputs are not supported. Select one JSON file.");
    }
    if !path.is_file() {
        bail!("JSON file not found: {}", path.display());
    }
    Ok(())
}

fn validate_destination(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("A JSON file path is required.");
    }
    if !crate::paths::has_ext(path, "json") {
        bail!("Only single .json files are supported.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_roundtrip_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(&path, "\u{feff}{\"z\":1,\"a\":[1,2]}").unwrap();
        let object = load_object(&path).unwrap();
        save_object(&object, &path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"z\":1,\"a\":[1,2]}");

        std::fs::write(&path, "{\"a\":1,}").unwrap();
        assert!(load_object(&path).is_err());
        assert!(load_object(dir.path().join("a.txt")).is_err());
    }
}
