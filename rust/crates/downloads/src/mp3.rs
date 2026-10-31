//! `Mp3Downloader` (mp3.cs): sound_machine_sample_1.mp3, ... into Habbo_Default/mp3 until 20
//! in a row fail.

use anyhow::Result;
use habbo_core::Color;

use crate::common::{self, colored, FetchError};

/// `Mp3Downloader.DownloadMp3sAsync`.
pub async fn download_mp3s() -> Result<()> {
    let config = common::load_config()?;
    let sound_machine_url = common::cfg(&config, "AppSettings:soundmachineurl")?;
    let mut mp3_number: u64 = 1;
    let mut error_count = 0;

    while error_count < 20 {
        let file_name = format!("./Habbo_Default/mp3/sound_machine_sample_{mp3_number}.mp3");

        if std::path::Path::new(&file_name).is_file() {
            colored(Color::DarkCyan, &format!("MP3 {mp3_number} already exists!"));
            mp3_number += 1;
            error_count = 0;
            continue;
        }

        let download_url = format!("{sound_machine_url}{mp3_number}.mp3");
        match download_file(&download_url, &file_name, &format!("MP3 {mp3_number}")).await {
            Ok(()) => error_count = 0,
            Err(FetchError::Http(message)) => {
                colored(Color::Red, &format!("Error downloading MP3 {mp3_number}: {message}"));
                error_count += 1;
            }
            Err(FetchError::Other(error)) => {
                colored(Color::Red, &format!("Unexpected error downloading MP3 {mp3_number}: {error}"));
                error_count += 1;
            }
        }
        mp3_number += 1;
    }

    colored(Color::Green, "Finished downloading MP3s!");
    Ok(())
}

async fn download_file(url: &str, file_path: &str, file_name: &str) -> Result<(), FetchError> {
    let result: Result<(), FetchError> = async {
        let bytes = common::get_bytes(common::default_client(), url).await?;
        std::fs::write(file_path, bytes)?;
        Ok(())
    }
    .await;
    match &result {
        Ok(()) => colored(Color::Green, &format!("Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}
