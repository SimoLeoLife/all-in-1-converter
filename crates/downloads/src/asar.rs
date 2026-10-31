//! A small reader for Electron .asar archives: u32 4, u32 header size, then the header pickle
//! (u32 payload size, u32 json length, json) and the file data after the header. All
//! integers are little-endian. File entries carry `offset` (a string) and `size`, relative to
//! the end of the header; `"unpacked": true` entries live in `<archive>.unpacked/`.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

/// The archive's header json and the header size (data starts at `8 + header_size`).
pub fn read_header(asar: &[u8]) -> Result<(Value, u64)> {
    if asar.len() < 16 {
        bail!("Not an .asar archive.");
    }
    let header_size = u64::from(u32_at(asar, 4));
    let json_length = u32_at(asar, 12) as usize;
    if 16 + json_length > asar.len() || 8 + header_size > asar.len() as u64 {
        bail!(".asar header is damaged.");
    }
    let header: Value = serde_json::from_slice(&asar[16..16 + json_length]).context(".asar header is not valid JSON")?;
    Ok((header, header_size))
}

/// The first file entry (depth first, header order) whose name matches case-insensitively,
/// and its path inside the archive ("a/b/name").
pub fn find_entry<'a>(directory: &'a Value, prefix: &str, name: &str) -> Option<(&'a Value, String)> {
    let children = directory.get("files")?.as_object()?;
    for (child_name, child) in children {
        let child_path = if prefix.is_empty() { child_name.clone() } else { format!("{prefix}/{child_name}") };
        if child.get("files").is_some() {
            if let Some(found) = find_entry(child, &child_path, name) {
                return Some(found);
            }
        } else if child_name.eq_ignore_ascii_case(name) {
            return Some((child, child_path));
        }
    }
    None
}

/// The bytes of the file named `name` (searched anywhere in the archive), `Ok(None)` when it
/// is not there or is unpacked and the unpacked folder does not hold it.
pub fn read_file(asar: &[u8], name: &str, unpacked_folder: Option<&Path>) -> Result<Option<Vec<u8>>> {
    let (header, header_size) = read_header(asar)?;
    let Some((node, entry_path)) = find_entry(&header, "", name) else { return Ok(None) };

    if node.get("unpacked").and_then(Value::as_bool) == Some(true) {
        let Some(folder) = unpacked_folder else { return Ok(None) };
        let path = folder.join(&entry_path);
        return Ok(if path.is_file() { Some(std::fs::read(path)?) } else { None });
    }

    let offset: i64 = match node.get("offset") {
        None => bail!("The given key was not present in the dictionary."),
        Some(Value::String(s)) => s.parse().with_context(|| format!("The input string '{s}' was not in a correct format."))?,
        Some(Value::Null) => 0,
        Some(_) => bail!("The requested operation requires an element of type 'String'."),
    };
    let size = node
        .get("size")
        .context("The given key was not present in the dictionary.")?
        .as_i64()
        .context("The requested operation requires an element of type 'Number'.")?;
    let start = 8 + header_size as i64 + offset;

    if offset < 0 || size < 0 || start + size > asar.len() as i64 {
        bail!(".asar entry lies outside the archive.");
    }
    Ok(Some(asar[start as usize..(start + size) as usize].to_vec()))
}

/// Builds an archive (for tests and tools): files in order, all packed.
pub fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut root = serde_json::Map::new();
    let mut data = Vec::new();
    for (path, bytes) in files {
        let parts: Vec<&str> = path.split('/').collect();
        let mut directory = &mut root;
        for part in &parts[..parts.len() - 1] {
            let entry = directory
                .entry(part.to_string())
                .or_insert_with(|| serde_json::json!({ "files": {} }));
            directory = entry["files"].as_object_mut().unwrap();
        }
        directory.insert(
            parts[parts.len() - 1].to_string(),
            serde_json::json!({ "size": bytes.len(), "offset": data.len().to_string() }),
        );
        data.extend_from_slice(bytes);
    }
    let json = serde_json::to_vec(&serde_json::json!({ "files": root })).unwrap();
    let padded = (json.len() + 3) & !3;
    let mut out = Vec::new();
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&((padded + 8) as u32).to_le_bytes());
    out.extend_from_slice(&((padded + 4) as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(16 + padded, 0);
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_files() {
        let asar = build(&[("package.json", b"{}"), ("client/generated/Habbo-Inventory-com.hab", b"HAB-data"), ("z.txt", b"z")]);
        let (header, size) = read_header(&asar).unwrap();
        assert_eq!(size as usize % 4, 0);
        let (_, path) = find_entry(&header, "", "habbo-inventory-com.hab").unwrap();
        assert_eq!(path, "client/generated/Habbo-Inventory-com.hab");
        assert_eq!(read_file(&asar, "habbo-inventory-com.hab", None).unwrap().unwrap(), b"HAB-data");
        assert_eq!(read_file(&asar, "z.txt", None).unwrap().unwrap(), b"z");
        assert_eq!(read_file(&asar, "missing", None).unwrap(), None);
    }

    #[test]
    fn unpacked_and_damaged() {
        let json = br#"{"files":{"a":{"files":{"b.hab":{"size":3,"unpacked":true}}}}}"#;
        let mut asar = Vec::new();
        asar.extend_from_slice(&4u32.to_le_bytes());
        asar.extend_from_slice(&((json.len() + 8) as u32).to_le_bytes());
        asar.extend_from_slice(&((json.len() + 4) as u32).to_le_bytes());
        asar.extend_from_slice(&(json.len() as u32).to_le_bytes());
        asar.extend_from_slice(json);

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_file(&asar, "b.hab", Some(dir.path())).unwrap(), None);
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/b.hab"), b"abc").unwrap();
        assert_eq!(read_file(&asar, "b.hab", Some(dir.path())).unwrap().unwrap(), b"abc");
        assert_eq!(read_file(&asar, "b.hab", None).unwrap(), None);

        assert!(read_header(&asar[..10]).is_err());
        let mut damaged = asar.clone();
        damaged[12] = 0xFF;
        assert!(read_header(&damaged).is_err());

        let outside = build(&[("x.hab", b"1234")]);
        let truncated = &outside[..outside.len() - 1];
        assert!(read_file(truncated, "x.hab", None).is_err());
    }
}
