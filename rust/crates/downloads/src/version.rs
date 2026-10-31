//! `VersionChecker` (System Classes/HabboVersion.cs): prints the current Habbo release.

use habbo_core::{console, outln, Color};

use crate::common::{self, FetchError, INDEX_OUT_OF_RANGE};

const EXTERNAL_VARIABLES_URL: &str = "https://www.habbo.com/gamedata/external_variables/1";

/// `VersionChecker.CheckVersionAsync`.
pub async fn check_version() -> anyhow::Result<()> {
    let result: Result<(), FetchError> = async {
        let source = common::get_string(common::default_client(), EXTERNAL_VARIABLES_URL).await?;
        match common::release_from_line_drop_last(&source) {
            Some(Ok(release)) => outln!("Current Habbo release: {release}"),
            Some(Err(_)) => return Err(FetchError::Other(anyhow::anyhow!(INDEX_OUT_OF_RANGE))),
            None => {}
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {}
        Err(FetchError::Http(message)) => {
            console::set_color(Color::Red);
            outln!("Error fetching Habbo release version: {message}");
            console::set_color(Color::Gray);
        }
        Err(FetchError::Other(error)) => {
            console::set_color(Color::Red);
            outln!("Unexpected error: {error}");
            console::set_color(Color::Gray);
        }
    }
    Ok(())
}
