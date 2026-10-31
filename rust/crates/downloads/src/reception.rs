//! `ReceptionDownloader` (Reception.cs): the hotel view images named by `${image.library.url}`
//! in external_variables into Habbo_Default/reception/<subfolder>.

use std::path::{Path, PathBuf};

use anyhow::Result;
use habbo_core::{console, outln, Color};

use crate::common::{self, colored, FetchError};

const SPLIT: &str = "${image.library.url}";
const BASE_URL: &str = "https://images.habbo.com/c_images/";

/// What one external_variables line asks for.
#[derive(Debug, PartialEq)]
pub enum ImageLine {
    NotSplit,
    EmptyName,
    NotImage(String),
    /// file path (relative to c_images), subfolder, whether the subfolder came from the path.
    Image { file_path: String, sub_folder: String, has_folder: bool },
}

pub fn parse_image_line(line: &str) -> ImageLine {
    let parts: Vec<&str> = line.split(SPLIT).collect();
    if parts.len() < 2 {
        return ImageLine::NotSplit;
    }
    let first = parts[1].split([',', ';']).next().unwrap_or("");
    let file_path = first.trim().trim_end_matches(['}', '"']).to_string();
    if file_path.is_empty() {
        return ImageLine::EmptyName;
    }
    let lower = file_path.to_lowercase();
    if ![".png", ".jpg", ".jpeg", ".gif"].iter().any(|ext| lower.ends_with(ext)) {
        return ImageLine::NotImage(file_path);
    }
    let sub_folder = Path::new(&file_path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    if sub_folder.is_empty() {
        ImageLine::Image { file_path, sub_folder: "other".into(), has_folder: false }
    } else {
        ImageLine::Image { file_path, sub_folder, has_folder: true }
    }
}

/// `ReceptionDownloader.DownloadReceptionImages`.
pub async fn download_reception_images() -> Result<()> {
    outln!("Starting Reception Download...");
    std::fs::create_dir_all("./temp")?;

    let external_variables_path = "./temp/external_variables.txt";
    outln!("Downloading external variables");

    let content = common::get_string(common::default_client(), "https://www.habbo.com/gamedata/external_variables/").await?;
    std::fs::write(external_variables_path, &content)?;

    outln!("🚀 Let's start downloading some images!");
    let mut download_count = 0;

    for line in common::read_lines(external_variables_path)? {
        if line.contains(SPLIT) {
            download_count = process_image_line(&line, "./Habbo_Default/reception", download_count).await?;
        }
    }

    colored(Color::Green, &format!("🎉 Finished downloading {download_count} images"));

    common::clean_temp()?;
    Ok(())
}

async fn process_image_line(line: &str, save_directory: &str, download_count: usize) -> Result<usize> {
    let (file_path, sub_folder) = match parse_image_line(line) {
        ImageLine::NotSplit => {
            outln!("❌ Skipping line (not split correctly)");
            return Ok(download_count);
        }
        ImageLine::EmptyName => {
            outln!("❌ Skipping line (empty filename)");
            return Ok(download_count);
        }
        ImageLine::NotImage(file_path) => {
            outln!("❌ Skipping non-image file: {file_path}");
            return Ok(download_count);
        }
        ImageLine::Image { file_path, sub_folder, has_folder } => {
            if !has_folder {
                console::set_color(Color::Yellow);
                outln!("❌ No subfolder found for: {file_path}. Saving in 'other/'");
                console::set_color(Color::Gray);
            }
            (file_path, sub_folder)
        }
    };

    let full_url = format!("{BASE_URL}{file_path}");
    let final_save_directory = Path::new(save_directory).join(&sub_folder);
    std::fs::create_dir_all(&final_save_directory)?;
    let full_file_path: PathBuf = final_save_directory.join(habbo_core::paths::file_name(Path::new(&file_path)));

    if full_file_path.is_file() {
        return Ok(download_count);
    }

    let mut download_count = download_count;
    let mut retry_count = 3;
    while retry_count > 0 {
        let request = common::default_client().get(&full_url).header(reqwest::header::ACCEPT, "image/png");
        let result: Result<Vec<u8>, FetchError> = async {
            let response = common::send_with(request, None).await?;
            common::ensure_success(&response)?;
            common::body(response).await
        }
        .await;
        match result {
            Ok(image_bytes) => {
                std::fs::write(&full_file_path, image_bytes)?;
                colored(Color::Green, &format!("📁 Downloaded: {file_path}"));
                download_count += 1;
                break;
            }
            Err(FetchError::Http(_)) => retry_count -= 1,
            Err(FetchError::Other(error)) => return Err(error),
        }
    }
    Ok(download_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_lines() {
        assert_eq!(
            parse_image_line(r#"landing.view.background=${image.library.url}reception/background_left.png"#),
            ImageLine::Image { file_path: "reception/background_left.png".into(), sub_folder: "reception".into(), has_folder: true }
        );
        assert_eq!(
            parse_image_line(r#"x=[{"image":"${image.library.url}web_promo_small/spromo_x.PNG"},{"a":1}]"#),
            ImageLine::Image { file_path: "web_promo_small/spromo_x.PNG".into(), sub_folder: "web_promo_small".into(), has_folder: true }
        );
        assert_eq!(
            parse_image_line("a=${image.library.url}top.gif;other"),
            ImageLine::Image { file_path: "top.gif".into(), sub_folder: "other".into(), has_folder: false }
        );
        assert_eq!(parse_image_line("a=${image.library.url}notes.txt"), ImageLine::NotImage("notes.txt".into()));
        assert_eq!(parse_image_line("a=${image.library.url},x"), ImageLine::EmptyName);
    }
}
