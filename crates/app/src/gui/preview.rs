//! Bundle preview: the files inside a .nitro / .hab, its sheet and its json.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use eframe::egui;

pub struct BundlePreview {
    pub path: PathBuf,
    pub entries: Vec<(String, usize)>,
    pub json: String,
    pub sheet: Option<(egui::TextureHandle, [usize; 2], String)>,
}

impl BundlePreview {
    pub fn open(ctx: &egui::Context, path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;
        let entries = habbo_nitro::bundle::read_entries(&bytes).context("Not a readable .nitro / .hab bundle")?;

        let mut json = String::new();
        let mut sheet = None;
        for (name, data) in &entries {
            let lower = name.to_lowercase();
            if lower.ends_with(".json") && json.is_empty() {
                json = match serde_json::from_slice::<serde_json::Value>(data) {
                    Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_default(),
                    Err(_) => String::from_utf8_lossy(data).into_owned(),
                };
            } else if (lower.ends_with(".png") || lower.ends_with(".webp")) && sheet.is_none() {
                if let Ok(image) = habbo_core::imaging::decode_rgba(data) {
                    let size = [image.width() as usize, image.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                    let texture = ctx.load_texture(format!("preview:{}", path.display()), color, egui::TextureOptions::NEAREST);
                    sheet = Some((texture, size, name.clone()));
                }
            }
        }

        Ok(Self { path: path.to_path_buf(), entries: entries.iter().map(|(n, d)| (n.clone(), d.len())).collect(), json, sheet })
    }

    pub fn name(&self) -> String {
        habbo_core::paths::file_name(&self.path)
    }
}
