//! `Mp3Downloader` (mp3.cs): sound_machine_sample_1.mp3, ... into Habbo_Default/mp3 until 20
//! in a row fail.
//!
//! Not in the C# version: config.ini `download_parallelism` samples are fetched ahead at once,
//! but every result is handled (printed, renamed from its .part, counted) in sample order, so the
//! "20 in a row" stop, the messages and the files are those of the one-by-one loop. Samples
//! fetched ahead past the stop are thrown away.

use std::path::PathBuf;

use anyhow::Result;
use futures::StreamExt;
use habbo_core::Color;

use crate::common::{self, colored, FetchError};
use crate::resume::{self, PartFile};

fn mp3_path(mp3_number: u64) -> PathBuf {
    PathBuf::from(format!("./Habbo_Default/mp3/sound_machine_sample_{mp3_number}.mp3"))
}

/// `Mp3Downloader.DownloadMp3sAsync`.
pub async fn download_mp3s() -> Result<()> {
    let config = common::load_config()?;
    let sound_machine_url = common::cfg(&config, "AppSettings:soundmachineurl")?;
    let parallelism = common::download_parallelism();
    let client = common::default_client();

    let sound_machine_url = &sound_machine_url;
    let fetches = futures::stream::iter(1u64..)
        .map(|mp3_number| async move {
            let file_name = mp3_path(mp3_number);
            if file_name.is_file() {
                return (mp3_number, None);
            }
            let download_url = format!("{sound_machine_url}{mp3_number}.mp3");
            let fetched: Result<PartFile, FetchError> =
                resume::fetch_ok(|| client.get(&download_url), &file_name, &resume::Options::default()).await;
            (mp3_number, Some(fetched))
        })
        .buffered(parallelism);
    let mut fetches = Box::pin(fetches);

    let mut error_count = 0;
    let mut last_mp3 = 0;
    while error_count < 20 {
        let Some((mp3_number, fetched)) = fetches.next().await else { break };
        last_mp3 = mp3_number;

        let Some(fetched) = fetched else {
            colored(Color::DarkCyan, &format!("MP3 {mp3_number} already exists!"));
            error_count = 0;
            continue;
        };

        match finish(fetched, &format!("MP3 {mp3_number}")) {
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
    }

    // The samples fetched ahead past the stop: the one-by-one loop never asked for them.
    drop(fetches);
    resume::discard_parts((last_mp3 + 1..=last_mp3 + parallelism as u64).map(mp3_path));

    colored(Color::Green, "Finished downloading MP3s!");
    Ok(())
}

/// The end of `DownloadFileAsync`: the .part becomes the file, and the result is printed.
fn finish(fetched: Result<PartFile, FetchError>, file_name: &str) -> Result<(), FetchError> {
    let result = fetched.and_then(|part| part.commit().map_err(FetchError::from));
    match &result {
        Ok(()) => colored(Color::Green, &format!("Downloaded: {file_name}")),
        Err(FetchError::Http(message)) => colored(Color::Red, &format!("Error downloading {file_name}: {message}")),
        Err(FetchError::Other(_)) => {}
    }
    result
}
