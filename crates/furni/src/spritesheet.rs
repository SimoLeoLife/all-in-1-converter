//! `Mapper/Spritesheet`: `SpriteSheetData` (the json `spritesheet` block),
//! `SpriteSheetMapper` (row packer with 2px padding and 1px edge extrusion) and
//! `ImageRestorer` (moves the tmp/ PNGs back under their asset names).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use habbo_core::outln;
use image::{Rgba, RgbaImage};
use indexmap::IndexMap;

use crate::assets::IgnoreCaseMap;
use crate::net;
use crate::stj::{FloatStyle, Obj, J};

const FRAME_PADDING: i32 = 2;
const EDGE_EXTRUDE: i32 = 1;

/// `SpriteSheetMapper.CleanAssetName`: `^([^_]+)_\1_` → `$1_` (a doubled first prefix).
pub fn clean_asset_name(name: &str, disable_clean_key: bool) -> String {
    if disable_clean_key {
        return name.to_string();
    }
    let Some(first) = name.find('_') else { return name.to_string() };
    if first == 0 {
        return name.to_string();
    }
    let prefix = &name[..first];
    let rest = &name[first + 1..];
    if rest.len() > prefix.len() && rest.starts_with(prefix) && rest.as_bytes()[prefix.len()] == b'_' {
        return format!("{prefix}_{}", &rest[prefix.len() + 1..]);
    }
    name.to_string()
}

/// `ForceCFUpper`: "cf_" / "cfc_" at the start or after an underscore become upper case.
pub fn force_cf_upper(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());
    let matches_at = |i: usize, pat: &str| -> bool {
        let pat: Vec<char> = pat.chars().collect();
        i + pat.len() <= chars.len() && pat.iter().enumerate().all(|(k, p)| chars[i + k].eq_ignore_ascii_case(p))
    };
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || chars[i - 1] == '_';
        if boundary {
            let len = if matches_at(i, "cfc_") {
                4
            } else if matches_at(i, "cf_") {
                3
            } else {
                0
            };
            if len > 0 {
                for c in &chars[i..i + len] {
                    out.push(c.to_ascii_uppercase());
                }
                i += len;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectData {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeData {
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameData {
    pub frame: RectData,
    pub rotated: bool,
    pub trimmed: bool,
    pub sprite_source_size: RectData,
    pub source_size: SizeData,
    pub pivot: (f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaData {
    pub image: String,
    pub format: String,
    pub size: SizeData,
    pub scale: f32,
    pub converter: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteSheetData {
    pub frames: IndexMap<String, FrameData>,
    pub meta: MetaData,
}

fn rect_json(r: &RectData) -> J {
    Obj::new()
        .put("x", J::int(r.x as i64))
        .put("y", J::int(r.y as i64))
        .put("w", J::int(r.w as i64))
        .put("h", J::int(r.h as i64))
        .build()
}

fn size_json(s: &SizeData) -> J {
    Obj::new().put("w", J::int(s.w as i64)).put("h", J::int(s.h as i64)).build()
}

impl SpriteSheetData {
    pub fn to_json(&self, style: FloatStyle) -> J {
        let frames = self
            .frames
            .iter()
            .map(|(key, f)| {
                let frame = Obj::new()
                    .put("frame", rect_json(&f.frame))
                    .put("rotated", J::Bool(f.rotated))
                    .put("trimmed", J::Bool(f.trimmed))
                    .put("spriteSourceSize", rect_json(&f.sprite_source_size))
                    .put("sourceSize", size_json(&f.source_size))
                    .put("pivot", Obj::new().put("x", J::f32_with(f.pivot.0, style)).put("y", J::f32_with(f.pivot.1, style)).build())
                    .build();
                (key.clone(), frame)
            })
            .collect();
        Obj::new()
            .put("frames", J::Obj(frames))
            .put(
                "meta",
                Obj::new()
                    .put("image", J::str(self.meta.image.clone()))
                    .put("format", J::str(self.meta.format.clone()))
                    .put("size", size_json(&self.meta.size))
                    .put("scale", J::f32_with(self.meta.scale, style))
                    .put("converter", J::str(self.meta.converter.clone()))
                    .build(),
            )
            .build()
    }
}

fn extrude_edges(sheet: &mut RgbaImage, sprite: &RgbaImage, draw_x: i32, draw_y: i32) {
    let w = sprite.width() as i32;
    let h = sprite.height() as i32;
    let mut put = |x: i32, y: i32, p: Rgba<u8>| sheet.put_pixel(x as u32, y as u32, p);
    let get = |x: i32, y: i32| *sprite.get_pixel(x as u32, y as u32);

    for x in 0..w {
        put(draw_x + x, draw_y - EDGE_EXTRUDE, get(x, 0));
        put(draw_x + x, draw_y + h, get(x, h - 1));
    }
    for y in 0..h {
        put(draw_x - EDGE_EXTRUDE, draw_y + y, get(0, y));
        put(draw_x + w, draw_y + y, get(w - 1, y));
    }
    put(draw_x - EDGE_EXTRUDE, draw_y - EDGE_EXTRUDE, get(0, 0));
    put(draw_x + w, draw_y - EDGE_EXTRUDE, get(w - 1, 0));
    put(draw_x - EDGE_EXTRUDE, draw_y + h, get(0, h - 1));
    put(draw_x + w, draw_y + h, get(w - 1, h - 1));
}

/// ImageSharp `DrawImage(image, point, 1f)` onto the (still transparent) sheet: normal
/// source-over blending, which on a transparent backdrop keeps every visible pixel and turns
/// fully transparent ones into (0,0,0,0).
fn draw_image(sheet: &mut RgbaImage, sprite: &RgbaImage, draw_x: i32, draw_y: i32) {
    for (x, y, pixel) in sprite.enumerate_pixels() {
        let tx = draw_x + x as i32;
        let ty = draw_y + y as i32;
        if tx < 0 || ty < 0 || tx as u32 >= sheet.width() || ty as u32 >= sheet.height() {
            continue;
        }
        let backdrop = *sheet.get_pixel(tx as u32, ty as u32);
        let result = if backdrop.0[3] == 0 {
            if pixel.0[3] == 0 { Rgba([0, 0, 0, 0]) } else { *pixel }
        } else {
            src_over(backdrop, *pixel)
        };
        sheet.put_pixel(tx as u32, ty as u32, result);
    }
}

/// ImageSharp's `PorterDuffFunctions.NormalSrcOver` in float, for a non-transparent backdrop
/// (never hit by the packer, whose sprites never overlap; kept for completeness).
fn src_over(backdrop: Rgba<u8>, source: Rgba<u8>) -> Rgba<u8> {
    let b: [f32; 4] = backdrop.0.map(|v| v as f32 / 255.0);
    let s: [f32; 4] = source.0.map(|v| v as f32 / 255.0);
    let blend_w = s[3] * b[3];
    let dst_w = b[3] - blend_w;
    let src_w = s[3] - blend_w;
    let alpha = dst_w + s[3];
    let mut out = [0u8; 4];
    for c in 0..3 {
        let color = (b[c] * dst_w + s[c] * src_w + s[c] * blend_w) / alpha.max(f32::EPSILON);
        out[c] = (color.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    out[3] = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Rgba(out)
}

/// The packed sheet before it is written: the image, its json block and its extension.
pub struct PackedSheet {
    pub image: RgbaImage,
    pub data: SpriteSheetData,
    pub extension: &'static str,
}

/// The layout and pixels of `SpriteSheetMapper.GenerateSpriteSheet`, without writing the file.
pub fn pack_sprite_sheet(
    images: &IndexMap<String, RgbaImage>,
    name: &str,
    canonical_mapping: &IgnoreCaseMap,
    _disable_clean_key: bool,
    num_rows: i32,
    max_width: i32,
    max_height: i32,
) -> Result<Option<PackedSheet>> {
    if images.is_empty() {
        outln!("⚠️ No images provided to generate sprite sheet.");
        return Ok(None);
    }

    let images_count = images.len() as i32;
    let images_per_row = ((images_count as f64) / (num_rows as f64)).ceil() as i32;
    let images_per_row = images_per_row.max(1) as usize;

    let values: Vec<(&String, &RgbaImage)> = images.iter().collect();
    let groups: Vec<&[(&String, &RgbaImage)]> = values.chunks(images_per_row).collect();

    let mut max_row_width = 0;
    let mut stacked_row_height = 0;
    for group in &groups {
        let row_width = FRAME_PADDING + group.iter().map(|(_, img)| img.width() as i32 + FRAME_PADDING).sum::<i32>();
        let row_height = group.iter().map(|(_, img)| img.height() as i32).max().unwrap_or(0);
        max_row_width = max_row_width.max(row_width);
        stacked_row_height += row_height + FRAME_PADDING;
    }

    let total_width = max_row_width;
    let total_height = FRAME_PADDING + stacked_row_height;
    if total_width > max_width || total_height > max_height {
        bail!(
            "Sprite sheet dimensions ({total_width}x{total_height}) exceed maximum allowed ({max_width}x{max_height}). Reduce the number of images or adjust the maximum dimensions."
        );
    }

    let mut sheet = RgbaImage::new(total_width as u32, total_height as u32);
    let extension = habbo_core::settings::resolve_sheet_extension(total_width as u32, total_height as u32, name);
    let mut data = SpriteSheetData {
        frames: IndexMap::new(),
        meta: MetaData {
            image: format!("{name}{extension}"),
            format: "RGBA8888".into(),
            size: SizeData { w: total_width, h: total_height },
            scale: 1.0,
            converter: "All-in-1-download-tool".into(),
        },
    };

    let mut current_y = FRAME_PADDING;
    for group in &groups {
        let mut current_x = FRAME_PADDING;
        let mut row_height = 0;
        for (key, image) in group.iter() {
            // NOTE(port): the C# passes `disableCleanKey: false` here whatever the caller asked.
            let short_key = clean_asset_name(key, false);
            let final_key = canonical_mapping.get(&short_key).map(str::to_string).unwrap_or(short_key);
            let final_key = force_cf_upper(&final_key);

            let (w, h) = (image.width() as i32, image.height() as i32);
            draw_image(&mut sheet, image, current_x, current_y);
            extrude_edges(&mut sheet, image, current_x, current_y);

            data.frames.insert(
                final_key,
                FrameData {
                    frame: RectData { x: current_x, y: current_y, w, h },
                    rotated: false,
                    trimmed: false,
                    sprite_source_size: RectData { x: 0, y: 0, w, h },
                    source_size: SizeData { w, h },
                    pivot: (0.5, 0.5),
                },
            );

            current_x += w + FRAME_PADDING;
            row_height = row_height.max(h);
        }
        current_y += row_height + FRAME_PADDING;
    }

    Ok(Some(PackedSheet { image: sheet, data, extension }))
}

/// `SpriteSheetMapper.GenerateSpriteSheet`: packs the images (in insertion order, rows of
/// `ceil(count / numRows)`), writes `{name}.webp` (or `.png`) into `output_directory` and
/// returns its path and json block. `Ok(None)` when there are no images.
#[allow(clippy::too_many_arguments)]
pub fn generate_sprite_sheet(
    images: &IndexMap<String, RgbaImage>,
    output_directory: &Path,
    name: &str,
    canonical_mapping: &IgnoreCaseMap,
    disable_clean_key: bool,
    num_rows: i32,
    max_width: i32,
    max_height: i32,
) -> Result<Option<(PathBuf, SpriteSheetData)>> {
    let Some(packed) = pack_sprite_sheet(images, name, canonical_mapping, disable_clean_key, num_rows, max_width, max_height)? else {
        return Ok(None);
    };
    let image_path = output_directory.join(format!("{name}{}", packed.extension));
    let bytes = habbo_core::imaging::encode_for_extension(&packed.image, packed.extension)?;
    std::fs::write(&image_path, bytes)?;
    Ok(Some((image_path, packed.data)))
}

/// `ImageRestorer.RestoreImagesFromTmpAsync`: every tmp/ PNG whose id (text before the first
/// '_') is in the mapping moves to `images/{name}.png`, replacing what is there.
pub fn restore_images_from_tmp(tmp_directory: &Path, images_directory: &Path, image_mapping: &HashMap<String, String>) {
    if !tmp_directory.is_dir() {
        outln!("❌ TMP directory not found: {}", tmp_directory.display());
        return;
    }
    if image_mapping.is_empty() {
        outln!("❌ In-memory image mapping is empty.");
        return;
    }
    let _ = std::fs::create_dir_all(images_directory);

    let tmp_files = net::get_files_recursive(tmp_directory, "*.png").unwrap_or_default();
    for tmp_file in tmp_files {
        let file_name = habbo_core::paths::file_stem(&tmp_file);
        let extension = net::extension_with_dot(&tmp_file);
        let id = file_name.split('_').next().unwrap_or("");
        let Some(correct_name) = image_mapping.get(id) else { continue };
        let new_file_path = images_directory.join(format!("{correct_name}{extension}"));
        if let Err(error) = move_file_overwrite(&tmp_file, &new_file_path) {
            outln!("❌ Error moving {file_name} to {}: {error}", new_file_path.display());
        }
    }
}

/// `File.Move(source, target, overwrite: true)`.
pub(crate) fn move_file_overwrite(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "the target is a directory"));
    }
    std::fs::rename(source, target)
}

/// `File.Move(source, target)` without overwrite (fails when the target exists).
pub(crate) fn move_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("Cannot create a file when that file already exists. : '{}'", target.display()),
        ));
    }
    std::fs::rename(source, target)
}

/// The converters' `LoadImages`: every top-level `*.png` in `images_directory` except
/// `sh_*` (and names `skip` rejects), decoded to RGBA, in directory order.
pub fn load_images_filtered(images_directory: &Path, skip: impl Fn(&str) -> bool) -> Result<IndexMap<String, RgbaImage>> {
    let mut images = IndexMap::new();
    for image_file in net::get_files(images_directory, "*.png")? {
        let image_name = habbo_core::paths::file_stem(&image_file);
        if image_name.starts_with("sh_") || skip(&image_name) {
            continue;
        }
        if images.contains_key(&image_name) {
            continue;
        }
        match std::fs::read(&image_file).map_err(anyhow::Error::from).and_then(|b| habbo_core::imaging::decode_rgba(&b)) {
            Ok(image) => {
                images.insert(image_name, image);
            }
            Err(error) => outln!("⚠️ Error loading image {}: {error}", image_file.display()),
        }
    }
    Ok(images)
}

/// The furniture `LoadImages` (also what the benchmark uses).
pub fn load_images(images_directory: &Path) -> Result<IndexMap<String, RgbaImage>> {
    load_images_filtered(images_directory, |_| false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(color))
    }

    #[test]
    fn names() {
        assert_eq!(clean_asset_name("chair_chair_64_a", false), "chair_64_a");
        assert_eq!(clean_asset_name("chair_chairs_64", false), "chair_chairs_64");
        assert_eq!(clean_asset_name("chair_chair", false), "chair_chair");
        assert_eq!(clean_asset_name("chair_chair_", false), "chair_");
        assert_eq!(clean_asset_name("_x_x_", false), "_x_x_");
        assert_eq!(force_cf_upper("cf_10_coin"), "CF_10_coin");
        assert_eq!(force_cf_upper("x_cfc_200_cf_y"), "x_CFC_200_CF_y");
        assert_eq!(force_cf_upper("xcf_y"), "xcf_y");
        assert_eq!(force_cf_upper("cf_cf_a"), "CF_CF_a");
    }

    #[test]
    fn packs_rows_with_padding_and_extrusion() {
        let mut images = IndexMap::new();
        // 12 images -> ceil(12 / 10) = 2 per row, 6 rows.
        for i in 0..12u32 {
            let mut img = solid(1 + i % 3, 1 + i % 2, [10 * i as u8, 20, 30, 255]);
            if i == 0 {
                img = RgbaImage::new(2, 2);
                img.put_pixel(0, 0, Rgba([255, 255, 255, 0])); // invisible but with colour
                img.put_pixel(1, 1, Rgba([1, 2, 3, 128]));
            }
            images.insert(format!("furni_furni_64_a_{i}"), img);
        }
        let mut canonical = IgnoreCaseMap::new();
        canonical.set("furni_64_a_1", "furni_furni_64_a_1".into());
        habbo_core::settings::set_spritesheet_format("png");
        let packed = pack_sprite_sheet(&images, "furni", &canonical, false, 10, 14000, 14000).unwrap().unwrap();

        // Row widths: 2 + (w+2) per sprite; heights: max per row + 2.
        let widths: Vec<i32> = (0..6)
            .map(|r| {
                2 + (0..2)
                    .map(|k| images[(r * 2 + k) as usize].width() as i32 + 2)
                    .sum::<i32>()
            })
            .collect();
        let heights: i32 = (0..6)
            .map(|r| (0..2).map(|k| images[(r * 2 + k) as usize].height() as i32).max().unwrap() + 2)
            .sum();
        assert_eq!(packed.data.meta.size, SizeData { w: *widths.iter().max().unwrap(), h: 2 + heights });
        assert_eq!(packed.data.meta.image, "furni.png");

        let keys: Vec<&String> = packed.data.frames.keys().collect();
        assert_eq!(keys[0], "furni_64_a_0");
        assert_eq!(keys[1], "furni_furni_64_a_1");
        let f0 = packed.data.frames["furni_64_a_0"];
        assert_eq!(f0.frame, RectData { x: 2, y: 2, w: 2, h: 2 });
        let f1 = packed.data.frames["furni_furni_64_a_1"];
        assert_eq!(f1.frame, RectData { x: 6, y: 2, w: 2, h: 2 });
        let f2 = packed.data.frames["furni_64_a_2"];
        assert_eq!(f2.frame, RectData { x: 2, y: 6, w: 3, h: 1 });

        let sheet = &packed.image;
        // Transparent source pixel drawn as (0,0,0,0); semi-transparent kept as is.
        assert_eq!(*sheet.get_pixel(2, 2), Rgba([0, 0, 0, 0]));
        assert_eq!(*sheet.get_pixel(3, 3), Rgba([1, 2, 3, 128]));
        // Extrusion copies the raw sprite pixels, colour of invisible pixels included.
        assert_eq!(*sheet.get_pixel(1, 1), Rgba([255, 255, 255, 0]));
        assert_eq!(*sheet.get_pixel(2, 1), Rgba([255, 255, 255, 0]));
        assert_eq!(*sheet.get_pixel(4, 4), Rgba([1, 2, 3, 128]));
        // Padding between sprites stays transparent.
        assert_eq!(*sheet.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(*sheet.get_pixel(6, 2), Rgba([10, 20, 30, 255]));
        assert_eq!(*sheet.get_pixel(5, 2), Rgba([10, 20, 30, 255]));

        let json = packed.data.to_json(FloatStyle::Fixed).to_string_indented().unwrap();
        assert!(json.contains("\"pivot\": {"));
        assert!(json.contains("\"x\": 0.5"));
        assert!(json.contains("\"scale\": 1,"));
        assert!(json.contains("\"converter\": \"All-in-1-download-tool\""));
    }

    #[test]
    fn rejects_oversized_sheets_and_empty_input() {
        let mut images = IndexMap::new();
        images.insert("a".to_string(), solid(20, 20, [1, 1, 1, 255]));
        let err = pack_sprite_sheet(&images, "x", &IgnoreCaseMap::new(), false, 10, 10, 10).err().unwrap();
        assert!(err.to_string().starts_with("Sprite sheet dimensions (24x24) exceed maximum allowed (10x10)."));
        assert!(pack_sprite_sheet(&IndexMap::new(), "x", &IgnoreCaseMap::new(), false, 10, 10, 10).unwrap().is_none());
    }

    #[test]
    fn writes_the_sheet_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut images = IndexMap::new();
        images.insert("a".to_string(), solid(3, 2, [9, 8, 7, 255]));
        let (path, data) = generate_sprite_sheet(&images, dir.path(), "x", &IgnoreCaseMap::new(), false, 10, 100, 100)
            .unwrap()
            .unwrap();
        let decoded = habbo_core::imaging::decode_rgba(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (7, 6));
        assert_eq!(data.meta.size, SizeData { w: 7, h: 6 });
        assert_eq!(*decoded.get_pixel(1, 1), Rgba([9, 8, 7, 255]));
    }
}
