//! Resumable downloads, shared by every downloader (a deliberate improvement over the C# code).
//!
//! A file is always streamed into `<file>.part` first and only becomes `<file>` once it is
//! complete, so a stopped run never leaves half a file that counts as downloaded. When a `.part`
//! is already there, the request asks for the rest (`Range: bytes=<len>-`):
//!
//! * `206 Partial Content` starting at `<len>`: the body is appended ("↻ Resuming ..." is printed);
//! * `200 OK` (the server ignored the range): the `.part` is written again from zero;
//! * `416 Range Not Satisfiable`: the `.part` is complete when the `Content-Range` total equals its
//!   length, otherwise it is deleted and the file downloaded again from zero.
//!
//! Range requests are sent with `Accept-Encoding: identity`, so the offsets count the same bytes
//! the `.part` holds (a first download that came gzip-encoded was stored decoded).
//!
//! Only one download per `.part` path runs at a time in this process (a per-path lock held until
//! the [`PartFile`] is committed or dropped), so two tasks can never write the same `.part`.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use habbo_core::Color;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;

use crate::common::{self, FetchError};

/// `<target>.part`.
pub fn part_path(target: &Path) -> PathBuf {
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

/// The `Range` header value that asks for everything after `existing` bytes (`None` for 0).
pub fn range_header(existing: u64) -> Option<String> {
    (existing > 0).then(|| format!("bytes={existing}-"))
}

/// A parsed `Content-Range: bytes <start>-<end>/<total>` (or `bytes */<total>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub total: Option<u64>,
}

pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim();
    let unit = rest.get(..5)?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let rest = rest[5..].trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse().ok()?),
    };
    let (start, end) = match range.trim() {
        "*" => (None, None),
        range => {
            let (start, end) = range.split_once('-')?;
            (Some(start.trim().parse().ok()?), Some(end.trim().parse().ok()?))
        }
    };
    Some(ContentRange { start, end, total })
}

/// What to do with a response, given how many bytes the `.part` already had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Write the body into the `.part` from zero.
    Overwrite,
    /// Append the body to the `.part`.
    Append,
    /// The `.part` already holds the whole file.
    Complete,
    /// Delete the `.part` and request the whole file again (without a range).
    Restart,
    /// A non-success status: report it, leave the `.part` alone.
    Fail,
}

pub fn decide(status: u16, existing: u64, content_range: Option<&str>) -> Decision {
    let success = (200..300).contains(&status);
    if existing == 0 {
        return if success { Decision::Overwrite } else { Decision::Fail };
    }
    let range = content_range.and_then(parse_content_range);
    match status {
        206 => match range {
            Some(ContentRange { start: Some(start), .. }) if start == existing => Decision::Append,
            _ => Decision::Restart,
        },
        416 => match range.and_then(|r| r.total) {
            Some(total) if total == existing => Decision::Complete,
            _ => Decision::Restart,
        },
        _ if success => Decision::Overwrite,
        _ => Decision::Fail,
    }
}

/// Per-download options.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// The client's timeout, for the .NET timeout message (`None` = the default 100 s).
    pub timeout: Option<Duration>,
    /// Create the target's folder once the response is known to be a success (the C# code did
    /// `Directory.CreateDirectory` between the request and the write).
    pub create_dir: bool,
    /// Resume a `.part` left by an earlier run (the default). `false` for gamedata/text files that
    /// can change between runs: an old `.part` is deleted and the file downloaded from zero, so
    /// two versions are never joined.
    pub resume: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { timeout: None, create_dir: false, resume: true }
    }
}

impl Options {
    /// Default options, without resume.
    pub const FRESH: Options = Options { timeout: None, create_dir: false, resume: false };

    /// The same options, without resume.
    pub fn fresh(self) -> Self {
        Options { resume: false, ..self }
    }
}

/// The length a download starts from: the `.part`'s, or 0 after deleting it when `resume` is off.
pub fn prepare_part(part: &Path, options: &Options) -> std::io::Result<u64> {
    if !options.resume {
        remove_if_exists(part)?;
    }
    Ok(part_len(part))
}

/// The result of [`fetch`].
pub enum Fetched {
    /// The `.part` holds the complete file.
    Done(PartFile),
    /// The server answered with this non-success status (nothing was written).
    Status(StatusCode),
}

/// A complete download still sitting in `<target>.part`.
pub struct PartFile {
    target: PathBuf,
    part: PathBuf,
    _lock: PathLock,
}

impl PartFile {
    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn part(&self) -> &Path {
        &self.part
    }

    /// The downloaded bytes.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.part)
    }

    /// Renames the `.part` to the target (replacing it).
    pub fn commit(self) -> std::io::Result<()> {
        std::fs::rename(&self.part, &self.target)
    }

    /// Renames the `.part` to another name (replacing it).
    pub fn commit_to(self, path: &Path) -> std::io::Result<()> {
        std::fs::rename(&self.part, path)
    }

    /// Writes the bytes through `habbo_core::workspace::write_output` (the Asset Workspace
    /// transaction when the target is inside it), then deletes the `.part`.
    pub fn commit_to_workspace(self) -> anyhow::Result<()> {
        let content = self.read()?;
        habbo_core::workspace::write_output(&self.target, &content)?;
        remove_if_exists(&self.part)?;
        Ok(())
    }

    /// Deletes the `.part` (an invalid download, so the next run does not resume it).
    pub fn discard(self) -> std::io::Result<()> {
        remove_if_exists(&self.part)
    }
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Deletes the `.part` of each target (downloads started ahead that turned out not to be wanted,
/// e.g. past the "20 in a row" stop of the icons and mp3 downloaders).
pub fn discard_parts(targets: impl IntoIterator<Item = PathBuf>) {
    for target in targets {
        let _ = remove_if_exists(&part_path(&target));
    }
}

type Locks = Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>;

fn locks() -> &'static Locks {
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

fn lock_key(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows) {
        PathBuf::from(absolute.to_string_lossy().to_lowercase())
    } else {
        absolute
    }
}

/// Exclusive use of one `.part` path in this process.
struct PathLock {
    key: PathBuf,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl PathLock {
    async fn acquire(path: &Path) -> Self {
        let key = lock_key(path);
        let mutex = locks().lock().unwrap_or_else(|e| e.into_inner()).entry(key.clone()).or_default().clone();
        let guard = mutex.lock_owned().await;
        PathLock { key, guard: Some(guard) }
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        let mut map = locks().lock().unwrap_or_else(|e| e.into_inner());
        drop(self.guard.take());
        // Waiters clone the Arc under the map lock, so a count of 1 means nobody else wants it.
        if map.get(&self.key).is_some_and(|m| Arc::strong_count(m) == 1) {
            map.remove(&self.key);
        }
    }
}

fn part_len(part: &Path) -> u64 {
    std::fs::metadata(part).ok().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0)
}

/// Downloads `request()` into `<target>.part`, resuming a `.part` left by an earlier run.
/// `request` builds the GET (it is called again when the download has to restart).
pub async fn fetch(
    request: impl Fn() -> reqwest::RequestBuilder,
    target: &Path,
    options: &Options,
) -> Result<Fetched, FetchError> {
    let part = part_path(target);
    let lock = PathLock::acquire(&part).await;
    prepare_part(&part, options)?;
    loop {
        let existing = part_len(&part);
        let mut builder = request();
        if let Some(range) = range_header(existing) {
            builder = builder.header(RANGE, range).header(ACCEPT_ENCODING, "identity");
        }
        let response = common::send_with(builder, options.timeout).await?;
        let status = response.status();
        let content_range = response.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
        match decide(status.as_u16(), existing, content_range.as_deref()) {
            Decision::Fail => return Ok(Fetched::Status(status)),
            Decision::Complete => break,
            // existing becomes 0, and decide never restarts from 0: this loops at most twice.
            Decision::Restart => remove_if_exists(&part)?,
            Decision::Overwrite => {
                write_body(response, &part, false, options).await?;
                break;
            }
            Decision::Append => {
                let name = habbo_core::paths::file_name(target);
                common::log(Color::Cyan, &format!("↻ Resuming {name} from {existing} bytes"));
                write_body(response, &part, true, options).await?;
                break;
            }
        }
    }
    Ok(Fetched::Done(PartFile { target: target.to_path_buf(), part, _lock: lock }))
}

/// Streams the body into the `.part`; what arrived before an error stays there for the next try.
async fn write_body(mut response: reqwest::Response, part: &Path, append: bool, options: &Options) -> Result<(), FetchError> {
    if options.create_dir {
        if let Some(directory) = part.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(directory)?;
        }
    }
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part)?;
    while let Some(chunk) = response.chunk().await.map_err(|e| common::map_reqwest(e, options.timeout))? {
        file.write_all(&chunk)?;
    }
    file.flush()?;
    Ok(())
}

/// [`fetch`] with `EnsureSuccessStatusCode`: a non-success status is the .NET `HttpRequestException`.
pub async fn fetch_ok(
    request: impl Fn() -> reqwest::RequestBuilder,
    target: &Path,
    options: &Options,
) -> Result<PartFile, FetchError> {
    match fetch(request, target, options).await? {
        Fetched::Done(part) => Ok(part),
        Fetched::Status(status) => Err(FetchError::Http(common::status_message(status))),
    }
}

/// `GetByteArrayAsync` + `File.WriteAllBytes`, through the `.part`.
pub async fn download_to(client: &reqwest::Client, url: &str, target: &Path, options: &Options) -> Result<(), FetchError> {
    fetch_ok(|| client.get(url), target, options).await?.commit()?;
    Ok(())
}

/// `GetByteArrayAsync` + `WorkspaceOutput.WriteAllBytesAsync`, through the `.part`.
pub async fn download_to_workspace(client: &reqwest::Client, url: &str, target: &Path, options: &Options) -> Result<(), FetchError> {
    fetch_ok(|| client.get(url), target, options).await?.commit_to_workspace()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_headers() {
        assert_eq!(range_header(0), None);
        assert_eq!(range_header(1), Some("bytes=1-".into()));
        assert_eq!(range_header(123456), Some("bytes=123456-".into()));
        assert_eq!(part_path(Path::new("a/b.swf")), PathBuf::from("a/b.swf.part"));
    }

    #[test]
    fn content_ranges() {
        assert_eq!(
            parse_content_range("bytes 100-199/200"),
            Some(ContentRange { start: Some(100), end: Some(199), total: Some(200) })
        );
        assert_eq!(parse_content_range("bytes */200"), Some(ContentRange { start: None, end: None, total: Some(200) }));
        assert_eq!(parse_content_range("Bytes 0-9/*"), Some(ContentRange { start: Some(0), end: Some(9), total: None }));
        assert_eq!(parse_content_range("bytes 1-x/5"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert_eq!(parse_content_range("bytes 0-1"), None);
        assert_eq!(parse_content_range(""), None);
    }

    #[test]
    fn decisions_without_part() {
        assert_eq!(decide(200, 0, None), Decision::Overwrite);
        assert_eq!(decide(206, 0, Some("bytes 0-9/10")), Decision::Overwrite);
        assert_eq!(decide(404, 0, None), Decision::Fail);
        assert_eq!(decide(416, 0, Some("bytes */0")), Decision::Fail);
        assert_eq!(decide(500, 0, None), Decision::Fail);
    }

    #[test]
    fn decisions_with_part() {
        assert_eq!(decide(206, 100, Some("bytes 100-199/200")), Decision::Append);
        assert_eq!(decide(206, 100, Some("bytes 0-199/200")), Decision::Restart, "range from the wrong offset");
        assert_eq!(decide(206, 100, None), Decision::Restart, "206 without Content-Range");
        assert_eq!(decide(200, 100, None), Decision::Overwrite, "range ignored");
        assert_eq!(decide(416, 200, Some("bytes */200")), Decision::Complete);
        assert_eq!(decide(416, 150, Some("bytes */200")), Decision::Restart, "file changed on the server");
        assert_eq!(decide(416, 300, Some("bytes */200")), Decision::Restart);
        assert_eq!(decide(416, 200, None), Decision::Restart, "416 without a total");
        assert_eq!(decide(404, 100, None), Decision::Fail);
        assert_eq!(decide(503, 100, None), Decision::Fail);
    }

    #[test]
    fn resume_flag() {
        assert!(Options::default().resume);
        let fresh = Options::FRESH;
        assert!(!fresh.resume);
        let fresh = Options { create_dir: true, ..Default::default() }.fresh();
        assert!(fresh.create_dir && !fresh.resume);

        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("a.txt.part");
        assert_eq!(prepare_part(&part, &Options::default()).unwrap(), 0, "no .part yet");
        assert_eq!(prepare_part(&part, &Options::FRESH).unwrap(), 0, "nothing to delete");
        std::fs::write(&part, b"12345").unwrap();
        assert_eq!(prepare_part(&part, &Options::default()).unwrap(), 5, "resumed from its length");
        assert!(part.exists());
        assert_eq!(prepare_part(&part, &Options::FRESH).unwrap(), 0, "deleted, from zero");
        assert!(!part.exists());
    }

    #[test]
    fn part_file_commit_and_discard() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("x.bin");
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        runtime.block_on(async {
            std::fs::write(part_path(&target), b"abc").unwrap();
            let part = PartFile { target: target.clone(), part: part_path(&target), _lock: PathLock::acquire(&part_path(&target)).await };
            assert_eq!(part.read().unwrap(), b"abc");
            part.commit().unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), b"abc");
            assert!(!part_path(&target).exists());

            std::fs::write(part_path(&target), b"bad").unwrap();
            let part = PartFile { target: target.clone(), part: part_path(&target), _lock: PathLock::acquire(&part_path(&target)).await };
            part.discard().unwrap();
            assert!(!part_path(&target).exists());
            assert!(locks().lock().unwrap().is_empty(), "locks are released");
        });
    }

    /// A real resume against Habbo's CDN: download one file, cut its .part in half, download again.
    /// `cargo test -p habbo-downloads -- --ignored resume_real_download --nocapture`
    #[test]
    #[ignore = "needs the network"]
    fn resume_real_download() {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("hh_human_body.swf");
            let release = crate::asset_downloader::get_release("https://www.habbo.com/gamedata/external_variables/1")
                .await
                .expect("external_variables")
                .expect("release");
            let url = &format!("https://images.habbo.com/gordon/{release}/hh_human_body.swf");
            println!("url: {url}");
            let client = common::default_client();

            // A first, whole download (no .part yet).
            let fetched = fetch_ok(|| client.get(url), &target, &Options::default()).await;
            let full = match fetched {
                Ok(part) => {
                    let bytes = part.read().unwrap();
                    part.commit().unwrap();
                    bytes
                }
                Err(error) => panic!("first download failed: {error}"),
            };
            println!("first download: {} bytes", full.len());
            assert!(full.len() > 1000);

            // An interrupted run: half of the file in the .part.
            std::fs::remove_file(&target).unwrap();
            std::fs::write(part_path(&target), &full[..full.len() / 2]).unwrap();
            download_to(client, url, &target, &Options::default()).await.unwrap();
            let resumed = std::fs::read(&target).unwrap();
            assert_eq!(resumed.len(), full.len());
            assert!(resumed == full, "resumed file differs");
            assert!(!part_path(&target).exists());

            // A .part that is already complete (416 or a full 200).
            let probe = client.get(url.as_str()).header(RANGE, range_header(full.len() as u64).unwrap()).send().await.unwrap();
            println!(
                "range past the end: {} (Content-Range {:?}) -> {:?}",
                probe.status(),
                probe.headers().get(CONTENT_RANGE),
                decide(probe.status().as_u16(), full.len() as u64, probe.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()))
            );
            std::fs::rename(&target, part_path(&target)).unwrap();
            download_to(client, url, &target, &Options::default()).await.unwrap();
            assert!(std::fs::read(&target).unwrap() == full);
            println!("resumed and complete-.part downloads match ({} bytes)", full.len());
        });
    }
}
