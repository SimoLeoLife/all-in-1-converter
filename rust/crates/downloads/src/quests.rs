//! `QuestsDownloader` (Quests.cs): the quest images named in external_flash_texts
//! ("quests.<name>.<image>...") into Habbo_Default/quests.

use std::path::Path;

use anyhow::Result;
use habbo_core::{outln, Color};

use crate::common::{self, colored, FetchError};

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

    let mut download_count = 0;
    for line in common::read_lines(TEMP_FILE_PATH)? {
        let Some((quest_name, quest_image)) = quest_parts(&line) else { continue };

        if !Path::new(&format!("Habbo_Default/quests/{quest_name}_{quest_image}.png")).is_file() && !quest_image.contains('=') {
            download_count += download_quest_image(&quests_url, &quest_name, &quest_image).await;
        } else if quest_image.contains('=') {
            colored(Color::Red, &format!("{quest_name}_{quest_image}.png is not valid!"));
        }

        if !Path::new(&format!("Habbo_Default/quests/{quest_name}.png")).is_file() {
            download_count += download_quest_image(&quests_url, &quest_name, "").await;
        }
    }

    console_done(download_count);
    Ok(())
}

fn console_done(download_count: usize) {
    habbo_core::console::set_color(Color::Green);
    outln!("Finished Downloading Quest images!");
    outln!("{download_count} images have been downloaded!");
    habbo_core::console::set_color(Color::Gray);
}

async fn download_quest_image(base_url: &str, quest_name: &str, quest_image: &str) -> usize {
    let file_name = if quest_image.is_empty() { format!("{quest_name}.png") } else { format!("{quest_name}_{quest_image}.png") };
    let file_path = format!("Habbo_Default/quests/{file_name}");

    match download_file(&format!("{base_url}/{file_name}"), &file_path, &file_name).await {
        Ok(()) => {
            colored(Color::Green, &format!("Downloaded: {file_name}"));
            1
        }
        Err(_) => 0,
    }
}

async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    let bytes = common::get_bytes(common::default_client(), url).await?;
    std::fs::write(file_path, bytes)?;
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
