//! `SpritesheetClothesMapper` / `EffectsSpritesheetMapper` (the same packer): images in
//! `ceil(n / 10)`-wide rows, 2 px padding, every sprite's edge extruded by 1 px, plus the
//! `SpriteSheetData` json (`frames` + `meta`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use habbo_core::{imaging, outln, settings};
use image::{Rgba, RgbaImage};

use crate::stj::{Node, ObjectBuilder};

const FRAME_PADDING: u32 = 2;
const EDGE_EXTRUDE: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectData {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// `FrameData`; rotated/trimmed are always false and the pivot 0.5/0.5 in these compilers.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameData {
    pub frame: RectData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteSheetData {
    pub frames: indexmap::IndexMap<String, FrameData>,
    pub image: String,
    pub width: u32,
    pub height: u32,
}

fn rect_node(x: u32, y: u32, w: u32, h: u32) -> Node {
    ObjectBuilder::new()
        .put("x", Node::int(x as i64))
        .put("y", Node::int(y as i64))
        .put("w", Node::int(w as i64))
        .put("h", Node::int(h as i64))
        .build()
}

fn size_node(w: u32, h: u32) -> Node {
    ObjectBuilder::new().put("w", Node::int(w as i64)).put("h", Node::int(h as i64)).build()
}

impl SpriteSheetData {
    /// The System.Text.Json form of the C# `SpriteSheetData` (floats 1.0f / 0.5f print as
    /// `1` / `0.5`).
    pub fn to_node(&self) -> Node {
        let frames = self
            .frames
            .iter()
            .map(|(name, f)| {
                let r = f.frame;
                let node = ObjectBuilder::new()
                    .put("frame", rect_node(r.x, r.y, r.w, r.h))
                    .put("rotated", Node::bool(false))
                    .put("trimmed", Node::bool(false))
                    .put("spriteSourceSize", rect_node(0, 0, r.w, r.h))
                    .put("sourceSize", size_node(r.w, r.h))
                    .put(
                        "pivot",
                        ObjectBuilder::new().put("x", Node::Raw("0.5".into())).put("y", Node::Raw("0.5".into())).build(),
                    )
                    .build();
                (name.clone(), node)
            })
            .collect();
        let meta = ObjectBuilder::new()
            .put("image", Node::str(self.image.clone()))
            .put("format", Node::str("RGBA8888"))
            .put("size", size_node(self.width, self.height))
            .put("scale", Node::Raw("1".into()))
            .put("converter", Node::str("All-in-1-dowload-tool"))
            .build();
        ObjectBuilder::new().put("frames", Node::Object(frames)).put("meta", meta).build()
    }
}

/// The packed sheet before it is encoded.
pub struct PackedSheet {
    pub image: RgbaImage,
    pub data: SpriteSheetData,
    pub extension: &'static str,
}

/// The layout and pixels of `GenerateSpriteSheet` (numRows = 10). `Ok(None)` for no images
/// (after printing the C# warning); `Err` when the sheet would pass the maximum size.
pub fn pack(images: &[(String, RgbaImage)], name: &str, max_width: u32, max_height: u32) -> Result<Option<PackedSheet>> {
    const NUM_ROWS: usize = 10;
    if images.is_empty() {
        outln!("⚠️ No images provided to generate sprite sheet.");
        return Ok(None);
    }

    let images_per_row = images.len().div_ceil(NUM_ROWS);
    let rows: Vec<&[(String, RgbaImage)]> = images.chunks(images_per_row).collect();

    let mut max_row_width = 0u64;
    let mut stacked_row_height = 0u64;
    for row in &rows {
        let row_width = FRAME_PADDING as u64 + row.iter().map(|(_, i)| i.width() as u64 + FRAME_PADDING as u64).sum::<u64>();
        let row_height = row.iter().map(|(_, i)| i.height() as u64).max().unwrap_or(0);
        max_row_width = max_row_width.max(row_width);
        stacked_row_height += row_height + FRAME_PADDING as u64;
    }
    let total_width = max_row_width;
    let total_height = FRAME_PADDING as u64 + stacked_row_height;

    if total_width > max_width as u64 || total_height > max_height as u64 {
        bail!(
            "Sprite sheet dimensions ({total_width}x{total_height}) exceed maximum allowed ({max_width}x{max_height}). \
             Reduce the number of images or adjust the maximum dimensions."
        );
    }
    let (total_width, total_height) = (total_width as u32, total_height as u32);

    let mut sheet = RgbaImage::new(total_width, total_height);
    let extension = settings::resolve_sheet_extension(total_width, total_height, name);
    let mut data = SpriteSheetData {
        frames: indexmap::IndexMap::new(),
        image: format!("{name}{extension}"),
        width: total_width,
        height: total_height,
    };

    let mut current_y = FRAME_PADDING;
    for row in rows {
        let mut current_x = FRAME_PADDING;
        let mut row_height = 0;
        for (key, image) in row {
            draw_over(&mut sheet, image, current_x, current_y);
            extrude_edges(&mut sheet, image, current_x, current_y);
            data.frames.insert(
                key.clone(),
                FrameData { frame: RectData { x: current_x, y: current_y, w: image.width(), h: image.height() } },
            );
            current_x += image.width() + FRAME_PADDING;
            row_height = row_height.max(image.height());
        }
        current_y += row_height + FRAME_PADDING;
    }

    Ok(Some(PackedSheet { image: sheet, data, extension }))
}

/// `GenerateSpriteSheet`: packs and writes `<outputDirectory>/<name><ext>`.
pub fn generate_sprite_sheet(
    images: &[(String, RgbaImage)],
    output_directory: &Path,
    name: &str,
    max_width: u32,
    max_height: u32,
) -> Result<Option<(PathBuf, SpriteSheetData)>> {
    let Some(packed) = pack(images, name, max_width, max_height)? else { return Ok(None) };
    let image_path = output_directory.join(format!("{name}{}", packed.extension));
    let bytes = imaging::encode_for_extension(&packed.image, packed.extension)?;
    std::fs::write(&image_path, bytes)?;
    Ok(Some((image_path, packed.data)))
}

/// ImageSharp `DrawImage(image, point, 1f)` onto the (transparent) sheet: Normal / SrcOver.
/// Every target pixel is still transparent here, so the result is the sprite pixel, except
/// a fully transparent sprite pixel, which becomes (0,0,0,0).
fn draw_over(sheet: &mut RgbaImage, sprite: &RgbaImage, draw_x: u32, draw_y: u32) {
    for (x, y, pixel) in sprite.enumerate_pixels() {
        let target = sheet.get_pixel_mut(draw_x + x, draw_y + y);
        *target = src_over(*target, *pixel);
    }
}

fn src_over(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    let da = dst[3] as f32 / 255.0;
    if da == 0.0 {
        return if src[3] == 0 { Rgba([0, 0, 0, 0]) } else { src };
    }
    if sa == 1.0 {
        return src;
    }
    // General Porter-Duff SrcOver (premultiplied, then un-premultiplied), as ImageSharp does.
    let alpha = sa + da * (1.0 - sa);
    let channel = |s: u8, d: u8| {
        let c = (s as f32 / 255.0 * sa + d as f32 / 255.0 * da * (1.0 - sa)) / alpha.max(f32::EPSILON);
        (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8
    };
    Rgba([
        channel(src[0], dst[0]),
        channel(src[1], dst[1]),
        channel(src[2], dst[2]),
        (alpha * 255.0 + 0.5).clamp(0.0, 255.0) as u8,
    ])
}

fn extrude_edges(sheet: &mut RgbaImage, sprite: &RgbaImage, draw_x: u32, draw_y: u32) {
    let (w, h) = sprite.dimensions();
    let e = EDGE_EXTRUDE;
    for x in 0..w {
        sheet.put_pixel(draw_x + x, draw_y - e, *sprite.get_pixel(x, 0));
        sheet.put_pixel(draw_x + x, draw_y + h, *sprite.get_pixel(x, h - 1));
    }
    for y in 0..h {
        sheet.put_pixel(draw_x - e, draw_y + y, *sprite.get_pixel(0, y));
        sheet.put_pixel(draw_x + w, draw_y + y, *sprite.get_pixel(w - 1, y));
    }
    sheet.put_pixel(draw_x - e, draw_y - e, *sprite.get_pixel(0, 0));
    sheet.put_pixel(draw_x + w, draw_y - e, *sprite.get_pixel(w - 1, 0));
    sheet.put_pixel(draw_x - e, draw_y + h, *sprite.get_pixel(0, h - 1));
    sheet.put_pixel(draw_x + w, draw_y + h, *sprite.get_pixel(w - 1, h - 1));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    #[test]
    fn layout_rows_and_frames() {
        settings::set_spritesheet_format("png");
        // 12 images -> 2 per row, 6 rows.
        let images: Vec<(String, RgbaImage)> =
            (0..12).map(|i| (format!("img{i}"), solid(i + 1, 2 * i + 1, [i as u8, 0, 0, 255]))).collect();
        let packed = pack(&images, "lib", 14000, 14000).unwrap().unwrap();

        // Widest row: images 10 and 11 (11 + 12 px wide): 2 + 13 + 14 = 29.
        assert_eq!(packed.data.width, 29);
        // Row heights: max of pairs = 3, 7, 11, 15, 19, 23 -> 78, + 6 * 2 + 2 = 92.
        assert_eq!(packed.data.height, 92);
        assert_eq!(packed.image.dimensions(), (29, 92));
        assert_eq!(packed.data.image, "lib.png");

        let f = |n: &str| packed.data.frames[n].frame;
        assert_eq!(f("img0"), RectData { x: 2, y: 2, w: 1, h: 1 });
        assert_eq!(f("img1"), RectData { x: 5, y: 2, w: 2, h: 3 });
        assert_eq!(f("img2"), RectData { x: 2, y: 7, w: 3, h: 5 });
        assert_eq!(packed.data.frames.keys().next().unwrap(), "img0");
    }

    #[test]
    fn pixels_are_drawn_and_extruded() {
        settings::set_spritesheet_format("png");
        let mut sprite = RgbaImage::new(2, 2);
        sprite.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        sprite.put_pixel(1, 0, Rgba([200, 100, 50, 0])); // transparent with colour
        sprite.put_pixel(0, 1, Rgba([1, 2, 3, 128]));
        sprite.put_pixel(1, 1, Rgba([4, 5, 6, 255]));
        let packed = pack(&[("a".into(), sprite.clone())], "x", 100, 100).unwrap().unwrap();
        let img = &packed.image;
        assert_eq!(img.dimensions(), (6, 6));
        // Drawn: transparent source becomes (0,0,0,0); the others are copied.
        assert_eq!(*img.get_pixel(2, 2), Rgba([10, 20, 30, 255]));
        assert_eq!(*img.get_pixel(3, 2), Rgba([0, 0, 0, 0]));
        assert_eq!(*img.get_pixel(2, 3), Rgba([1, 2, 3, 128]));
        // Extruded edges copy the raw sprite pixels (including the transparent colour).
        assert_eq!(*img.get_pixel(3, 1), Rgba([200, 100, 50, 0]));
        assert_eq!(*img.get_pixel(1, 1), Rgba([10, 20, 30, 255]));
        assert_eq!(*img.get_pixel(4, 4), Rgba([4, 5, 6, 255]));
        assert_eq!(*img.get_pixel(1, 3), Rgba([1, 2, 3, 128]));
        assert_eq!(*img.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(*img.get_pixel(5, 5), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn too_large_is_an_error_and_empty_is_none() {
        settings::set_spritesheet_format("png");
        assert!(pack(&[("a".into(), solid(20, 1, [0; 4]))], "x", 10, 100).is_err());
        assert!(pack(&[], "x", 10, 10).unwrap().is_none());
    }

    #[test]
    fn json_matches_system_text_json() {
        let mut frames = indexmap::IndexMap::new();
        frames.insert("a".to_string(), FrameData { frame: RectData { x: 2, y: 2, w: 3, h: 4 } });
        let data = SpriteSheetData { frames, image: "lib.webp".into(), width: 7, height: 8 };
        let json = crate::stj::to_string_indented(&data.to_node()).replace("\r\n", "\n");
        let expected = r#"{
  "frames": {
    "a": {
      "frame": {
        "x": 2,
        "y": 2,
        "w": 3,
        "h": 4
      },
      "rotated": false,
      "trimmed": false,
      "spriteSourceSize": {
        "x": 0,
        "y": 0,
        "w": 3,
        "h": 4
      },
      "sourceSize": {
        "w": 3,
        "h": 4
      },
      "pivot": {
        "x": 0.5,
        "y": 0.5
      }
    }
  },
  "meta": {
    "image": "lib.webp",
    "format": "RGBA8888",
    "size": {
      "w": 7,
      "h": 8
    },
    "scale": 1,
    "converter": "All-in-1-dowload-tool"
  }
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn writes_the_sheet_file() {
        settings::set_spritesheet_format("png");
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = generate_sprite_sheet(&[("a".into(), solid(3, 3, [9, 9, 9, 255]))], dir.path(), "s", 100, 100)
            .unwrap()
            .unwrap();
        assert_eq!(path, dir.path().join("s.png"));
        let decoded = imaging::decode_rgba(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(*decoded.get_pixel(2, 2), Rgba([9, 9, 9, 255]));
    }
}
