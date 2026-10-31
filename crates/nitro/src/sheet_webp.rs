//! PNG sheet -> lossless WebP. The WebP is decoded again and must match every pixel; when it
//! does not, or the sheet is too large for the WebP limit, the PNG is kept.

use anyhow::Result;
use habbo_core::console::n0;
use habbo_core::{imaging, settings};

use crate::{hab, FileEntry};

/// `Ok(webp)` when converted; `Err(reason)` when the PNG is kept.
pub fn try_convert(png: &[u8], name: &str) -> Result<std::result::Result<Vec<u8>, &'static str>> {
    let image = imaging::decode_rgba(png)?;

    if settings::resolve_sheet_extension(image.width(), image.height(), name) != ".webp" {
        return Ok(Err("kept PNG (too large for lossless WebP)"));
    }

    let encoded = imaging::encode_webp_lossless(&image)?;
    let check = imaging::decode_rgba(&encoded)?;
    if check.dimensions() != image.dimensions() || check.as_raw() != image.as_raw() {
        return Ok(Err("kept PNG (WebP would change pixels)"));
    }
    Ok(Ok(encoded))
}

/// A one-line description of a conversion, like the C# `result` string.
pub fn describe(png_len: usize, webp_len: usize) -> String {
    format!("PNG {} -> WebP {} bytes", n0(png_len as i64), n0(webp_len as i64))
}

/// A bundle entry with its PNG sheet as WebP (renamed to .webp); other entries unchanged.
pub fn to_webp_entry(file: FileEntry, name: &str) -> Result<FileEntry> {
    if !file.0.to_lowercase().ends_with(".png") {
        return Ok(file);
    }
    Ok(match try_convert(&file.1, name)? {
        Ok(webp) => (change_extension(&file.0, ".webp"), webp),
        Err(_) => file,
    })
}

/// `HabSheetConverter.ToWebp`: swaps the PNG sheet of a Habbo .hab for a lossless WebP.
/// The json is kept byte for byte; when the WebP would not be identical the .hab is kept.
/// Returns the bytes and a description of what happened.
pub fn hab_to_webp(hab_bytes: &[u8]) -> Result<(Vec<u8>, String)> {
    let (name, mut files) = hab::read(hab_bytes)?;

    let pngs: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, (file_name, _))| file_name.to_lowercase().ends_with(".png"))
        .map(|(index, _)| index)
        .collect();
    let has_json = files.iter().any(|(file_name, _)| file_name.to_lowercase().ends_with(".json"));

    if pngs.len() != 1 || !has_json {
        return Ok((hab_bytes.to_vec(), "kept as is (not one json + one png sheet)".into()));
    }

    let (png_name, png) = files[pngs[0]].clone();
    match try_convert(&png, &name)? {
        Err(reason) => Ok((hab_bytes.to_vec(), reason.to_string())),
        Ok(webp) => {
            let result = describe(png.len(), webp.len());
            files[pngs[0]] = (change_extension(&png_name, ".webp"), webp);
            Ok((hab::write(&name, &files)?, result))
        }
    }
}

/// `Path.ChangeExtension`.
pub fn change_extension(file_name: &str, extension: &str) -> String {
    let path = std::path::Path::new(file_name);
    match path.extension() {
        Some(_) => {
            let stem_len = file_name.len() - path.extension().unwrap().len() - 1;
            format!("{}{}", &file_name[..stem_len], extension)
        }
        None => format!("{file_name}{extension}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_sheet_becomes_identical_webp() {
        let mut image = image::RgbaImage::new(4, 3);
        image.put_pixel(1, 1, image::Rgba([10, 20, 30, 128]));
        let png = imaging::encode_png(&image).unwrap();
        let (name, webp) = to_webp_entry(("a.png".into(), png), "a").unwrap();
        assert_eq!(name, "a.webp");
        assert_eq!(imaging::decode_rgba(&webp).unwrap(), image);
        assert_eq!(change_extension("x.y.png", ".webp"), "x.y.webp");
    }
}
