//! `ReceptionDownloader` (Reception.cs): the hotel view images named by `${image.library.url}`
//! in external_variables into Habbo_Default/reception/<subfolder>.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::StreamExt;
use habbo_core::{console, outln, Color};

use crate::common::{self, colored, FetchError};
use crate::resume::{self, PartFile};

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

    resume::download_to(
        common::default_client(),
        "https://www.habbo.com/gamedata/external_variables/",
        Path::new(external_variables_path),
        &resume::Options::FRESH,
    )
    .await?;

    outln!("🚀 Let's start downloading some images!");

    // Not in the C# version: config.ini download_parallelism images are fetched at once, but every
    // line is handled in order (same messages, same files) like the one-by-one loop.
    let mut seen = HashSet::new();
    let lines: Vec<Line> = common::read_lines(external_variables_path)?
        .iter()
        .filter(|line| line.contains(SPLIT))
        .map(|line| plan_line(line, "./Habbo_Default/reception", &mut seen))
        .collect();

    let results = futures::stream::iter(lines)
        .map(|line| async move {
            let fetched = match &line {
                Line::Image { full_url, directory, full_file_path, first: true, .. } => {
                    Some(fetch_new(full_url, directory, full_file_path).await)
                }
                _ => None,
            };
            (line, fetched)
        })
        .buffered(common::download_parallelism());
    let mut results = std::pin::pin!(results);

    let mut download_count = 0;
    while let Some((line, fetched)) = results.next().await {
        let (file_path, full_url, directory, full_file_path, has_folder, first) = match line {
            Line::Skip(message) => {
                outln!("{message}");
                continue;
            }
            Line::Image { file_path, full_url, directory, full_file_path, has_folder, first } => {
                (file_path, full_url, directory, full_file_path, has_folder, first)
            }
        };
        if !has_folder {
            console::set_color(Color::Yellow);
            outln!("❌ No subfolder found for: {file_path}. Saving in 'other/'");
            console::set_color(Color::Gray);
        }
        let fetched = match fetched {
            Some(fetched) => fetched?,
            // Asked for by an earlier line: fetched again only when that left no file.
            None if !first => fetch_new(&full_url, &directory, &full_file_path).await?,
            None => None,
        };
        if let Some(part) = fetched {
            part.commit()?;
            colored(Color::Green, &format!("📁 Downloaded: {file_path}"));
            download_count += 1;
        }
    }

    colored(Color::Green, &format!("🎉 Finished downloading {download_count} images"));

    common::clean_temp()?;
    Ok(())
}

/// One external_variables line, planned in order.
enum Line {
    /// A line that is skipped, with its message.
    Skip(String),
    Image {
        file_path: String,
        full_url: String,
        directory: PathBuf,
        full_file_path: PathBuf,
        has_folder: bool,
        /// The first line of this run that asks for this file (fetched ahead).
        first: bool,
    },
}

fn plan_line(line: &str, save_directory: &str, seen: &mut HashSet<String>) -> Line {
    match parse_image_line(line) {
        ImageLine::NotSplit => Line::Skip("❌ Skipping line (not split correctly)".into()),
        ImageLine::EmptyName => Line::Skip("❌ Skipping line (empty filename)".into()),
        ImageLine::NotImage(file_path) => Line::Skip(format!("❌ Skipping non-image file: {file_path}")),
        ImageLine::Image { file_path, sub_folder, has_folder } => {
            let full_url = format!("{BASE_URL}{file_path}");
            let directory = Path::new(save_directory).join(&sub_folder);
            let full_file_path = directory.join(habbo_core::paths::file_name(Path::new(&file_path)));
            let first = seen.insert(common::path_key(&full_file_path));
            Line::Image { file_path, full_url, directory, full_file_path, has_folder, first }
        }
    }
}

/// The folder, the `File.Exists` skip and up to 3 attempts on HTTP failures: the complete
/// download in its .part, or None (already there, or every attempt failed).
async fn fetch_new(full_url: &str, directory: &Path, full_file_path: &Path) -> Result<Option<PartFile>> {
    std::fs::create_dir_all(directory)?;
    if full_file_path.is_file() {
        return Ok(None);
    }
    let client = common::default_client();
    let mut retry_count = 3;
    while retry_count > 0 {
        let request = || client.get(full_url).header(reqwest::header::ACCEPT, "image/png");
        match resume::fetch_ok(request, full_file_path, &resume::Options::default()).await {
            Ok(part) => return Ok(Some(part)),
            Err(FetchError::Http(_)) => retry_count -= 1,
            Err(FetchError::Other(error)) => return Err(error),
        }
    }
    Ok(None)
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
