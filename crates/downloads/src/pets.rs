//! `PetsDownloader` (Pets.cs): Habbo's pet libraries (gordon/<release>/<pet>) into
//! Habbo_Default/pets/{swf,hab}.

use anyhow::Result;
use habbo_core::paths::HabboAssetFolder;
use habbo_core::{outln, settings, Color};

use crate::asset_downloader;
use crate::common::{self, log};

/// The pet libraries to download; config.ini pet_libraries overrides it.
pub const DEFAULT_PETS: [&str; 36] = [
    "bear", "bearbaby", "bunnydepressed", "bunnyeaster", "bunnyevil", "bunnylove", "cat", "chicken", "cow", "croco",
    "demonmonkey", "dog", "dragon", "dragondog", "fools", "frog", "gnome", "haloompa", "horse", "kittenbaby", "lion",
    "monkey", "monster", "monsterplant", "pig", "pigeonevil", "pigeongood", "pigletbaby", "pterosaur", "puppybaby",
    "rhino", "spider", "terrier", "terrierbaby", "turtle", "velociraptor",
];

/// config.ini pet_libraries (comma separated, trimmed, empty entries dropped) or the default list.
pub fn pet_list(custom: Option<&str>) -> Vec<String> {
    match custom.filter(|c| !c.trim().is_empty()) {
        Some(custom) => custom.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        None => DEFAULT_PETS.iter().map(|s| s.to_string()).collect(),
    }
}

/// `PetsDownloader.DownloadPetsAsync`.
pub async fn download_pets() -> Result<()> {
    let config = common::load_config()?;
    let external_vars_url = common::cfg(&config, "AppSettings:externalvarsurl")?;
    let gordon_url = common::cfg(&config, "AppSettings:effecturl")?.trim_end_matches('/').to_string();

    let release = match asset_downloader::get_release(&external_vars_url).await {
        Ok(release) => release,
        Err(error) => {
            log(Color::Red, &format!("Error downloading external_variables: {error}"));
            return Ok(());
        }
    };
    let Some(release) = release.filter(|r| !r.is_empty()) else {
        log(Color::Red, "Error: Could not determine the release version.");
        return Ok(());
    };

    let pets = pet_list(config.get("AppSettings:pet_libraries").map(String::as_str));

    outln!(
        "Downloading {} pet libraries from release {release}; download format: {} (config.ini download_format)",
        pets.len(),
        settings::download_format()
    );

    asset_downloader::prepare_folder(HabboAssetFolder::PETS).await?;

    let libraries = asset_downloader::distinct_ignore_case(pets)
        .into_iter()
        .map(|pet| {
            let url = format!("{gordon_url}/{release}/{pet}");
            (pet, url)
        })
        .collect();
    let counts = asset_downloader::download_libraries(HabboAssetFolder::PETS, libraries).await?;

    log(Color::Green, &asset_downloader::summary("pet files", &counts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pet_lists() {
        assert_eq!(pet_list(None).len(), 36);
        assert_eq!(pet_list(Some("  ")).len(), 36);
        assert_eq!(pet_list(Some(" cat, ,dog,,Cat ")), ["cat", "dog", "Cat"]);
    }
}
