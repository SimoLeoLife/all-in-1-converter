//! Self-update from GitHub releases. Releases of the Rust build are tagged `rust-v<version>` and
//! carry `habbo-downloader-windows-x64.exe` / `habbo-downloader-linux-x64`. config.ini
//! `update_repo` picks the repository, `update_check=false` turns the startup check off.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use habbo_core::{outln, settings};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_REPO: &str = "SimoLeoLife/all-in-1-converter";
const TAG_PREFIX: &str = "rust-v";

#[cfg(windows)]
const ASSET_NAME: &str = "habbo-downloader-windows-x64.exe";
#[cfg(not(windows))]
const ASSET_NAME: &str = "habbo-downloader-linux-x64";

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub page_url: String,
    pub asset_url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn check_enabled() -> bool {
    !matches!(settings::read_setting("update_check").map(|v| v.to_lowercase()).as_deref(), Some("false" | "0" | "no"))
}

fn repo() -> String {
    settings::read_setting("update_repo").filter(|r| r.contains('/')).unwrap_or_else(|| DEFAULT_REPO.to_string())
}

/// The newest published Rust release when it is newer than this build.
pub async fn check() -> Result<Option<Release>> {
    Ok(latest().await?.filter(|release| is_newer(&release.version)))
}

/// True when `version` is newer than this build.
pub fn is_newer(version: &str) -> bool {
    matches!((parse_version(version), parse_version(CURRENT_VERSION)), (Some(other), Some(current)) if other > current)
}

/// The newest published Rust release, newer than this build or not. Uses the GitHub API; when
/// that refuses (60 anonymous calls an hour per IP), the releases Atom feed instead.
pub async fn latest() -> Result<Option<Release>> {
    let repo = repo();
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=30");
    let response = habbo_core::http::client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("Could not reach GitHub")?;
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Ok(newest(releases_from_feed(&repo).await?));
    }
    if !status.is_success() {
        bail!("GitHub answered {status} for {url}");
    }
    let releases: Vec<GithubRelease> = response.json().await.context("Unexpected answer from GitHub")?;
    Ok(newest(releases))
}

/// The repository's page on GitHub.
pub fn repo_url() -> String {
    format!("https://github.com/{}", repo())
}

/// Releases read from `https://github.com/<repo>/releases.atom` (no API limit). The feed has no
/// asset list, so the download link is the standard one for our asset name; `install` refuses
/// a missing or too small file.
async fn releases_from_feed(repo: &str) -> Result<Vec<GithubRelease>> {
    let feed = habbo_core::http::get_string(&format!("https://github.com/{repo}/releases.atom")).await.context("Could not read the releases feed")?;
    Ok(parse_feed(repo, &feed))
}

fn parse_feed(repo: &str, feed: &str) -> Vec<GithubRelease> {
    let marker = "/releases/tag/";
    let mut tags: Vec<String> = Vec::new();
    for (index, _) in feed.match_indices(marker) {
        let rest = &feed[index + marker.len()..];
        let tag: String = rest.chars().take_while(|c| !matches!(c, '"' | '<' | '&' | ' ')).collect();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags.into_iter()
        .map(|tag| GithubRelease {
            html_url: format!("https://github.com/{repo}/releases/tag/{tag}"),
            assets: vec![GithubAsset {
                name: ASSET_NAME.to_string(),
                browser_download_url: format!("https://github.com/{repo}/releases/download/{tag}/{ASSET_NAME}"),
            }],
            tag_name: tag,
            body: None,
            draft: false,
            prerelease: false,
        })
        .collect()
}

fn newest(releases: Vec<GithubRelease>) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = parse_version(r.tag_name.strip_prefix(TAG_PREFIX)?)?;
            let asset = r.assets.iter().find(|a| a.name == ASSET_NAME)?;
            Some((
                version,
                Release {
                    version: r.tag_name.trim_start_matches(TAG_PREFIX).to_string(),
                    page_url: r.html_url.clone(),
                    asset_url: asset.browser_download_url.clone(),
                    notes: r.body.clone().unwrap_or_default(),
                },
            ))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, release)| release)
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Downloads the release and swaps it in for the running exe (kept as `.old` until the next
/// start). Returns the path to start.
pub async fn install(release: &Release) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("The program path is unknown")?;
    let bytes = habbo_core::http::get_bytes(&release.asset_url).await.context("Download failed")?;
    if bytes.len() < 1024 * 1024 {
        bail!("The downloaded file is too small ({} bytes); not installing it", bytes.len());
    }
    let new = exe.with_extension("new");
    let old = exe.with_extension("old");
    std::fs::write(&new, &bytes).with_context(|| format!("Could not write {}", new.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = std::fs::remove_file(&old);
    // A running exe can be renamed (not overwritten) on Windows.
    std::fs::rename(&exe, &old).with_context(|| format!("Could not move {} aside", exe.display()))?;
    if let Err(error) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(error).with_context(|| format!("Could not put the new version in place of {}", exe.display()));
    }
    Ok(exe)
}

/// Removes the previous version left by an update.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

/// Starts the (updated) program again with these arguments.
pub fn restart(exe: &std::path::Path, args: &[&str]) -> std::io::Result<()> {
    std::process::Command::new(exe).args(args).current_dir(std::env::current_dir().unwrap_or_default()).spawn().map(|_| ())
}

/// The "Check for updates" operation (CLI menu and Professional window).
pub async fn check_and_install() -> Result<()> {
    outln!("Current version: {CURRENT_VERSION}");
    outln!("Checking {} for a newer release...", repo());
    let Some(release) = check().await? else {
        outln!("✅ You have the latest version.");
        return Ok(());
    };
    outln!("🆕 Version {} is available: {}", release.version, release.page_url);
    if !release.notes.trim().is_empty() {
        outln!();
        outln!("{}", release.notes.trim());
        outln!();
    }
    let answer = habbo_core::console::prompt("Install it now? (Y/N): ");
    if !answer.eq_ignore_ascii_case("y") {
        outln!("Not installed.");
        return Ok(());
    }
    let exe = install(&release).await?;
    outln!("✅ Installed {}. Restart the converter to use it ({}).", release.version, exe.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, asset: bool, prerelease: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            html_url: format!("https://example.test/{tag}"),
            body: None,
            draft: false,
            prerelease,
            assets: if asset {
                vec![GithubAsset { name: ASSET_NAME.into(), browser_download_url: format!("https://example.test/{tag}/{ASSET_NAME}") }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn picks_the_newest_rust_release_with_our_asset() {
        let releases = vec![
            release("rust-v1.0.1", true, false),
            release("rust-v1.2.0", true, false),
            release("rust-v2.0.0", false, false), // no binary for this OS
            release("rust-v3.0.0", true, true),   // prerelease
            release("v9.0.0", true, false),       // the .NET build
        ];
        assert_eq!(newest(releases).map(|r| r.version).as_deref(), Some("1.2.0"));
        // The latest release is reported also when it is this build; `is_newer` tells them apart.
        assert_eq!(newest(vec![release("rust-v1.0.0", true, false)]).map(|r| r.version).as_deref(), Some("1.0.0"));
        assert!(is_newer("99.0.0"));
        assert!(!is_newer(CURRENT_VERSION));
        assert!(!is_newer("0.1.0"));
        assert_eq!(parse_version("v1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
    }

    #[test]
    fn feed_fallback_finds_rust_tags() {
        let feed = r#"<entry><link rel="alternate" type="text/html" href="https://github.com/o/r/releases/tag/v2026.10.20.32"/></entry>
<entry><link rel="alternate" type="text/html" href="https://github.com/o/r/releases/tag/rust-v1.3.0"/></entry>"#;
        let release = newest(parse_feed("o/r", feed)).unwrap();
        assert_eq!(release.version, "1.3.0");
        assert_eq!(release.asset_url, format!("https://github.com/o/r/releases/download/rust-v1.3.0/{ASSET_NAME}"));
    }
}
