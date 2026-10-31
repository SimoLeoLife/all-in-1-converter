//! `HabboAssetDownloader`: the shared download of Habbo's asset libraries (furniture, clothes,
//! effects, pets) into a Habbo_Default/<kind>/{swf,hab} folder. config.ini download_format
//! picks .swf, .hab or both; a .hab gets a lossless WebP sheet (spritesheet_format=webp) with
//! its json left untouched.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use futures::TryStreamExt;
use habbo_core::paths::{self, HabboAssetFolder};
use habbo_core::{outln, settings, Color};
use rayon::prelude::*;

use crate::common::{self, log};

pub const MAX_PARALLEL_DOWNLOADS: usize = 8;

/// `HabboAssetDownloader.Http`: 2 minute timeout.
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    common::client_with_timeout(&CLIENT, Duration::from_secs(120))
}

#[derive(Debug, Default)]
pub struct Counts {
    pub swf: AtomicUsize,
    pub hab: AtomicUsize,
    pub failed: AtomicUsize,
}

/// Creates the folders and moves a download from before the swf/ and hab/ folders into them.
pub async fn prepare_folder(folder: HabboAssetFolder) -> Result<()> {
    if settings::download_swf() {
        std::fs::create_dir_all(folder.swf())?;
    }
    if settings::download_hab() {
        std::fs::create_dir_all(folder.hab())?;
    }
    tokio::task::spawn_blocking(move || move_old_layout(folder)).await?
}

/// Downloads every library ("name", "url without extension") in the configured formats.
pub async fn download_libraries(folder: HabboAssetFolder, libraries: Vec<(String, String)>) -> Result<Counts> {
    let counts = Counts::default();
    futures::stream::iter(libraries.into_iter().map(Ok::<_, anyhow::Error>))
        .try_for_each_concurrent(MAX_PARALLEL_DOWNLOADS, |(name, base_url)| {
            let counts = &counts;
            async move { download_library(folder, &name, &base_url, counts).await }
        })
        .await?;
    Ok(counts)
}

pub async fn download_library(folder: HabboAssetFolder, name: &str, base_url: &str, counts: &Counts) -> Result<()> {
    if settings::download_swf() {
        let swf_path = folder.swf().join(format!("{name}.swf"));
        if !swf_path.is_file() {
            if let Some(swf) = try_get(&format!("{base_url}.swf")).await {
                if is_swf(&swf) {
                    save(&swf_path, &swf)?;
                    counts.swf.fetch_add(1, Ordering::SeqCst);
                    log(Color::Green, &format!("Downloaded: {name}.swf"));
                }
            }
        }
    }

    if settings::download_hab() {
        let hab_path = folder.hab().join(format!("{name}.hab"));
        if !hab_path.is_file() {
            if let Some(hab) = try_get(&format!("{base_url}.hab")).await {
                if habbo_nitro::hab::is_hab(&hab) {
                    let name = name.to_string();
                    let saved = tokio::task::spawn_blocking(move || save_hab(&hab_path, &name, &hab)).await?;
                    if saved {
                        counts.hab.fetch_add(1, Ordering::SeqCst);
                    } else {
                        counts.failed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn summary(what: &str, counts: &Counts) -> String {
    let failed = counts.failed.load(Ordering::SeqCst);
    format!(
        "Downloaded {} new .swf and {} new .hab {what}{}",
        counts.swf.load(Ordering::SeqCst),
        counts.hab.load(Ordering::SeqCst),
        if failed > 0 { format!("; {failed} .hab files could not be saved.") } else { ".".to_string() }
    )
}

/// The Habbo release folder ("flash-assets-PRODUCTION-...") from external_variables.
pub async fn get_release(external_variables_url: &str) -> Result<Option<String>, common::FetchError> {
    let source = common::get_string(http(), external_variables_url).await?;
    Ok(common::release_trim_end(&source))
}

/// Saves a .hab with a WebP sheet (or as downloaded for spritesheet_format=png).
pub fn save_hab(hab_path: &Path, name: &str, hab: &[u8]) -> bool {
    let result: Result<String> = (|| {
        let (output, result) = if settings::use_webp() {
            habbo_nitro::sheet_webp::hab_to_webp(hab)?
        } else {
            (hab.to_vec(), "as downloaded (spritesheet_format=png)".to_string())
        };
        save(hab_path, &output)?;
        Ok(result)
    })();
    match result {
        Ok(result) => {
            log(Color::Green, &format!("Downloaded: {name}.hab ({result})"));
            true
        }
        Err(error) => {
            log(Color::Red, &format!("Error saving {name}.hab: {error}"));
            false
        }
    }
}

/// A download from before the swf/ and hab/ folders: .swf files move into swf/, .hab files are
/// converted into hab/ like a new download, so nothing has to be downloaded again.
fn move_old_layout(folder: HabboAssetFolder) -> Result<()> {
    let root = folder.root();
    if !root.is_dir() {
        return Ok(());
    }

    for swf in paths::files_with_ext(&root, "swf") {
        std::fs::create_dir_all(folder.swf())?;
        let target = folder.swf().join(paths::file_name(&swf));
        if target.is_file() {
            std::fs::remove_file(&swf)?;
        } else {
            std::fs::rename(&swf, &target)?;
        }
    }

    let habs = paths::files_with_ext(&root, "hab");
    if habs.is_empty() || !settings::download_hab() {
        return Ok(());
    }

    outln!("Moving {} .hab files into {}...", habs.len(), folder.hab().display());
    std::fs::create_dir_all(folder.hab())?;

    // Each file is decoded, WebP-encoded and checked: half the cores keeps the machine usable.
    let threads = std::cmp::max(1, std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) / 2);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    pool.install(|| {
        habs.par_iter().try_for_each(|hab_file| -> Result<()> {
            let name = paths::file_stem(hab_file);
            let target = folder.hab().join(format!("{name}.hab"));
            if !target.is_file() {
                let hab = std::fs::read(hab_file)?;
                if !habbo_nitro::hab::is_hab(&hab) || !save_hab(&target, &name, &hab) {
                    return Ok(());
                }
            }
            std::fs::remove_file(hab_file)?;
            Ok(())
        })
    })
}

/// The body of a successful response, or None (missing asset, network error).
pub async fn try_get(url: &str) -> Option<Vec<u8>> {
    let response = common::send(http(), url).await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    common::body(response).await.ok()
}

/// Writes next to the target first, so a stopped run never leaves half a file that counts as downloaded.
pub fn save(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".part");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, path)
}

/// An error page from the CDN must never be saved as an asset.
pub fn is_swf(data: &[u8]) -> bool {
    data.len() > 3 && matches!(data[0], b'F' | b'C' | b'Z') && data[1] == b'W' && data[2] == b'S'
}

pub fn is_png(data: &[u8]) -> bool {
    data.len() > 8 && data[0] == 0x89 && data[1] == b'P' && data[2] == b'N' && data[3] == b'G'
}

/// `DownloadLibrariesAsync` over a list, keeping the first spelling of names that differ only in case.
pub fn distinct_ignore_case<I: IntoIterator<Item = String>>(items: I) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|item| seen.insert(item.to_lowercase())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_checks() {
        assert!(is_swf(b"FWS\x0a"));
        assert!(is_swf(b"CWS\x0a"));
        assert!(!is_swf(b"<html"));
        assert!(!is_swf(b"FWS"));
        assert!(is_png(b"\x89PNG\r\n\x1a\n\0"));
        assert!(!is_png(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn summary_text() {
        let counts = Counts::default();
        counts.swf.store(2, Ordering::SeqCst);
        assert_eq!(summary("pet files", &counts), "Downloaded 2 new .swf and 0 new .hab pet files.");
        counts.failed.store(1, Ordering::SeqCst);
        assert_eq!(summary("x", &counts), "Downloaded 2 new .swf and 0 new .hab x; 1 .hab files could not be saved.");
    }

    #[test]
    fn distinct_keeps_first_spelling() {
        let list = distinct_ignore_case(["Cat".to_string(), "dog".into(), "cat".into()]);
        assert_eq!(list, ["Cat", "dog"]);
    }

    #[test]
    fn save_writes_through_part_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.swf");
        save(&path, b"x").unwrap();
        save(&path, b"y").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"y");
        assert!(!dir.path().join("a.swf.part").exists());
    }
}
