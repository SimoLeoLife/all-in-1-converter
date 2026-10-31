//! `Badges` (Badge.cs): every hotel's badges into Habbo_Default/badges.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use futures::StreamExt;
use habbo_core::{console, outln, paths, Color};

use crate::common::{self, colored, FetchError};
use crate::resume;

const BADGES_DIRECTORY: &str = "./Habbo_Default/badges";
const TEMP_DIRECTORY: &str = "./temp";
const MAX_PARALLEL_DOWNLOADS: usize = 32;
const DOMAINS: [&str; 9] = ["com", "fr", "fi", "es", "nl", "de", "it", "com.tr", "com.br"];

/// `Badges.DownloadBadgesAsync`.
pub async fn download_badges() -> anyhow::Result<()> {
    std::fs::create_dir_all(BADGES_DIRECTORY)?;
    std::fs::create_dir_all(TEMP_DIRECTORY)?;

    let initial_badge_count = paths::files_recursive(Path::new(BADGES_DIRECTORY)).len();

    // The hotels share most badges: collect the names once, then download each badge once.
    let badge_names: Mutex<(HashSet<String>, Vec<String>)> = Mutex::new((HashSet::new(), Vec::new()));
    let reads = DOMAINS.iter().map(|domain| {
        let badge_names = &badge_names;
        async move {
            outln!("Start initializing badges from .{}", domain.to_uppercase());
            let names = read_badge_names(domain).await?;
            {
                let mut guard = badge_names.lock().unwrap();
                let (set, order) = &mut *guard;
                for name in names {
                    if set.insert(name.clone()) {
                        order.push(name);
                    }
                }
            }
            outln!("Finished reading badges from .{}", domain.to_uppercase());
            anyhow::Ok(())
        }
    });
    for result in futures::future::join_all(reads).await {
        result?;
    }

    let (_, names) = badge_names.into_inner().unwrap();
    outln!(
        "{} different badges in {} hotels; downloading the missing ones...",
        console::n0(names.len() as i64),
        DOMAINS.len()
    );
    futures::stream::iter(names)
        .for_each_concurrent(MAX_PARALLEL_DOWNLOADS, |name| async move { download_badge(&name).await })
        .await;

    let final_badge_count = paths::files_recursive(Path::new(BADGES_DIRECTORY)).len();
    colored(
        Color::Green,
        &format!("\nDownloading done! We downloaded {} badges!", final_badge_count as i64 - initial_badge_count as i64),
    );
    Ok(())
}

/// The badge names in one hotel's external_flash_texts (empty when it cannot be downloaded).
async fn read_badge_names(domain: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("https://www.habbo.{domain}/gamedata/external_flash_texts/1");
    let file_path = Path::new(TEMP_DIRECTORY).join(format!("external_flash_texts_{domain}.txt"));

    let client = common::default_client();
    match resume::fetch(|| client.get(&url), &file_path, &resume::Options::FRESH).await {
        Ok(resume::Fetched::Done(part)) => part.commit()?,
        Ok(resume::Fetched::Status(status)) => {
            colored(
                Color::Red,
                &format!("Failed to download external flash texts for {domain}. Status code: {}", common::status_name(status)),
            );
            return Ok(Vec::new());
        }
        Err(FetchError::Http(message)) => {
            colored(Color::Red, &format!("HTTP request failed for {domain}: {message}"));
            return Ok(Vec::new());
        }
        Err(FetchError::Other(error)) => return Err(error),
    }

    let lines = common::read_lines(&file_path)?;
    Ok(parse_badge_names(lines.iter().map(String::as_str)).collect())
}

/// The badge codes of "badge_name_CODE=..." lines; _HHCA / _HHUK variants are skipped unless fb_ or al_.
pub fn parse_badge_names<'a>(lines: impl Iterator<Item = &'a str> + 'a) -> impl Iterator<Item = String> + 'a {
    lines.filter_map(|line| {
        if !line.starts_with("badge_name_") {
            return None;
        }
        let (key, _) = line.split_once('=')?;
        let badge_name = key.replace("badge_name_", "");
        if badge_name.starts_with("fb_")
            || badge_name.starts_with("al_")
            || (!badge_name.contains("_HHCA") && !badge_name.contains("_HHUK"))
        {
            Some(badge_name)
        } else {
            None
        }
    })
}

async fn download_badge(badge_name: &str) {
    let url = format!("http://images-eussl.habbo.com/c_images/album1584/{badge_name}.gif");
    let file_path = Path::new(BADGES_DIRECTORY).join(format!("{badge_name}.gif"));
    if file_path.is_file() {
        return;
    }

    let result: Result<bool, FetchError> = async {
        // Streamed into a .part first (resumed by the next run), so a failed download never leaves half a .gif.
        let client = common::default_client();
        match resume::fetch(|| client.get(&url), &file_path, &resume::Options::default()).await? {
            resume::Fetched::Done(part) => {
                part.commit()?;
                Ok(true)
            }
            resume::Fetched::Status(_) => Ok(false),
        }
    }
    .await;

    match result {
        Ok(true) => colored(Color::Green, &format!("Downloading badge: {badge_name}.gif")),
        Ok(false) => {}
        Err(error) => colored(Color::Red, &format!("HTTP request failed for badge {badge_name}.gif: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_names_filter_hotel_variants() {
        let text = "badge_name_ACH_1=A\nbadge_desc_X=y\nbadge_name_X_HHCA=no\nbadge_name_fb_X_HHUK=yes\nbadge_name_al_Y_HHCA=yes\nbadge_name_NOEQ\nbadge_name_Z_HHUK=no";
        let names: Vec<String> = parse_badge_names(text.lines()).collect();
        assert_eq!(names, ["ACH_1", "fb_X_HHUK", "al_Y_HHCA"]);
    }
}
