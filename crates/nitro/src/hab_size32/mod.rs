//! Port of `HabSize32Merger`: Hotel Tools "Add size 32 to HAB furniture". Habbo's .hab furniture
//! has no size 32 (the zoomed-out room). When a .nitro (or .hab) of the same furni has it, its
//! size 32 sprites, assets and visualization are copied in. The 64 sprites and the rest of the json
//! stay as they are; the 32 sprites are added below the sheet.
//! Default: Habbo_Default/hof_furni/hab + SWFCompiler/furniture/nitro -> SWFCompiler/furniture/hab.

mod node;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use habbo_core::paths::HabboAssetFolder;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_core::{console, imaging, out, outln};
use image::RgbaImage;

use crate::{hab, nitro_to_hab, sheet_webp, writer, FileEntry};
use node::Node;

const GAP: i64 = 1;

/// The .NET message for the `!` dereference of a missing node.
const NULL_REFERENCE: &str = "Object reference not set to an instance of an object.";

/// `HabSize32Merger.FurnitureAsync`.
pub async fn furniture() -> Result<()> {
    let base_dir = runtime::router().asset_directory(WorkspaceAssetKind::Furniture, Path::new("SWFCompiler").join("furniture"));
    let hab_dir = ask("Habbo .hab furniture (without size 32)", HabboAssetFolder::FURNITURE.hab());
    let donor_dir = ask("Furniture with size 32 (.nitro or .hab)", writer::folder_for(&base_dir, ".nitro"));
    let output_dir = writer::folder_for(&base_dir, ".hab");

    for folder in [&hab_dir, &donor_dir] {
        if folder.is_dir() {
            continue;
        }
        std::fs::create_dir_all(folder)?;
        outln!("📁 Created {}; put the files there and run this tool again.", folder.display());
        return Ok(());
    }

    let mut donors: HashMap<String, PathBuf> = HashMap::new();
    for file in crate::bundle_tools::files_recursive(&donor_dir) {
        let lower = file.to_string_lossy().to_lowercase();
        if lower.ends_with(".nitro") || lower.ends_with(".hab") {
            donors.entry(fold(&habbo_core::paths::file_stem(&file))).or_insert(file);
        }
    }
    let hab_files: Vec<PathBuf> =
        crate::bundle_tools::files_recursive(&hab_dir).into_iter().filter(|p| habbo_core::paths::has_ext(p, "hab")).collect();
    if hab_files.is_empty() {
        outln!("⚠️ No .hab files found in '{}'.", hab_dir.display());
        return Ok(());
    }

    std::fs::create_dir_all(&output_dir)?;
    outln!(
        "Adding size 32 to {} .hab files from {} files in {} -> {}",
        hab_files.len(),
        donors.len(),
        donor_dir.display(),
        output_dir.display()
    );

    let stopwatch = Instant::now();
    let state = Arc::new(State::default());
    let donors = Arc::new(donors);

    let (s, out_dir) = (state.clone(), output_dir.clone());
    crate::bundle_tools::for_each_parallel(hab_files.clone(), crate::bundle_tools::processors_minus_one(), move |hab_file: &PathBuf| {
        let name = habbo_core::paths::file_stem(hab_file);
        let output_path = out_dir.join(format!("{name}.hab"));
        let result = (|| -> Result<()> {
            if output_path.is_file() {
                s.skip("already in the output folder");
                return Ok(());
            }
            let Some(donor_file) = donors.get(&fold(&name)) else {
                s.skip("no file with size 32 of the same name");
                return Ok(());
            };

            let hab_bytes = std::fs::read(hab_file)?;
            let (result, reason) = merge(&hab_bytes, &std::fs::read(donor_file)?)?;
            let Some(result) = result else {
                s.skip(&reason);
                return Ok(());
            };

            habbo_core::workspace::write_output(&output_path, &result)?;
            s.merged.fetch_add(1, Ordering::SeqCst);
            s.input_bytes.fetch_add(hab_bytes.len() as u64, Ordering::SeqCst);
            s.output_bytes.fetch_add(result.len() as u64, Ordering::SeqCst);
            Ok(())
        })();
        if let Err(error) = result {
            s.failed.fetch_add(1, Ordering::SeqCst);
            outln!("❌ {name}.hab: {error}");
        }
    })
    .await?;

    let mut reasons: Vec<(String, usize)> = state.reasons.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
    // NOTE(port): C# ordered a ConcurrentDictionary by count only; ties come out in name order here.
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (why, count) in reasons {
        outln!("   skipped {count}: {why}");
    }

    habbo_core::summary::print_summary(
        "Add size 32 to HAB furniture",
        hab_files.len(),
        state.merged.load(Ordering::SeqCst),
        state.skipped.load(Ordering::SeqCst),
        state.failed.load(Ordering::SeqCst),
        state.input_bytes.load(Ordering::SeqCst),
        state.output_bytes.load(Ordering::SeqCst),
        stopwatch.elapsed(),
        &output_dir,
        Some("HAB with size 32"),
    );
    Ok(())
}

#[derive(Default)]
struct State {
    merged: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    input_bytes: AtomicU64,
    output_bytes: AtomicU64,
    reasons: Mutex<HashMap<String, usize>>,
}

impl State {
    fn skip(&self, why: &str) {
        self.skipped.fetch_add(1, Ordering::SeqCst);
        *self.reasons.lock().unwrap().entry(why.to_string()).or_insert(0) += 1;
    }
}

/// `StringComparer.OrdinalIgnoreCase` key.
fn fold(name: &str) -> String {
    name.to_uppercase()
}

fn ask(what: &str, default_path: PathBuf) -> PathBuf {
    outln!("{what}:");
    outln!(" [1] {}", default_path.display());
    outln!(" [2] Custom folder path");
    out!("Select [Default is 1]: ");
    if console::read_line_opt().map(|s| s.trim().to_string()).as_deref() != Some("2") {
        return default_path;
    }
    out!("Enter custom directory path: ");
    match console::read_line_opt() {
        Some(input) => {
            let input = input.trim().trim_matches(['"', '\'']);
            if input.trim().is_empty() {
                default_path
            } else {
                PathBuf::from(input)
            }
        }
        None => default_path,
    }
}

fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.to_lowercase().ends_with(suffix)
}

fn is_sheet(name: &str) -> bool {
    ends_with_ci(name, ".png") || ends_with_ci(name, ".webp")
}

/// `node[key]!`.
fn req<'a>(node: &'a Node, key: &str) -> Result<&'a Node> {
    node.prop(key).ok_or_else(|| anyhow!(NULL_REFERENCE))
}

fn req_mut<'a>(node: &'a mut Node, key: &str) -> Result<&'a mut Node> {
    match node.get_mut(key) {
        Some(Node::Null) | None => Err(anyhow!(NULL_REFERENCE)),
        Some(found) => Ok(found),
    }
}

fn visualization(json: &Node, size: i32) -> Result<Option<Node>> {
    let Some(visualizations) = json.prop("visualizations") else { return Ok(None) };
    for v in visualizations.as_array()? {
        if !matches!(v, Node::Object(_)) {
            continue;
        }
        if let Some(found) = v.prop("size") {
            if found.get_int()? == size {
                return Ok(Some(v.clone()));
            }
        }
    }
    Ok(None)
}

fn layer_count(visualization: &Node) -> Result<i32> {
    visualization.prop("layerCount").map(Node::get_int).unwrap_or(Ok(0))
}

fn frame(x: i64, y: i64, w: i64, h: i64) -> Node {
    let obj = |pairs: Vec<(&str, Node)>| Node::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    obj(vec![
        ("frame", obj(vec![("x", Node::int(x)), ("y", Node::int(y)), ("w", Node::int(w)), ("h", Node::int(h))])),
        ("rotated", Node::Bool(false)),
        ("trimmed", Node::Bool(false)),
        ("spriteSourceSize", obj(vec![("x", Node::int(0)), ("y", Node::int(0)), ("w", Node::int(w)), ("h", Node::int(h))])),
        ("sourceSize", obj(vec![("w", Node::int(w)), ("h", Node::int(h))])),
        ("pivot", obj(vec![("x", Node::Number("0.5".into())), ("y", Node::Number("0.5".into()))])),
    ])
}

#[derive(Clone, Copy)]
struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

/// `HabSize32Merger.Merge`: the .hab with the size 32 of the donor bundle (.nitro or .hab) added,
/// or `None` with the reason when it cannot be: the .hab has a size 32 already, the donor has
/// none, or their 64 graphics differ.
pub fn merge(hab_bytes: &[u8], donor: &[u8]) -> Result<(Option<Vec<u8>>, String)> {
    let skip = |reason: &str| Ok((None, reason.to_string()));
    let (bundle_name, mut files) = hab::read(hab_bytes)?;
    let donor_files: Vec<FileEntry> = if hab::is_hab(donor) { hab::read(donor)?.1 } else { nitro_to_hab::read_nitro(donor)? };

    let json_index = files.iter().position(|f| ends_with_ci(&f.0, ".json"));
    let donor_json_file = donor_files.iter().find(|f| ends_with_ci(&f.0, ".json"));
    let (Some(json_index), Some(donor_json_file)) = (json_index, donor_json_file) else {
        return skip("no json in the bundle");
    };

    let mut json = node::parse(&files[json_index].1)?;
    let donor_json = node::parse(&donor_json_file.1)?;
    json.as_object()?;
    donor_json.as_object()?;
    let name = match json.prop("name") {
        Some(n) => n.get_string()?,
        None => bundle_name.clone(),
    };
    let donor_name = match donor_json.prop("name") {
        Some(n) => n.get_string()?,
        None => name.clone(),
    };

    if visualization(&json, 32)?.is_some() {
        return skip("already has size 32");
    }
    let donor32 = visualization(&donor_json, 32)?;
    let donor64 = visualization(&donor_json, 64)?;
    let hab64 = visualization(&json, 64)?;
    let (Some(donor32), Some(donor64), Some(hab64)) = (donor32, donor64, hab64) else {
        return skip("the other file has no size 32");
    };

    // Same furni version: the 32 art must belong to these 64 graphics.
    let assets = req(&json, "assets")?.as_object()?;
    let donor_assets = req(&donor_json, "assets")?.as_object()?;
    let assets64: HashSet<&str> = assets.iter().map(|(k, _)| k.as_str()).filter(|k| k.contains("_64_")).collect();
    let donor_assets64: HashSet<&str> = donor_assets.iter().map(|(k, _)| k.as_str()).filter(|k| k.contains("_64_")).collect();
    if assets64 != donor_assets64 || layer_count(&hab64)? != layer_count(&donor64)? || layer_count(&donor32)? != layer_count(&hab64)? {
        return skip("the 64 graphics differ (another version of the furni)");
    }

    let image_index = files.iter().position(|f| is_sheet(&f.0));
    let donor_image_file = donor_files.iter().find(|f| is_sheet(&f.0));
    let (Some(image_index), Some(donor_image_file)) = (image_index, donor_image_file) else {
        return skip("no sprite sheet in the bundle");
    };

    req(req(&json, "spritesheet")?, "frames")?.as_object()?;
    let donor_frames = req(req(&donor_json, "spritesheet")?, "frames")?;
    donor_frames.as_object()?;
    let assets32: Vec<(String, Node)> = donor_assets.iter().filter(|(k, _)| k.contains("_32_")).cloned().collect();

    // The 32 sprites with their own image; the others point at one of these with "source".
    let mut sprites: Vec<(String, Rect)> = Vec::new();
    for (asset_name, asset) in &assets32 {
        if asset.prop("source").is_some() {
            continue;
        }
        let frame_node = donor_frames.prop(&format!("{donor_name}_{asset_name}")).and_then(|f| f.prop("frame"));
        let Some(frame_node @ Node::Object(_)) = frame_node else {
            return skip(&format!("{asset_name} has no sprite"));
        };
        let int = |key: &str| -> Result<i64> { Ok(req(frame_node, key)?.get_int()? as i64) };
        sprites.push((asset_name.clone(), Rect { x: int("x")?, y: int("y")?, w: int("w")?, h: int("h")? }));
    }

    let sheet = imaging::decode_rgba(&files[image_index].1)?;
    let donor_sheet = imaging::decode_rgba(&donor_image_file.1)?;

    // Shelf-pack the 32 sprites below the existing sheet, tallest first.
    let sheet_w = sheet.width() as i64;
    let sheet_h = sheet.height() as i64;
    let width = sheet_w.max(sprites.iter().map(|s| s.1.w).max().unwrap_or(0));
    let mut order: Vec<&(String, Rect)> = sprites.iter().collect();
    order.sort_by(|a, b| b.1.h.cmp(&a.1.h).then_with(|| a.0.cmp(&b.0)));
    let mut placed: HashMap<&str, (i64, i64)> = HashMap::new();
    let (mut x, mut y, mut row_height) = (0i64, sheet_h + GAP, 0i64);
    for (asset_name, area) in order {
        if x > 0 && x + area.w > width {
            x = 0;
            y += row_height + GAP;
            row_height = 0;
        }
        placed.insert(asset_name.as_str(), (x, y));
        x += area.w + GAP;
        row_height = row_height.max(area.h);
    }
    let height = if sprites.is_empty() { sheet_h } else { y + row_height };

    if width <= 0 || height <= 0 {
        bail!("Specified argument was out of the range of valid values. (Parameter 'width')");
    }
    let mut merged = RgbaImage::new(width as u32, height as u32);
    draw(&mut merged, &sheet, Rect { x: 0, y: 0, w: sheet_w, h: sheet_h }, (0, 0));
    let mut new_frames: Vec<(String, Node)> = Vec::new();
    for (asset_name, area) in &sprites {
        if area.w <= 0 || area.h <= 0 || area.x < 0 || area.y < 0 || area.x + area.w > donor_sheet.width() as i64 || area.y + area.h > donor_sheet.height() as i64 {
            bail!("Crop rectangle should be smaller than the source image. (Parameter 'cropRectangle')");
        }
        let at = placed[asset_name.as_str()];
        draw(&mut merged, &donor_sheet, *area, at);
        new_frames.push((format!("{name}_{asset_name}"), frame(at.0, at.1, area.w, area.h)));
    }

    // Now change the json.
    {
        let frames = req_mut(req_mut(&mut json, "spritesheet")?, "frames")?;
        for (key, value) in new_frames {
            frames.set(&key, value)?;
        }
    }
    {
        let assets = req_mut(&mut json, "assets")?;
        for (asset_name, asset) in &assets32 {
            assets.set(asset_name, asset.clone())?;
        }
    }
    req_mut(&mut json, "visualizations")?.as_array_mut()?.push(donor32);

    // Same image format as before: a WebP sheet stays WebP (lossless, pixel-checked).
    let mut image_name = files[image_index].0.clone();
    let png = imaging::encode_png(&merged)?;
    let mut image = png.clone();
    let webp = if ends_with_ci(&image_name, ".webp") { sheet_webp::try_convert(&png, &name)?.ok() } else { None };
    match webp {
        Some(webp) => image = webp,
        None => image_name = sheet_webp::change_extension(&image_name, ".png"),
    }

    {
        let meta = req_mut(req_mut(&mut json, "spritesheet")?, "meta")?;
        meta.as_object()?;
        meta.set("image", Node::String(habbo_core::paths::file_name(Path::new(&image_name))))?;
        meta.set("size", Node::Object(vec![("w".into(), Node::int(width)), ("h".into(), Node::int(height))]))?;
    }

    files[json_index].1 = node::to_bytes(&json);
    files[image_index] = (image_name, image);

    let reason = format!("added {} size 32 assets", assets32.len());
    Ok((Some(hab::write(&bundle_name, &files)?), reason))
}

/// ImageSharp `DrawImage(source, at, 1f)` onto a transparent area: Normal/SrcOver over an empty
/// backdrop gives the source pixel, except that a fully transparent one becomes (0,0,0,0).
fn draw(target: &mut RgbaImage, source: &RgbaImage, area: Rect, at: (i64, i64)) {
    for sy in 0..area.h {
        for sx in 0..area.w {
            let (tx, ty) = (at.0 + sx, at.1 + sy);
            if tx < 0 || ty < 0 || tx >= target.width() as i64 || ty >= target.height() as i64 {
                continue;
            }
            let pixel = *source.get_pixel((area.x + sx) as u32, (area.y + sy) as u32);
            let pixel = if pixel.0[3] == 0 { image::Rgba([0, 0, 0, 0]) } else { pixel };
            target.put_pixel(tx as u32, ty as u32, pixel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        imaging::encode_png(&RgbaImage::from_pixel(w, h, image::Rgba(color))).unwrap()
    }

    fn hab_json() -> String {
        r#"{"name":"chair","assets":{"chair_64_a_0_0":{"x":1}},"visualizations":[{"size":64,"layerCount":1}],
            "spritesheet":{"frames":{"chair_chair_64_a_0_0":{"frame":{"x":0,"y":0,"w":4,"h":4}}},"meta":{"image":"chair.png","size":{"w":4,"h":4}}}}"#
            .to_string()
    }

    fn donor_json() -> String {
        r#"{"name":"chair","assets":{"chair_64_a_0_0":{"x":1},"chair_32_a_0_0":{"x":2},"chair_32_a_2_0":{"source":"chair_32_a_0_0","flipH":true}},
            "visualizations":[{"size":64,"layerCount":1},{"size":32,"layerCount":1}],
            "spritesheet":{"frames":{"chair_chair_64_a_0_0":{"frame":{"x":0,"y":0,"w":4,"h":4}},"chair_chair_32_a_0_0":{"frame":{"x":4,"y":0,"w":2,"h":3}}},"meta":{}}}"#
            .to_string()
    }

    #[test]
    fn adds_size_32_below_the_sheet() {
        let hab_bytes = hab::write("chair", &[("chair.json".into(), hab_json().into_bytes()), ("chair.png".into(), sheet(4, 4, [255, 0, 0, 255]))]).unwrap();
        let mut donor_sheet = RgbaImage::from_pixel(6, 4, image::Rgba([0, 0, 255, 255]));
        donor_sheet.put_pixel(4, 0, image::Rgba([0, 255, 0, 255]));
        let donor = hab::write(
            "chair",
            &[("chair.json".into(), donor_json().into_bytes()), ("chair.png".into(), imaging::encode_png(&donor_sheet).unwrap())],
        )
        .unwrap();

        let (result, reason) = merge(&hab_bytes, &donor).unwrap();
        assert_eq!(reason, "added 2 size 32 assets");
        let (name, files) = hab::read(&result.unwrap()).unwrap();
        assert_eq!(name, "chair");
        assert_eq!(files[1].0, "chair.png");

        let json = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(json.contains(r#""chair_chair_32_a_0_0":{"frame":{"x":0,"y":5,"w":2,"h":3},"rotated":false,"trimmed":false,"spriteSourceSize":{"x":0,"y":0,"w":2,"h":3},"sourceSize":{"w":2,"h":3},"pivot":{"x":0.5,"y":0.5}}"#), "{json}");
        assert!(json.contains(r#""chair_32_a_2_0":{"source":"chair_32_a_0_0","flipH":true}"#));
        assert!(json.contains(r#"{"size":32,"layerCount":1}]"#));
        assert!(json.ends_with(r#""meta":{"image":"chair.png","size":{"w":4,"h":8}}}}"#), "{json}");

        let merged = imaging::decode_rgba(&files[1].1).unwrap();
        assert_eq!(merged.dimensions(), (4, 8));
        assert_eq!(merged.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(merged.get_pixel(0, 4).0, [0, 0, 0, 0]);
        assert_eq!(merged.get_pixel(0, 5).0, [0, 255, 0, 255]);
        assert_eq!(merged.get_pixel(1, 7).0, [0, 0, 255, 255]);

        // Merging again: the output has a size 32 now.
        let hab_with_32 = hab::write("chair", &files).unwrap();
        assert_eq!(merge(&hab_with_32, &donor).unwrap().1, "already has size 32");
    }

    #[test]
    fn skips_other_versions_and_missing_parts() {
        let hab_bytes = hab::write("chair", &[("chair.json".into(), hab_json().into_bytes()), ("chair.png".into(), sheet(4, 4, [1, 2, 3, 255]))]).unwrap();
        let other = donor_json().replace("chair_64_a_0_0\":{\"x\":1},\"chair_32", "chair_64_b_0_0\":{\"x\":1},\"chair_32");
        let donor = hab::write("chair", &[("chair.json".into(), other.into_bytes())]).unwrap();
        assert_eq!(merge(&hab_bytes, &donor).unwrap().1, "the 64 graphics differ (another version of the furni)");

        let donor = hab::write("chair", &[("chair.json".into(), hab_json().into_bytes())]).unwrap();
        assert_eq!(merge(&hab_bytes, &donor).unwrap().1, "the other file has no size 32");

        let donor = hab::write("chair", &[("chair.json".into(), donor_json().into_bytes())]).unwrap();
        assert_eq!(merge(&hab_bytes, &donor).unwrap().1, "no sprite sheet in the bundle");

        let donor = hab::write("chair", &[("chair.png".into(), vec![1])]).unwrap();
        assert_eq!(merge(&hab_bytes, &donor).unwrap().1, "no json in the bundle");
    }
}
