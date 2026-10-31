//! `NitroBundler`: writes the .nitro container. Big-endian i16 file count, then per file
//! i16 name length, UTF-8 name, i32 data length and the gzip-compressed data.

use std::io::Write;

use anyhow::{bail, Result};
use flate2::write::GzEncoder;
use flate2::Compression;

#[derive(Default)]
pub struct NitroBundler {
    files: Vec<(String, Vec<u8>)>,
}

impl NitroBundler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file; the same name again replaces the earlier data (keeping its position).
    pub fn add_file(&mut self, name: &str, data: Vec<u8>) -> Result<()> {
        if name.trim().is_empty() {
            bail!("File name cannot be null or empty.");
        }
        if data.is_empty() {
            bail!("File data cannot be null or empty.");
        }
        match self.files.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = data,
            None => self.files.push((name.to_string(), data)),
        }
        Ok(())
    }

    pub fn to_buffer(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.files.len() as i16).to_be_bytes());
        for (name, data) in &self.files {
            let name_bytes = name.as_bytes();
            out.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
            out.extend_from_slice(name_bytes);
            let compressed = compress(data)?;
            out.extend_from_slice(&(compressed.len() as i32).to_be_bytes());
            out.extend_from_slice(&compressed);
        }
        Ok(out)
    }
}

fn compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}
