//! `NitroFileExtractor`: unpacks one .nitro / .hab into `<name>.json` (indented) and
//! `<name>.<png|webp>` in a folder.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use base64::Engine;
use habbo_nitro::bundle::NitroBundle;

/// `NitroFileExtractor.ExtractNitroFile`.
pub async fn extract_nitro_file(nitro_file_path: &Path, output_dir: &Path) -> Result<()> {
    extract_into(nitro_file_path, output_dir, None).await
}

/// `bundle_extract_dir`: where NitroBundle saves every entry (`None` = NitroCompiler/extracted/furni).
pub(crate) async fn extract_into(nitro_file_path: &Path, output_dir: &Path, bundle_extract_dir: Option<PathBuf>) -> Result<()> {
    if !nitro_file_path.is_file() {
        bail!("Nitro file not found: {}", nitro_file_path.display());
    }

    let data = tokio::fs::read(nitro_file_path).await?;

    // NOTE(port): the C# NitroBundle let a damaged .hab (or a .hab json that is not JSON) throw
    // out of here; habbo_nitro's NitroBundle prints and carries on, so check that first.
    if habbo_nitro::hab::is_hab(&data) {
        let (_, files) = habbo_nitro::hab::read(&data)?;
        for (name, content) in &files {
            if name.to_lowercase().ends_with(".json") {
                serde_json::from_slice::<serde_json::Value>(content)?;
            }
        }
    }

    let bundle = match bundle_extract_dir {
        Some(dir) => NitroBundle::with_extract_dir(&data, dir),
        None => NitroBundle::new(&data),
    };

    std::fs::create_dir_all(output_dir)?;

    let name = habbo_core::paths::file_stem(nitro_file_path);
    let json_path = output_dir.join(format!("{name}.json"));
    let mut texture_ext = bundle.texture_extension().to_string();
    if !texture_ext.starts_with('.') {
        texture_ext.insert(0, '.');
    }
    let texture_path = output_dir.join(format!("{name}{texture_ext}"));

    if let Some(json) = bundle.json_file() {
        tokio::fs::write(&json_path, crate::dotnet::stj_indented(json)).await?;
    }

    if let Some(texture) = bundle.base_texture().filter(|t| !t.is_empty()) {
        let texture_data = base64::engine::general_purpose::STANDARD.decode(texture)?;
        tokio::fs::write(&texture_path, texture_data).await?;
    }
    Ok(())
}
