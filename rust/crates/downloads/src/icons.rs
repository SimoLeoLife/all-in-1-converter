//! `IconDownloader` (icons.cs): catalogue icons icon_1.png, icon_2.png, ... into
//! Habbo_Default/icons until 20 in a row are missing.
//!
//! Not in the C# version: config.ini `download_parallelism` icons are fetched ahead at once, but
//! every result is handled (printed, renamed from its .part, counted) in icon order, so the
//! "20 in a row" stop, the messages and the files are those of the one-by-one loop. Icons fetched
//! ahead past the stop are thrown away.

use std::path::PathBuf;

use anyhow::Result;
use futures::StreamExt;
use habbo_core::Color;

use crate::common::{self, colored, FetchError};
use crate::resume::{self, PartFile};

fn icon_path(icon_number: u64) -> PathBuf {
    PathBuf::from(format!("./Habbo_Default/icons/icon_{icon_number}.png"))
}

/// `IconDownloader.DownloadIcons`.
pub async fn download_icons() -> Result<()> {
    let config = common::load_config()?;
    let catalog_icon_url = common::cfg(&config, "AppSettings:catalogiconurl")?;
    let parallelism = common::download_parallelism();
    let client = common::default_client();

    let catalog_icon_url = &catalog_icon_url;
    let fetches = futures::stream::iter(1u64..)
        .map(|icon_number| async move {
            let file_path = icon_path(icon_number);
            if file_path.is_file() {
                return (icon_number, None);
            }
            let url = format!("{catalog_icon_url}{icon_number}.png");
            let fetched: Result<PartFile, FetchError> =
                resume::fetch_ok(|| client.get(&url), &file_path, &resume::Options::default()).await;
            (icon_number, Some(fetched))
        })
        .buffered(parallelism);
    let mut fetches = Box::pin(fetches);

    let mut consecutive_errors = 0;
    let mut last_icon = 0;
    while consecutive_errors < 20 {
        let Some((icon_number, fetched)) = fetches.next().await else { break };
        last_icon = icon_number;
        match fetched {
            Some(fetched) => match fetched.and_then(|part| part.commit().map_err(FetchError::from)) {
                Ok(()) => {
                    colored(Color::Green, &format!("Downloaded Icon {icon_number}"));
                    consecutive_errors = 0;
                }
                Err(FetchError::Http(message)) if message.contains("404") => {
                    colored(Color::Red, &format!("Icon {icon_number} is not found on the server."));
                    consecutive_errors += 1;
                }
                Err(error) => {
                    colored(Color::Red, &format!("Error downloading icon {icon_number}: {error}"));
                    consecutive_errors += 1;
                }
            },
            None => {
                colored(Color::DarkCyan, &format!("Icon {icon_number} already exists!"));
                consecutive_errors = 0;
            }
        }
    }

    // The icons fetched ahead past the stop: the one-by-one loop never asked for them.
    drop(fetches);
    resume::discard_parts((last_icon + 1..=last_icon + parallelism as u64).map(icon_path));

    colored(Color::Green, "Finished downloading icons!");
    Ok(())
}
