//! `EffectIconsExtractor` (EffectIcons.cs): the avatar effect icons (fx_icon_<type>) for the
//! client's avatareditor.effects.icon.url. Habbo does not put them on its CDN: they ship inside
//! the Habbo Classic app, at HabboClassicWin.zip/resources/app.asar/client/generated/
//! habbo-inventory-com.hab. The source can be the zip, the app.asar, the .hab, or a folder with
//! any of them or with already extracted icons. Output: Habbo_Default/fx_icons/fx_icon_<type>.png.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use habbo_core::{paths, Color};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::asset_downloader::{is_png, save};
use crate::common::{self, log, FetchError};

pub const BUNDLE_NAME: &str = "habbo-inventory-com.hab";
const ZIP_ASAR_PATH: &str = "resources/app.asar";
const DEFAULT_SOURCE: &str = "./Habbo_Default/import/effect_icons";
const OUTPUT_FOLDER: &str = "./Habbo_Default/fx_icons";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// fx_icon_12_png inside the bundle; fx_icon_12_png.png or fx_icon_12.png once extracted.
fn icon_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^fx_icon_([0-9]+)(?:_png)?(?:\.png)?$").unwrap())
}

/// `EffectIconsExtractor.ExtractAsync`.
pub async fn extract_effect_icons() -> Result<()> {
    let config = common::load_config()?;
    let mut source = config
        .get("AppSettings:effect_icons_source")
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| DEFAULT_SOURCE.to_string());

    // A download link (for example a mirror of the Habbo Classic app) is fetched first;
    // a link put in effect_icons_source counts as well.
    let mut url = config
        .get("AppSettings:effect_icons_url")
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_matches('"').to_string());

    if is_link(&source) {
        url.get_or_insert_with(|| source.clone());
        source = DEFAULT_SOURCE.to_string();
    }

    if let Some(url) = url {
        if let Some(downloaded) = download_source(&url).await? {
            source = downloaded.to_string_lossy().into_owned();
        }
    }

    let source_path = PathBuf::from(&source);
    let icons = tokio::task::spawn_blocking(move || -> Result<BTreeMap<i32, Vec<u8>>> {
        let mut icons = BTreeMap::new();
        if source_path.is_file() {
            read_file(&source_path, &mut icons);
        } else if source_path.is_dir() {
            read_folder(&source_path, &mut icons)?;
        } else {
            std::fs::create_dir_all(DEFAULT_SOURCE)?;
            log(Color::Red, &format!("Effect icons source not found: {}", source_path.display()));
        }
        Ok(icons)
    })
    .await??;

    if icons.is_empty() {
        log(
            Color::Yellow,
            &format!(
                "No effect icons found. Put HabboClassicWin.zip, its resources/app.asar, {BUNDLE_NAME} or a folder of\n\
                 fx_icon_<type> PNG files in {DEFAULT_SOURCE}, or point config.ini effect_icons_source at it."
            ),
        );
        return Ok(());
    }

    std::fs::create_dir_all(OUTPUT_FOLDER)?;

    let (mut written, mut unchanged) = (0, 0);
    for (kind, png) in &icons {
        let path = Path::new(OUTPUT_FOLDER).join(format!("fx_icon_{kind}.png"));
        if path.is_file() && std::fs::read(&path)? == *png {
            unchanged += 1;
            continue;
        }
        save(&path, png)?;
        written += 1;
    }

    log(Color::Green, &format!("Effect icons: {written} written, {unchanged} already up to date, in {OUTPUT_FOLDER}."));
    log(Color::Gray, "Copy the fx_icons folder into the client's image library (avatareditor.effects.icon.url).");
    Ok(())
}

fn is_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn download_client() -> &'static reqwest::Client {
    // The whole app zip is large: allow more time than the asset downloads get.
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    common::client_with_timeout(&CLIENT, DOWNLOAD_TIMEOUT)
}

/// `Path.GetInvalidFileNameChars()` on Windows.
fn has_invalid_file_name_chars(name: &str) -> bool {
    name.chars().any(|c| (c as u32) < 32 || matches!(c, '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\' | '/'))
}

/// The stem and extension of the local copy of `url`: the link's file name plus the first 10
/// hex digits of the SHA-256 of the absolute link, so a newer link is fetched again.
pub fn download_name(url: &reqwest::Url) -> (String, String) {
    let mut name = url.path().rsplit('/').next().unwrap_or("").to_string();
    if name.trim().is_empty() || has_invalid_file_name_chars(&name) {
        name = "effect_icons_download".to_string();
    }
    let digest = Sha256::digest(url.as_str().as_bytes());
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect::<String>()[..10].to_string();
    let path = Path::new(&name);
    let stem = format!("{}-{hash}", paths::file_stem(path));
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (stem, extension)
}

/// Downloads the effect_icons_url file (zip, app.asar, .hab or one PNG) into the import folder,
/// once. Anything that is not one of those (an error page) is thrown away.
async fn download_source(url: &str) -> Result<Option<PathBuf>> {
    let uri = match reqwest::Url::parse(url) {
        Ok(uri) if uri.scheme() == "http" || uri.scheme() == "https" => uri,
        _ => {
            log(Color::Red, &format!("effect_icons_url is not an http(s) link: {url}"));
            return Ok(None);
        }
    };

    std::fs::create_dir_all(DEFAULT_SOURCE)?;

    // Every release's link can end in "app.asar": the link's hash keeps each download apart.
    let (stem, extension) = download_name(&uri);
    let mut path = Path::new(DEFAULT_SOURCE).join(format!("{stem}{extension}"));

    let mut earlier: Vec<PathBuf> = std::fs::read_dir(DEFAULT_SOURCE)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = paths::file_name(p);
            name.to_lowercase().starts_with(&stem.to_lowercase()) && !name.to_lowercase().ends_with(".part")
        })
        .collect();
    earlier.sort();
    if let Some(earlier) = earlier.into_iter().next() {
        log(Color::DarkCyan, &format!("Using the earlier download {} (delete it to download again).", earlier.display()));
        return Ok(Some(earlier));
    }

    let mut temp = path.as_os_str().to_owned();
    temp.push(".part");
    let temp = PathBuf::from(temp);

    let result: Result<Option<PathBuf>, FetchError> = async {
        log(Color::Blue, &format!("Downloading {uri}..."));
        let response = common::send_with(download_client().get(uri.clone()), Some(DOWNLOAD_TIMEOUT)).await?;
        common::ensure_success(&response)?;
        let bytes = common::body(response).await?;
        std::fs::write(&temp, bytes)?;

        let Some(kind) = detect_kind(&temp)? else {
            std::fs::remove_file(&temp)?;
            log(Color::Red, "The download is not a zip, app.asar, .hab or PNG file.");
            return Ok(None);
        };

        // Give the file the extension its content has, so it is read the right way.
        if !path.to_string_lossy().to_lowercase().ends_with(kind) {
            let mut with_kind = path.as_os_str().to_owned();
            with_kind.push(kind);
            path = PathBuf::from(with_kind);
        }
        std::fs::rename(&temp, &path)?;
        log(Color::Green, &format!("Downloaded {}.", path.display()));
        Ok(Some(path.clone()))
    }
    .await;

    match result {
        Ok(path) => Ok(path),
        Err(error) => {
            if temp.is_file() {
                std::fs::remove_file(&temp)?;
            }
            log(Color::Red, &format!("Could not download {uri}: {error}"));
            Ok(None)
        }
    }
}

/// The kind of a downloaded source from its first 8 bytes.
pub fn detect_kind_bytes(head: &[u8]) -> Option<&'static str> {
    if head.len() < 8 {
        return None;
    }
    if head[..4] == [b'P', b'K', 3, 4] {
        return Some(".zip");
    }
    if head[..4] == [b'H', b'A', b'B', 0] {
        return Some(".hab");
    }
    let mut padded = head[..8].to_vec();
    padded.push(0);
    if is_png(&padded) {
        return Some(".png");
    }
    if u32::from_le_bytes([head[0], head[1], head[2], head[3]]) == 4 {
        return Some(".asar");
    }
    None
}

fn detect_kind(path: &Path) -> std::io::Result<Option<&'static str>> {
    let mut head = [0u8; 8];
    let mut file = std::fs::File::open(path)?;
    let mut read = 0;
    while read < head.len() {
        let n = file.read(&mut head[read..])?;
        if n == 0 {
            break;
        }
        read += n;
    }
    Ok(detect_kind_bytes(&head[..read]))
}

fn read_file(path: &Path, icons: &mut BTreeMap<i32, Vec<u8>>) {
    let result = (|| -> Result<()> {
        let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let bundle = match extension.as_str() {
            "zip" => from_zip(path)?,
            "asar" => {
                let mut unpacked = path.as_os_str().to_owned();
                unpacked.push(".unpacked");
                crate::asar::read_file(&std::fs::read(path)?, BUNDLE_NAME, Some(Path::new(&unpacked)))?
            }
            _ => Some(std::fs::read(path)?),
        };

        let Some(bundle) = bundle else {
            log(Color::Red, &format!("{BUNDLE_NAME} not found in {}.", path.display()));
            return Ok(());
        };

        let (_, files) = habbo_nitro::hab::read(&bundle)?;
        for (name, data) in files {
            add(&name, data, icons);
        }
        Ok(())
    })();
    if let Err(error) = result {
        log(Color::Red, &format!("Could not read {}: {error}", path.display()));
    }
}

fn from_zip(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;

    // An entry Electron keeps outside the archive sits in app.asar.unpacked.
    let prefix = format!("{ZIP_ASAR_PATH}.unpacked/").to_lowercase();
    let mut unpacked = None;
    let mut asar = None;
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index)?;
        let full_name = entry.name().to_string();
        let name = full_name.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        if unpacked.is_none() && full_name.to_lowercase().starts_with(&prefix) && name.eq_ignore_ascii_case(BUNDLE_NAME) {
            unpacked = Some(index);
        }
        if asar.is_none() && full_name == ZIP_ASAR_PATH {
            asar = Some(index);
        }
    }

    if let Some(index) = unpacked {
        return Ok(Some(read_entry(&mut zip, index)?));
    }
    match asar {
        None => Ok(None),
        Some(index) => {
            let bytes = read_entry(&mut zip, index)?;
            crate::asar::read_file(&bytes, BUNDLE_NAME, None)
        }
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, index: usize) -> Result<Vec<u8>> {
    let mut entry = zip.by_index(index)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// `Directory.EnumerateFiles` search pattern (`*` and `?`), case-insensitive like Windows.
pub fn matches_pattern(name: &str, pattern: &str) -> bool {
    fn matches(name: &[char], pattern: &[char]) -> bool {
        match pattern.first() {
            None => name.is_empty(),
            Some('*') => (0..=name.len()).any(|i| matches(&name[i..], &pattern[1..])),
            Some('?') => !name.is_empty() && matches(&name[1..], &pattern[1..]),
            Some(c) => name.first().is_some_and(|n| n.to_lowercase().eq(c.to_lowercase())) && matches(&name[1..], &pattern[1..]),
        }
    }
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    matches(&name, &pattern)
}

fn read_folder(folder: &Path, icons: &mut BTreeMap<i32, Vec<u8>>) -> Result<()> {
    let files = paths::files_recursive(folder);
    let named = |pattern: &'static str| -> Vec<&PathBuf> {
        files.iter().filter(|f| matches_pattern(&paths::file_name(f), pattern)).collect()
    };

    for bundle in named(BUNDLE_NAME) {
        read_file(bundle, icons);
    }
    for asar in named("*.asar") {
        read_file(asar, icons);
    }
    for zip in named("HabboClassic*.zip") {
        read_file(zip, icons);
    }
    for file in named("fx_icon_*") {
        add(&paths::file_name(file), std::fs::read(file)?, icons);
    }
    Ok(())
}

/// Only real PNGs: a renamed or broken file must not end up as an icon. The first one of a type wins.
pub fn add(name: &str, data: Vec<u8>, icons: &mut BTreeMap<i32, Vec<u8>>) {
    let Some(captures) = icon_name().captures(name) else { return };
    if !is_png(&data) {
        return;
    }
    let Ok(kind) = captures[1].parse::<i32>() else { return };
    if kind <= 0 {
        return;
    }
    icons.entry(kind).or_insert(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";

    #[test]
    fn icon_names() {
        let mut icons = BTreeMap::new();
        add("fx_icon_12_png", PNG.to_vec(), &mut icons);
        add("FX_ICON_3.png", PNG.to_vec(), &mut icons);
        add("fx_icon_4_png.png", PNG.to_vec(), &mut icons);
        add("fx_icon_0.png", PNG.to_vec(), &mut icons);
        add("fx_icon_5.gif", PNG.to_vec(), &mut icons);
        add("fx_icon_6.png", b"not a png at all".to_vec(), &mut icons);
        add("fx_icon_99999999999.png", PNG.to_vec(), &mut icons);
        add("fx_icon_12.png", b"\x89PNG\r\n\x1a\nother".to_vec(), &mut icons);
        assert_eq!(icons.keys().copied().collect::<Vec<_>>(), [3, 4, 12]);
        assert_eq!(icons[&12], PNG, "first one wins");
    }

    #[test]
    fn kinds() {
        assert_eq!(detect_kind_bytes(b"PK\x03\x04abcd"), Some(".zip"));
        assert_eq!(detect_kind_bytes(b"HAB\0abcd"), Some(".hab"));
        assert_eq!(detect_kind_bytes(b"\x89PNG\r\n\x1a\n"), Some(".png"));
        assert_eq!(detect_kind_bytes(&[4, 0, 0, 0, 1, 2, 3, 4]), Some(".asar"));
        assert_eq!(detect_kind_bytes(b"<html><b"), None);
        assert_eq!(detect_kind_bytes(b"PK\x03"), None);
    }

    #[test]
    fn patterns() {
        assert!(matches_pattern("HabboClassicWin.ZIP", "HabboClassic*.zip"));
        assert!(matches_pattern("app.asar", "*.asar"));
        assert!(!matches_pattern("app.asar.bak", "*.asar"));
        assert!(matches_pattern("fx_icon_1.png", "fx_icon_*"));
    }

    #[test]
    fn download_names_hash_the_link() {
        let a = download_name(&reqwest::Url::parse("https://example.com/r1/app.asar").unwrap());
        let b = download_name(&reqwest::Url::parse("https://example.com/r2/app.asar").unwrap());
        assert!(a.0.starts_with("app-") && a.0.len() == "app-".len() + 10);
        assert_eq!(a.1, ".asar");
        assert_ne!(a.0, b.0);
        let c = download_name(&reqwest::Url::parse("https://example.com/").unwrap());
        assert!(c.0.starts_with("effect_icons_download-"));
        assert_eq!(c.1, "");
    }

    #[test]
    fn zip_with_asar_and_hab() {
        use std::io::Write;
        let hab = habbo_nitro::hab::write("inv", &[("fx_icon_7_png".into(), PNG.to_vec()), ("x.json".into(), b"{}".to_vec())]).unwrap();
        let asar = crate::asar::build(&[("client/generated/habbo-inventory-com.hab", &hab)]);

        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("HabboClassicWin.zip");
        {
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("resources/app.asar", options).unwrap();
            writer.write_all(&asar).unwrap();
            writer.finish().unwrap();
        }
        let mut icons = BTreeMap::new();
        read_folder(dir.path(), &mut icons).unwrap();
        assert_eq!(icons.keys().copied().collect::<Vec<_>>(), [7]);
    }
}
