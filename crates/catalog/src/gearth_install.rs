//! G-Earth on demand: when the catalogue rip finds no G-Earth running, it offers to download the
//! official release (github.com/G-Realm/G-Earth, MIT licence, Java included) into `Tools/G-Earth/`
//! once and to start it. Nothing is downloaded without a "yes".

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use habbo_core::console::{self, Color};
use habbo_core::{outc, outln};

const REPOSITORY: &str = "G-Realm/G-Earth";

#[cfg(windows)]
const ASSET: &str = "G-Earth-windows-x64.zip";
#[cfg(target_os = "linux")]
const ASSET: &str = "G-Earth-linux-x64.tar.gz";
#[cfg(target_os = "macos")]
const ASSET: &str = "G-Earth-mac-x64.zip";

#[cfg(windows)]
const LAUNCHER: &str = "G-Earth.exe";
#[cfg(not(windows))]
const LAUNCHER: &str = "G-Earth";

pub fn install_dir() -> PathBuf {
    Path::new("Tools").join("G-Earth")
}

/// The G-Earth launcher in `Tools/G-Earth`, wherever the archive put it.
pub fn installed_launcher() -> Option<PathBuf> {
    let dir = install_dir();
    let direct = dir.join(LAUNCHER);
    if direct.is_file() {
        return Some(direct);
    }
    // An archive with one top folder.
    std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path().join(LAUNCHER)).find(|p| p.is_file())
}

/// True when something listens on one of G-Earth's extension ports.
pub async fn is_running(ports: &[u16]) -> bool {
    for &port in ports {
        let connect = tokio::net::TcpStream::connect(("127.0.0.1", port));
        if matches!(tokio::time::timeout(Duration::from_millis(400), connect).await, Ok(Ok(_))) {
            return true;
        }
    }
    false
}

/// Asks to start (and first download, when missing) G-Earth. Returns true when it was started.
pub async fn offer_to_start() -> Result<bool> {
    let launcher = match installed_launcher() {
        Some(launcher) => {
            let answer = console::prompt("G-Earth is not running. Start the G-Earth in Tools/G-Earth now? (Y/N): ");
            if !answer.eq_ignore_ascii_case("y") {
                return Ok(false);
            }
            launcher
        }
        None => {
            outln!("G-Earth is not running and is not in Tools/G-Earth yet.");
            let answer = console::prompt(&format!(
                "Download the official G-Earth (about 100 MB, MIT licence, github.com/{REPOSITORY}) into Tools/G-Earth and start it? (Y/N): "
            ));
            if !answer.eq_ignore_ascii_case("y") {
                outln!("Start G-Earth yourself, then run this again.");
                return Ok(false);
            }
            download_and_extract().await?
        }
    };
    start(&launcher)?;
    outc!(Color::Green, "G-Earth started. In G-Earth press Connect, then log into Habbo with your client.");
    Ok(true)
}

async fn download_and_extract() -> Result<PathBuf> {
    // GitHub sends the newest release's asset for this link, without an API call.
    let url = format!("https://github.com/{REPOSITORY}/releases/latest/download/{ASSET}");
    outln!("Downloading {url}");
    let mut response = habbo_core::http::client()
        .get(&url)
        .timeout(Duration::from_secs(30 * 60))
        .send()
        .await
        .context("Could not reach GitHub")?;
    if !response.status().is_success() {
        bail!("GitHub answered {} for {url}", response.status());
    }
    let total = response.content_length().unwrap_or(0);
    let dir = install_dir();
    std::fs::create_dir_all(&dir)?;
    let archive_path = dir.join(format!("{ASSET}.part"));
    let mut file = std::fs::File::create(&archive_path)?;
    let mut done = 0u64;
    let mut last_percent = u64::MAX;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
        done += chunk.len() as u64;
        if total > 0 {
            let percent = done * 100 / total;
            if percent / 5 != last_percent / 5 {
                last_percent = percent;
                outln!("[{:>4}/{} MB] ({percent}%)", done / (1024 * 1024), total / (1024 * 1024));
            }
        }
    }
    drop(file);

    outln!("Unpacking into {} ...", dir.display());
    let archive = archive_path.clone();
    let target = dir.clone();
    tokio::task::spawn_blocking(move || extract(&archive, &target)).await??;
    let _ = std::fs::remove_file(&archive_path);
    installed_launcher().with_context(|| format!("{LAUNCHER} was not found in the G-Earth download ({})", dir.display()))
}

fn extract(archive: &Path, target: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    std::fs::File::open(archive)?.read_to_end(&mut bytes)?;
    if ASSET.ends_with(".zip") {
        zip::ZipArchive::new(std::io::Cursor::new(bytes))?.extract(target)?;
    } else {
        let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
        tar::Archive::new(gz).unpack(target)?;
    }
    Ok(())
}

fn start(launcher: &Path) -> Result<()> {
    let dir = launcher.parent().unwrap_or(Path::new("."));
    let mut command = std::process::Command::new(std::path::absolute(launcher)?);
    command.current_dir(dir);
    command.spawn().with_context(|| format!("Could not start {}", launcher.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_launcher_at_the_top_or_one_folder_down() {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert!(installed_launcher().is_none());
        let nested = install_dir().join("G-Earth-1.5.4");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join(LAUNCHER), b"").unwrap();
        assert_eq!(installed_launcher(), Some(nested.join(LAUNCHER)));
        std::fs::write(install_dir().join(LAUNCHER), b"").unwrap();
        assert_eq!(installed_launcher(), Some(install_dir().join(LAUNCHER)));
        std::env::set_current_dir(previous).unwrap();
    }

    #[test]
    fn unpacks_a_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("g.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("G-Earth.exe", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"MZ").unwrap();
        zip.finish().unwrap();
        if ASSET.ends_with(".zip") {
            extract(&archive, &dir.path().join("out")).unwrap();
            assert!(dir.path().join("out").join("G-Earth.exe").is_file());
        }
    }
}
