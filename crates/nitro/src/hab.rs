//! Habbo's .hab bundle, the HTML5 client's replacement for .swf (and ours for .nitro):
//! "HAB\0", u16 version, u16 flags, u32 index length, u32 raw index length, u32 data length
//! (all little-endian), then a zlib JSON index `{ format: "hab", version, name, entries: [...] }`
//! and the data section. Entry offsets count from the start of the data section.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::FileEntry;

pub const HEADER_LENGTH: usize = 20;
const FORMAT_VERSION: u16 = 1;
/// Habbo's own furni bundles carry flags 1.
const FLAGS: u16 = 1;
const MAGIC: [u8; 4] = *b"HAB\0";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HabIndex {
    format: String,
    version: i32,
    #[serde(default)]
    name: Option<String>,
    entries: Option<Vec<HabIndexEntry>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HabIndexEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    mime_type: Option<String>,
    offset: i64,
    stored_length: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    original_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    compression: Option<String>,
}

pub fn is_hab(data: &[u8]) -> bool {
    data.len() >= HEADER_LENGTH && data[..4] == MAGIC
}

/// Packs the files into one .hab. Like Habbo's own bundles, an entry is zlib-compressed only
/// when that makes it smaller; an already compressed image is stored as is ("none").
pub fn write(name: &str, files: &[FileEntry]) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut entries = Vec::with_capacity(files.len());

    for (file_name, bytes) in files {
        let deflated = deflate(bytes)?;
        let compress = deflated.len() < bytes.len();
        let stored: &[u8] = if compress { &deflated } else { bytes };
        entries.push(HabIndexEntry {
            name: file_name.clone(),
            mime_type: Some(mime_type_of(file_name).to_string()),
            offset: data.len() as i64,
            stored_length: stored.len() as i64,
            original_length: Some(bytes.len() as i64),
            compression: Some(if compress { "deflate" } else { "none" }.to_string()),
        });
        data.extend_from_slice(stored);
    }

    let raw_index = serde_json::to_vec(&HabIndex {
        format: "hab".into(),
        version: FORMAT_VERSION as i32,
        name: Some(name.to_string()),
        entries: Some(entries),
    })?;
    let index = deflate(&raw_index)?;

    let mut output = Vec::with_capacity(HEADER_LENGTH + index.len() + data.len());
    output.extend_from_slice(&MAGIC);
    output.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    output.extend_from_slice(&FLAGS.to_le_bytes());
    output.extend_from_slice(&(index.len() as u32).to_le_bytes());
    output.extend_from_slice(&(raw_index.len() as u32).to_le_bytes());
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&index);
    output.extend_from_slice(&data);
    Ok(output)
}

/// Reads every entry; all lengths are checked, so a damaged file errors instead of giving half a bundle.
pub fn read(buffer: &[u8]) -> Result<(String, Vec<FileEntry>)> {
    if !is_hab(buffer) {
        bail!("Not a .hab bundle (missing HAB header).");
    }

    let u32_at = |offset: usize| u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize;
    let index_length = u32_at(8);
    let raw_index_length = u32_at(12);
    let data_length = u32_at(16);
    let data_start = HEADER_LENGTH + index_length;

    if data_start + data_length > buffer.len() {
        bail!(".hab bundle is shorter than its header says.");
    }

    let raw_index = inflate(&buffer[HEADER_LENGTH..data_start])?;
    if raw_index_length != 0 && raw_index.len() != raw_index_length {
        bail!(".hab index has the wrong length.");
    }

    let index: HabIndex = serde_json::from_slice(&raw_index).context(".hab index is not a \"hab\" index.")?;
    let Some(entries) = index.entries.filter(|_| index.format == "hab") else {
        bail!(".hab index is not a \"hab\" index.");
    };

    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.name.is_empty()
            || entry.offset < 0
            || entry.stored_length < 0
            || (entry.offset + entry.stored_length) as usize > data_length
        {
            bail!(".hab entry \"{}\" lies outside the data section.", entry.name);
        }
        let start = data_start + entry.offset as usize;
        let stored = &buffer[start..start + entry.stored_length as usize];
        let bytes = if entry.compression.as_deref() == Some("deflate") { inflate(stored)? } else { stored.to_vec() };

        if entry.original_length.is_some_and(|length| bytes.len() as i64 != length) {
            bail!(".hab entry \"{}\" has the wrong length.", entry.name);
        }
        files.push((entry.name, bytes));
    }

    Ok((index.name.unwrap_or_default(), files))
}

pub fn mime_type_of(file_name: &str) -> &'static str {
    let extension = std::path::Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "json" => "application/json",
        "png" => "image/png",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "xml" => "application/xml",
        _ => "text/plain",
    }
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

/// Habbo stores zlib streams; a raw deflate stream is accepted too.
fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    if ZlibDecoder::new(data).read_to_end(&mut output).is_ok() {
        return Ok(output);
    }
    output.clear();
    DeflateDecoder::new(data).read_to_end(&mut output).context("Invalid .hab compressed data.")?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stores_incompressible_entries_raw() {
        let json = br#"{"name":"chair","a":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]}"#.to_vec();
        let random: Vec<u8> = (0..64u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let files = vec![("chair.json".to_string(), json.clone()), ("chair.webp".to_string(), random.clone())];
        let bytes = write("chair", &files).unwrap();
        assert!(is_hab(&bytes));
        let (name, read_back) = read(&bytes).unwrap();
        assert_eq!(name, "chair");
        assert_eq!(read_back, files);

        let raw_index = inflate(&bytes[HEADER_LENGTH..HEADER_LENGTH + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize]).unwrap();
        let text = String::from_utf8(raw_index).unwrap();
        assert!(text.starts_with(r#"{"format":"hab","version":1,"name":"chair","entries":[{"name":"chair.json","mimeType":"application/json","offset":0,"#));
        assert!(text.contains(r#""compression":"none""#));
    }

    #[test]
    fn rejects_truncated() {
        let bytes = write("x", &[("x.json".into(), b"{}".to_vec())]).unwrap();
        assert!(read(&bytes[..bytes.len() - 1]).is_err());
        assert!(read(b"nope").is_err());
    }
}
