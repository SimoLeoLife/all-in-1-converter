//! `ConverterSettings`: spritesheet format (WebP vs PNG) and download format (swf/hab/both),
//! read lazily from config.ini and cached until [`reload`].

use std::sync::RwLock;

use crate::outln;

struct Cache {
    spritesheet_format: Option<String>,
    download_format: Option<String>,
    webp_method: Option<i32>,
}

static CACHE: RwLock<Cache> = RwLock::new(Cache { spritesheet_format: None, download_format: None, webp_method: None });

/// "webp" (default) or "png", from `spritesheet_format=` in config.ini.
pub fn spritesheet_format() -> String {
    if let Some(format) = CACHE.read().unwrap().spritesheet_format.clone() {
        return format;
    }
    let format = match read_setting_prefix("spritesheet_format").as_deref() {
        Some("png") => "png",
        _ => "webp", // lighter assets and faster loading
    }
    .to_string();
    CACHE.write().unwrap().spritesheet_format = Some(format.clone());
    format
}

pub fn set_spritesheet_format(format: &str) {
    CACHE.write().unwrap().spritesheet_format = Some(format.to_string());
}

/// Forgets the cached values, so an edited config.ini is read again.
pub fn reload() {
    let mut cache = CACHE.write().unwrap();
    cache.spritesheet_format = None;
    cache.download_format = None;
    cache.webp_method = None;
}

pub fn use_webp() -> bool {
    spritesheet_format().eq_ignore_ascii_case("webp")
}

/// config.ini `download_format`: swf, hab or both, for furniture, clothes, effects and pets.
/// Older configs: `furniture_download_format`, else .swf plus the .hab when `download_hab` is on.
pub fn download_format() -> String {
    if let Some(format) = CACHE.read().unwrap().download_format.clone() {
        return format;
    }
    let value = read_setting("download_format")
        .or_else(|| read_setting("furniture_download_format"))
        .map(|v| v.to_lowercase());
    let value = match value.as_deref() {
        Some(v @ ("swf" | "hab" | "both")) => v.to_string(),
        _ => {
            let legacy = read_setting("download_hab").map(|v| v.to_lowercase());
            if matches!(legacy.as_deref(), Some("false" | "0")) { "swf" } else { "both" }.to_string()
        }
    };
    CACHE.write().unwrap().download_format = Some(value.clone());
    value
}

pub fn set_download_format(format: &str) {
    CACHE.write().unwrap().download_format = Some(format.to_string());
}

pub fn download_swf() -> bool {
    matches!(download_format().as_str(), "swf" | "both")
}

pub fn download_hab() -> bool {
    matches!(download_format().as_str(), "hab" | "both")
}

/// config.ini `webp_method`: libwebp's effort, 0 (fastest) to 6 (smallest files, the C#
/// encoder's Level6 and the default). Every level is lossless; only speed and size change.
pub fn webp_method() -> i32 {
    if let Some(method) = CACHE.read().unwrap().webp_method {
        return method;
    }
    let method = read_setting("webp_method").and_then(|v| v.parse::<i32>().ok()).filter(|v| (0..=6).contains(v)).unwrap_or(6);
    CACHE.write().unwrap().webp_method = Some(method);
    method
}

pub fn image_extension() -> &'static str {
    if use_webp() { ".webp" } else { ".png" }
}

/// The largest sheet the C# lossless WebP encoder could take (8 bytes per pixel in one
/// 1 GiB buffer). Kept so both versions pick PNG for the same oversized sheets.
pub const WEBP_LOSSLESS_PIXEL_LIMIT: i64 = (1i64 << 30) / 8; // 134,217,728 pixels

pub fn fits_webp_lossless(width: u32, height: u32) -> bool {
    width as i64 * height as i64 <= WEBP_LOSSLESS_PIXEL_LIMIT
}

/// The extension a sheet of this size is written with: the configured format, except
/// that a WebP sheet past the lossless limit falls back to PNG.
pub fn resolve_sheet_extension(width: u32, height: u32, sheet_name: &str) -> &'static str {
    if !use_webp() {
        return ".png";
    }
    if fits_webp_lossless(width, height) {
        return ".webp";
    }
    outln!(
        "\u{2139}\u{FE0F} {sheet_name}: {width}x{height} is {} pixels, past the {} the lossless WebP encoder can hold in one buffer - writing this sheet as PNG.",
        crate::console::n0(width as i64 * height as i64),
        crate::console::n0(WEBP_LOSSLESS_PIXEL_LIMIT)
    );
    ".png"
}

/// The value of a config.ini line "key=value" (any section), or `None`.
pub fn read_setting(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(crate::ini::default_config_path()).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        name.trim().eq_ignore_ascii_case(key).then(|| value.trim().to_string())
    })
}

/// C# matched `spritesheet_format` with StartsWith, then lower-cased the value.
fn read_setting_prefix(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(crate::ini::default_config_path()).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.len() >= key.len() && trimmed[..key.len()].eq_ignore_ascii_case(key) {
            if let Some((_, value)) = trimmed.split_once('=') {
                let value = value.trim().to_lowercase();
                if value == "png" || value == "webp" {
                    return Some(value);
                }
            }
        }
    }
    None
}
