//! The shared HTTP client. The C# downloaders each kept a static `HttpClient` with the
//! browser User-Agent below; here one client is shared by everything.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

/// The shared async client (User-Agent set, gzip on, 100 s timeout like HttpClient's default).
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(100))
            .pool_max_idle_per_host(64)
            .build()
            .expect("HTTP client")
    })
}

/// `HttpClient.GetStringAsync`: fails on a non-success status.
pub async fn get_string(url: &str) -> Result<String> {
    let response = client().get(url).send().await.with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Response status code does not indicate success: {} ({}). URL: {url}", status.as_u16(), status.canonical_reason().unwrap_or(""));
    }
    Ok(response.text().await?)
}

/// `HttpClient.GetByteArrayAsync`: fails on a non-success status.
pub async fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let response = client().get(url).send().await.with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Response status code does not indicate success: {} ({}). URL: {url}", status.as_u16(), status.canonical_reason().unwrap_or(""));
    }
    Ok(response.bytes().await?.to_vec())
}

/// GET that returns the status instead of failing on it: `Ok(None)` for a non-success status
/// (the C# code mostly checked `IsSuccessStatusCode` and skipped 404s).
pub async fn try_get_bytes(url: &str) -> Result<Option<Vec<u8>>> {
    let response = client().get(url).send().await.with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    Ok(Some(response.bytes().await?.to_vec()))
}
