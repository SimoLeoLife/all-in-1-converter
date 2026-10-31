//! `QuestsDownloader` (Quests.cs): the quest images named in external_flash_texts
//! ("quests.<name>.<image>...") into Habbo_Default/quests.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::StreamExt;
use habbo_core::{outln, Color};

use crate::common::{self, colored, FetchError};
use crate::resume::{self, PartFile};

const TEMP_FILE_PATH: &str = "./temp/external_texts.txt";

/// The (quest name, image) of a "quests." line: parts 1 and 2 of a split on '.', lower-cased.
pub fn quest_parts(line: &str) -> Option<(String, String)> {
    if !line.starts_with("quests.") {
        return None;
    }
    let parts: Vec<&str> = line.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    Some((parts[1].to_lowercase(), parts[2].to_lowercase()))
}

/// `QuestsDownloader.DownloadQuestsAsync`.
pub async fn download_quests() -> Result<()> {
    let config = common::load_config()?;
    let result = run(&config).await;
    // finally: the temp folder goes, whatever happened.
    let cleanup = common::clean_temp();
    result?;
    cleanup?;
    Ok(())
}

async fn run(config: &std::collections::HashMap<String, String>) -> Result<()> {
    std::fs::create_dir_all("./temp")?;

    let external_text_url = common::cfg(config, "AppSettings:externaltexturl")?;
    if external_text_url.is_empty() {
        colored(Color::Red, "Error: External Text URL is not configured.");
        return Ok(());
    }
    let quests_url = common::cfg(config, "AppSettings:questsurl")?;
    if quests_url.is_empty() {
        colored(Color::Red, "Error: Quests URL is not configured.");
        return Ok(());
    }

    if Path::new(TEMP_FILE_PATH).is_file() {
        std::fs::remove_file(TEMP_FILE_PATH)?;
    }

    download_file(&external_text_url, TEMP_FILE_PATH, "external_texts.txt").await?;

    outln!("External Flash Texts Downloaded...");
    outln!("Begin parsing...");

    // Not in the C# version: config.ini download_parallelism images are fetched at once, but every
    // result is handled in the order of the one-by-one loop (same messages, same files).
    let mut steps = Vec::new();
    let mut seen = HashSet::new();
    for line in common::read_lines(TEMP_FILE_PATH)? {
        let Some((quest_name, quest_image)) = quest_parts(&line) else { continue };
        if !quest_image.contains('=') {
            steps.push(image_step(format!("{quest_name}_{quest_image}.png"), &mut seen));
        } else {
            steps.push(Step::Invalid(format!("{quest_name}_{quest_image}.png is not valid!")));
        }
        steps.push(image_step(format!("{quest_name}.png"), &mut seen));
    }

    let client = common::default_client();
    let quests_url = &quests_url;
    let results = futures::stream::iter(steps)
        .map(|step| async move {
            match step {
                Step::Image { file_name, first: true } => {
                    let file_path = quest_path(&file_name);
                    let fetched = if file_path.is_file() { None } else { Some(fetch(client, quests_url, &file_name).await) };
                    (Step::Image { file_name, first: true }, fetched)
                }
                other => (other, None),
            }
        })
        .buffered(common::download_parallelism());
    let mut results = std::pin::pin!(results);

    let mut download_count = 0;
    while let Some((step, fetched)) = results.next().await {
        match step {
            Step::Invalid(message) => colored(Color::Red, &message),
            Step::Image { file_name, first: true } => {
                if let Some(fetched) = fetched {
                    download_count += finish(fetched, &file_name);
                }
            }
            // Asked for earlier: downloaded again only when the earlier attempt left no file.
            Step::Image { file_name, first: false } => {
                if !quest_path(&file_name).is_file() {
                    download_count += finish(fetch(client, quests_url, &file_name).await, &file_name);
                }
            }
        }
    }

    console_done(download_count);
    Ok(())
}

enum Step {
    Invalid(String),
    /// `first`: the first time this run asks for the file (fetched ahead); later ones are
    /// resolved in order, like the one-by-one loop's `File.Exists` check.
    Image { file_name: String, first: bool },
}

fn image_step(file_name: String, seen: &mut HashSet<String>) -> Step {
    let first = seen.insert(file_name.clone());
    Step::Image { file_name, first }
}

fn quest_path(file_name: &str) -> PathBuf {
    PathBuf::from(format!("Habbo_Default/quests/{file_name}"))
}

fn console_done(download_count: usize) {
    habbo_core::console::set_color(Color::Green);
    outln!("Finished Downloading Quest images!");
    outln!("{download_count} images have been downloaded!");
    habbo_core::console::set_color(Color::Gray);
}

async fn fetch(client: &reqwest::Client, base_url: &str, file_name: &str) -> Result<PartFile, FetchError> {
    let url = format!("{base_url}/{file_name}");
    resume::fetch_ok(|| client.get(&url), &quest_path(file_name), &resume::Options::default()).await
}

/// The end of `DownloadQuestImageAsync`: 1 when the image was saved (failures are silent).
fn finish(fetched: Result<PartFile, FetchError>, file_name: &str) -> usize {
    match fetched.and_then(|part| part.commit().map_err(FetchError::from)) {
        Ok(()) => {
            colored(Color::Green, &format!("Downloaded: {file_name}"));
            colored(Color::Green, &format!("Downloaded: {file_name}"));
            1
        }
        Err(_) => 0,
    }
}

async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    resume::download_to(common::default_client(), url, Path::new(file_path), &resume::Options::FRESH).await?;
    colored(Color::Green, &format!("Downloaded: {file_name}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quest_lines() {
        assert_eq!(quest_parts("quests.ROOMBUILDER.Name=Room"), Some(("roombuilder".into(), "name=room".into())));
        assert_eq!(quest_parts("quests..A..B.c"), Some(("a".into(), "b".into())));
        assert_eq!(quest_parts("quests.a"), None);
        assert_eq!(quest_parts("xquests.a.b"), None);
    }
}
