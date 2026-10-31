//! `IconDownloader` (icons.cs): catalogue icons icon_1.png, icon_2.png, ... into
//! Habbo_Default/icons until 20 in a row are missing.

use anyhow::Result;
use habbo_core::Color;

use crate::common::{self, colored, FetchError};

/// `IconDownloader.DownloadIcons`.
pub async fn download_icons() -> Result<()> {
    let config = common::load_config()?;
    let catalog_icon_url = common::cfg(&config, "AppSettings:catalogiconurl")?;
    let mut icon_number: u64 = 1;
    let mut consecutive_errors = 0;

    while consecutive_errors < 20 {
        let file_path = format!("./Habbo_Default/icons/icon_{icon_number}.png");

        if !std::path::Path::new(&file_path).is_file() {
            let url = format!("{catalog_icon_url}{icon_number}.png");
            let result: Result<(), FetchError> = async {
                let data = common::get_bytes(common::default_client(), &url).await?;
                std::fs::write(&file_path, data)?;
                Ok(())
            }
            .await;
            match result {
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
            }
        } else {
            colored(Color::DarkCyan, &format!("Icon {icon_number} already exists!"));
            consecutive_errors = 0;
        }

        icon_number += 1;
    }

    colored(Color::Green, "Finished downloading icons!");
    Ok(())
}
