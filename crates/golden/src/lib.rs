//! Golden tests: real Habbo files converted and compared with the C# output (see tests/ and
//! `tests/golden/README.md`).
//!
//! The fixtures live in `tests/golden/<category>/`:
//! - `swf/` the input SWFs,
//! - `manifest.json` every bundle the C# tool wrote, with its entry names in order,
//! - `expected/<entry>` the json entries byte for byte, and every sheet entry as a lossless
//!   PNG named `<entry stem>.png` (the WebP encoders differ, so pixels are compared, not bytes).
//!
//! This library holds the shared helpers: reading bundles, the readable diffs and the import
//! used by the `golden-import` binary to (re)build the expected files from C# .hab output.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use serde_json::{Map, Value};

pub use habbo_nitro::FileEntry;

/// `tests/golden`.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tests").join("golden")
}

/// True for the bundle entries that are sprite sheets.
pub fn is_image_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".webp") || lower.ends_with(".png")
}

/// The file in `expected/` that holds an entry: the json itself, a sheet as `<stem>.png`.
pub fn expected_file_name(entry: &str) -> String {
    if is_image_entry(entry) {
        let stem = Path::new(entry).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{stem}.png")
    } else {
        entry.to_string()
    }
}

/// The entries of a .hab or .nitro bundle, in stored order.
pub fn read_bundle(path: &Path) -> Result<Vec<FileEntry>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if habbo_nitro::hab::is_hab(&data) {
        Ok(habbo_nitro::hab::read(&data)?.1)
    } else {
        habbo_nitro::bundle::read_entries(&data)
    }
}

/// `manifest.json`: bundle name -> entry names, in file order.
pub type Manifest = Vec<(String, Vec<String>)>;

pub fn read_manifest(path: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let Value::Object(map) = serde_json::from_str::<Value>(&text)? else { bail!("{} is not a json object", path.display()) };
    map.into_iter()
        .map(|(name, entries)| {
            let entries = entries
                .as_array()
                .with_context(|| format!("manifest entry {name} is not an array"))?
                .iter()
                .map(|e| e.as_str().map(str::to_string).context("manifest entry names must be strings"))
                .collect::<Result<Vec<_>>>()?;
            Ok((name, entries))
        })
        .collect()
}

pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let map: Map<String, Value> = manifest
        .iter()
        .map(|(name, entries)| (name.clone(), Value::Array(entries.iter().cloned().map(Value::String).collect())))
        .collect();
    let mut text = serde_json::to_string_pretty(&Value::Object(map))?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

/// Lossless PNG with the best compression (keeps the committed fixtures small).
pub fn encode_png_best(image: &RgbaImage) -> Result<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage> {
    habbo_core::imaging::decode_rgba(bytes)
}

/// Writes the expected files of every bundle in `bundle_dir` (sorted by name) into
/// `category_dir/expected` and `category_dir/manifest.json`. Old expected files are removed.
pub fn import_bundles(bundle_dir: &Path, category_dir: &Path) -> Result<Manifest> {
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(bundle_dir)
        .with_context(|| format!("reading {}", bundle_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), Some("hab" | "nitro")))
        .collect();
    bundles.sort();
    if bundles.is_empty() {
        bail!("no .hab/.nitro files in {}", bundle_dir.display());
    }

    let expected_dir = category_dir.join("expected");
    if expected_dir.is_dir() {
        std::fs::remove_dir_all(&expected_dir)?;
    }
    std::fs::create_dir_all(&expected_dir)?;

    let mut manifest = Manifest::new();
    for bundle in bundles {
        let name = bundle.file_stem().unwrap().to_string_lossy().into_owned();
        let entries = read_bundle(&bundle)?;
        let mut names = Vec::new();
        for (entry, bytes) in entries {
            let target = expected_dir.join(expected_file_name(&entry));
            if is_image_entry(&entry) {
                std::fs::write(&target, encode_png_best(&decode_image(&bytes)?)?)?;
            } else {
                std::fs::write(&target, &bytes)?;
            }
            names.push(entry);
        }
        manifest.push((name, names));
    }
    write_manifest(&category_dir.join("manifest.json"), &manifest)?;
    Ok(manifest)
}

fn escape(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).escape_debug().to_string()
}

/// `None` when equal, else where the first byte differs, with context from both sides.
pub fn diff_bytes(expected: &[u8], actual: &[u8]) -> Option<String> {
    if expected == actual {
        return None;
    }
    let index = expected.iter().zip(actual).position(|(a, b)| a != b).unwrap_or(expected.len().min(actual.len()));
    let line = expected[..index].iter().filter(|&&b| b == b'\n').count() + 1;
    let column = index - expected[..index].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1) + 1;
    let start = index.saturating_sub(60);
    let context = |data: &[u8]| escape(&data[start.min(data.len())..(index + 60).min(data.len())]);
    Some(format!(
        "first difference at byte {index} (line {line}, column {column}); expected {} bytes, got {}\n      expected: …{}…\n      actual:   …{}…",
        expected.len(),
        actual.len(),
        context(expected),
        context(actual)
    ))
}

/// `None` when every pixel is equal, else the number of differing pixels and the first one.
pub fn diff_images(expected: &RgbaImage, actual: &RgbaImage) -> Option<String> {
    if expected.dimensions() != actual.dimensions() {
        return Some(format!("sheet size differs: expected {:?}, got {:?}", expected.dimensions(), actual.dimensions()));
    }
    let mut count = 0usize;
    let mut first = None;
    for (x, y, pixel) in expected.enumerate_pixels() {
        let other = actual.get_pixel(x, y);
        if pixel != other {
            count += 1;
            first.get_or_insert((x, y, pixel.0, other.0));
        }
    }
    first.map(|(x, y, e, a)| {
        format!(
            "{count} of {} pixels differ; first at ({x},{y}): expected RGBA {e:?}, got {a:?}",
            expected.width() as u64 * expected.height() as u64
        )
    })
}

/// The first place two json values differ, as `path: expected X, got Y` (objects compared by
/// key, so key order is not checked; arrays element by element).
pub fn diff_values(path: &str, expected: &Value, actual: &Value) -> Option<String> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (key, value) in e {
                let child = format!("{path}.{key}");
                match a.get(key) {
                    Some(other) => {
                        if let Some(d) = diff_values(&child, value, other) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{child}: missing")),
                }
            }
            a.keys().find(|k| !e.contains_key(*k)).map(|k| format!("{path}.{k}: not expected"))
        }
        (Value::Array(e), Value::Array(a)) => {
            for (i, (x, y)) in e.iter().zip(a).enumerate() {
                if let Some(d) = diff_values(&format!("{path}[{i}]"), x, y) {
                    return Some(d);
                }
            }
            (e.len() != a.len()).then(|| format!("{path}: {} elements expected, got {}", e.len(), a.len()))
        }
        _ if expected == actual => None,
        _ => Some(format!("{path}: expected {expected}, got {actual}")),
    }
}

fn frame_rect(frame: &Value) -> Option<(u32, u32, u32, u32)> {
    let f = frame.get("frame")?;
    let n = |k: &str| f.get(k).and_then(Value::as_u64).map(|v| v as u32);
    Some((n("x")?, n("y")?, n("w")?, n("h")?))
}

/// Compares a converted asset without depending on where the packer put each sprite: the json
/// without its `spritesheet`, the spritesheet `meta` without `size`, every frame's entry without
/// `frame.x`/`frame.y`, and every frame's pixels (cropped from each sheet at its own position).
///
/// The packer lays sprites out in directory listing order (as the C# tool did), which NTFS
/// returns sorted by name but Linux file systems do not; this is the check that still holds there.
pub fn diff_layout_independent(
    expected_json: &[u8],
    expected_sheet: Option<&RgbaImage>,
    actual_json: &[u8],
    actual_sheet: Option<&RgbaImage>,
) -> Vec<String> {
    let parse = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes);
    let (mut expected, mut actual) = match (parse(expected_json), parse(actual_json)) {
        (Ok(e), Ok(a)) => (e, a),
        (_, Err(error)) => return vec![format!("json does not parse: {error}")],
        (Err(error), _) => return vec![format!("expected json does not parse: {error}")],
    };
    let take_sheet = |v: &mut Value| v.as_object_mut().and_then(|o| o.remove("spritesheet"));
    let (expected_ss, actual_ss) = (take_sheet(&mut expected), take_sheet(&mut actual));

    let mut problems = Vec::new();
    if let Some(d) = diff_values("$", &expected, &actual) {
        problems.push(format!("json differs at {d}"));
    }

    let (mut expected_ss, mut actual_ss) = match (expected_ss, actual_ss) {
        (None, None) => return problems,
        (Some(e), Some(a)) => (e, a),
        (e, _) => {
            problems.push(format!("spritesheet {}", if e.is_some() { "missing" } else { "not expected" }));
            return problems;
        }
    };
    for ss in [&mut expected_ss, &mut actual_ss] {
        if let Some(meta) = ss.get_mut("meta").and_then(Value::as_object_mut) {
            meta.remove("size");
        }
    }
    if let Some(d) = diff_values("$.spritesheet.meta", &expected_ss["meta"], &actual_ss["meta"]) {
        problems.push(format!("json differs at {d}"));
    }

    let empty = Map::new();
    let expected_frames = expected_ss.get("frames").and_then(Value::as_object).unwrap_or(&empty);
    let actual_frames = actual_ss.get("frames").and_then(Value::as_object).unwrap_or(&empty);
    let missing: Vec<&String> = expected_frames.keys().filter(|k| !actual_frames.contains_key(*k)).collect();
    let extra: Vec<&String> = actual_frames.keys().filter(|k| !expected_frames.contains_key(*k)).collect();
    if !missing.is_empty() || !extra.is_empty() {
        problems.push(format!(
            "spritesheet frames differ: {} missing (e.g. {:?}), {} not expected (e.g. {:?})",
            missing.len(),
            missing.iter().take(3).collect::<Vec<_>>(),
            extra.len(),
            extra.iter().take(3).collect::<Vec<_>>()
        ));
    }

    let mut differing_frames = Vec::new();
    for (name, expected_frame) in expected_frames {
        let Some(actual_frame) = actual_frames.get(name) else { continue };
        let without_position = |frame: &Value| {
            let mut frame = frame.clone();
            if let Some(rect) = frame.get_mut("frame").and_then(Value::as_object_mut) {
                rect.remove("x");
                rect.remove("y");
            }
            frame
        };
        if let Some(d) = diff_values(&format!("$.spritesheet.frames.{name}"), &without_position(expected_frame), &without_position(actual_frame)) {
            problems.push(format!("json differs at {d}"));
            continue;
        }
        let (Some(expected_sheet), Some(actual_sheet)) = (expected_sheet, actual_sheet) else {
            problems.push("sheet image missing".into());
            break;
        };
        let crop = |sheet: &RgbaImage, frame: &Value| {
            let (x, y, w, h) = frame_rect(frame)?;
            (x + w <= sheet.width() && y + h <= sheet.height()).then(|| image::imageops::crop_imm(sheet, x, y, w, h).to_image())
        };
        match (crop(expected_sheet, expected_frame), crop(actual_sheet, actual_frame)) {
            (Some(e), Some(a)) => {
                if let Some(d) = diff_images(&e, &a) {
                    differing_frames.push(format!("{name}: {d}"));
                }
            }
            _ => differing_frames.push(format!("{name}: frame rectangle outside the sheet")),
        }
    }
    if !differing_frames.is_empty() {
        problems.push(format!(
            "{} of {} sprites differ in pixels; first: {}",
            differing_frames.len(),
            expected_frames.len(),
            differing_frames[0]
        ));
    }
    problems
}

/// The expected files hold the bytes the C# tool wrote on Windows, where every json and SQL line
/// ends with `Environment.NewLine` = CRLF (line breaks inside one SQL INSERT are a bare LF on
/// every OS). Both tools write `Environment.NewLine`, so on Linux the same output has LF where
/// Windows has CRLF: this turns the Windows bytes into the bytes expected on this OS.
pub fn with_platform_newlines(windows_bytes: &[u8]) -> Vec<u8> {
    if habbo_core::paths::NEWLINE == "\r\n" {
        return windows_bytes.to_vec();
    }
    let mut out = Vec::with_capacity(windows_bytes.len());
    for (i, &b) in windows_bytes.iter().enumerate() {
        if b == b'\r' && windows_bytes.get(i + 1) == Some(&b'\n') {
            continue;
        }
        out.push(b);
    }
    out
}

/// True where the sheet layout is the C# tool's (NTFS lists directories sorted by name, the
/// order the C# tool saw), so json and sheets must match byte for byte / pixel for pixel. On
/// other systems the directory order (and so the packing) differs; [`diff_layout_independent`]
/// is the check there.
pub const STRICT_LAYOUT: bool = cfg!(windows);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_diff_points_at_first_difference() {
        assert!(diff_bytes(b"abc", b"abc").is_none());
        let report = diff_bytes(b"{\n  \"a\": 1\n}", b"{\n  \"a\": 2\n}").unwrap();
        assert!(report.contains("byte 9 (line 2, column 8)"), "{report}");
        assert!(diff_bytes(b"abc", b"abcd").unwrap().contains("byte 3"));
    }

    #[test]
    fn image_diff_counts_pixels() {
        let a = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4]));
        let mut b = a.clone();
        assert!(diff_images(&a, &b).is_none());
        b.put_pixel(2, 1, image::Rgba([0, 0, 0, 0]));
        b.put_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        assert!(diff_images(&a, &b).unwrap().starts_with("2 of 6 pixels differ; first at (1,1)"));
        assert!(diff_images(&a, &RgbaImage::new(2, 2)).unwrap().contains("size"));
    }

    fn sheet_json(a: (u32, u32), b: (u32, u32), size: (u32, u32)) -> Vec<u8> {
        let frame = |(x, y): (u32, u32)| {
            serde_json::json!({"frame": {"x": x, "y": y, "w": 2, "h": 1}, "rotated": false, "sourceSize": {"w": 2, "h": 1}})
        };
        let json = serde_json::json!({
            "name": "x",
            "assets": {"a": {"x": 1}},
            "spritesheet": {"frames": {"a": frame(a), "b": frame(b)}, "meta": {"image": "x.webp", "size": {"w": size.0, "h": size.1}}}
        });
        serde_json::to_vec_pretty(&json).unwrap()
    }

    #[test]
    fn layout_independent_compare_ignores_positions_only() {
        let red = image::Rgba([255, 0, 0, 255]);
        let blue = image::Rgba([0, 0, 255, 255]);
        // "a" red at (0,0), "b" blue at (0,1) vs. a column layout swapped into a row.
        let mut column = RgbaImage::new(2, 2);
        for x in 0..2 {
            column.put_pixel(x, 0, red);
            column.put_pixel(x, 1, blue);
        }
        let mut row = RgbaImage::new(4, 1);
        for x in 0..2 {
            row.put_pixel(x + 2, 0, red);
            row.put_pixel(x, 0, blue);
        }
        let expected = sheet_json((0, 0), (0, 1), (2, 2));
        let actual = sheet_json((2, 0), (0, 0), (4, 1));
        assert!(diff_layout_independent(&expected, Some(&column), &actual, Some(&row)).is_empty());
        assert!(diff_bytes(&expected, &actual).is_some());

        row.put_pixel(3, 0, blue);
        let problems = diff_layout_independent(&expected, Some(&column), &actual, Some(&row));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("1 of 2 sprites differ in pixels; first: a: 1 of 2 pixels differ"), "{problems:?}");

        let other = String::from_utf8(actual.clone()).unwrap().replace("\"x\": 1", "\"x\": 5");
        let problems = diff_layout_independent(&expected, Some(&column), other.as_bytes(), Some(&row));
        assert!(problems.iter().any(|p| p.contains("$.assets.a.x: expected 1, got 5")), "{problems:?}");
    }

    #[test]
    fn platform_newlines() {
        let text = b"a\r\nb\nc\r\n";
        let expected: &[u8] = if cfg!(windows) { text } else { b"a\nb\nc\n" };
        assert_eq!(with_platform_newlines(text), expected);
    }

    #[test]
    fn expected_names() {
        assert_eq!(expected_file_name("chair.webp"), "chair.png");
        assert_eq!(expected_file_name("chair.json"), "chair.json");
    }
}
