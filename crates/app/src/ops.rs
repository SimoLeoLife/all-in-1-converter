//! Operations that live in the app itself: the download-all sequence and the version check.

use anyhow::Result;
use habbo_core::console::{reset_color, set_color, Color};
use habbo_core::outln;

use crate::catalog;

/// `DownloadAllAsync`: clothes, furnidata, productdata, furniture, pets, variables, texts, icons.
pub async fn download_all() -> Result<()> {
    for id in [
        "habbo.clothes",
        "habbo.furnidata",
        "habbo.productdata",
        "habbo.furniture",
        "habbo.pets",
        "habbo.variables",
        "habbo.texts",
        "habbo.icons",
    ] {
        catalog::run(id).await?;
    }
    Ok(())
}

/// `CliRunner.DisplayVersionAsync`.
pub async fn display_version() -> Result<()> {
    outln!("Fetching the current Habbo version...");
    let version = habbo_version().await;
    set_color(Color::Green);
    outln!("Current Habbo Version: {version}");
    reset_color();
    Ok(())
}

/// The current Habbo client build, for the About page.
pub async fn fetch_habbo_version() -> Result<String> {
    let source = habbo_core::http::get_string("https://www.habbo.com/gamedata/external_variables/1").await?;
    parse_release(&source).ok_or_else(|| anyhow::anyhow!("flash.client.url is not in the external variables"))
}

async fn habbo_version() -> String {
    match habbo_core::http::get_string("https://www.habbo.com/gamedata/external_variables/1").await {
        Ok(source) => parse_release(&source).unwrap_or_else(|| "Unknown".into()),
        Err(error) => {
            set_color(Color::Red);
            outln!("Error fetching Habbo version: {error}");
            reset_color();
            "Unknown".into()
        }
    }
}

/// The release folder in `flash.client.url=.../gordon/<RELEASE>/`. C# dropped the last
/// character of the line (the trailing `/` or `\r`) and took path segment 4.
fn parse_release(source: &str) -> Option<String> {
    let line = source.split(['\r', '\n']).find(|line| line.contains("flash.client.url="))?;
    let trimmed = &line[..line.len().saturating_sub(1)];
    trimmed.split('/').nth(4).map(str::to_string)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_from_external_variables() {
        let text = "a=1\nflash.client.url=//images.habbo.com/gordon/flash-assets-PRODUCTION-202601011234-123/\nb=2";
        assert_eq!(parse_release(text).as_deref(), Some("flash-assets-PRODUCTION-202601011234-123"));
        assert_eq!(parse_release("x=1"), None);
    }
}
