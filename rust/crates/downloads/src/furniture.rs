//! `FurnitureDownloader` (Furniture.cs): Habbo's furniture into Habbo_Default/hof_furni/{swf,hab,icons}.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use futures::TryStreamExt;
use habbo_core::paths::{hof_furni, HabboAssetFolder};
use habbo_core::{outln, settings, Color};

use crate::asset_downloader::{self, is_png, try_fetch};
use crate::common::{self, log, parse_int_strict};
use crate::xml::Document;

/// (classname, revision) of every furnitype in roomitemtypes and wallitemtypes.
pub fn furniture_entries(doc: &Document) -> Result<Option<Vec<(String, i32)>>> {
    let Some(root) = doc.element("furnidata") else { return Ok(None) };
    let mut entries = Vec::new();
    for section in ["roomitemtypes", "wallitemtypes"] {
        let Some(items) = root.element(section) else { continue };
        for item in items.elements("furnitype") {
            let classname = item.attr("classname").unwrap_or("").to_string();
            let revision = match item.element("revision") {
                None => 0,
                Some(revision) => parse_int_strict(Some(&revision.value()))?,
            };
            if !classname.is_empty() {
                entries.push((classname, revision));
            }
        }
    }
    Ok(Some(entries))
}

/// One library per furni (colour variants share it), at the highest revision of the group.
/// Groups keep first-seen order and the first spelling (OrdinalIgnoreCase).
pub fn furniture_libraries(entries: &[(String, i32)], furniture_url: &str) -> Vec<(String, String)> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut groups: Vec<(String, i32)> = Vec::new(); // first spelling, max revision
    for (classname, revision) in entries {
        let name = classname.split('*').next().unwrap_or("");
        match index.get(&name.to_lowercase()) {
            Some(&i) => groups[i].1 = std::cmp::max(groups[i].1, *revision),
            None => {
                index.insert(name.to_lowercase(), groups.len());
                groups.push((name.to_string(), *revision));
            }
        }
    }
    groups
        .into_iter()
        .map(|(name, revision)| {
            let url = format!("{furniture_url}/{revision}/{name}");
            (name, url)
        })
        .collect()
}

/// The icon name of a classname: "name" or "name_variant".
pub fn icon_name(classname: &str) -> String {
    let mut parts = classname.split('*');
    let furniture_name = parts.next().unwrap_or("");
    match parts.next() {
        Some(variant) if !variant.is_empty() => format!("{furniture_name}_{variant}"),
        _ => furniture_name.to_string(),
    }
}

/// `FurnitureDownloader.DownloadFurnitureAsync`.
pub async fn download_furniture() -> Result<()> {
    let config = common::load_config()?;
    let furnidata_url = common::cfg(&config, "AppSettings:furnidataXML")?;
    let furniture_url = common::cfg(&config, "AppSettings:furnitureurl")?.trim_end_matches('/').to_string();

    outln!("Furniture download format: {} (config.ini download_format)", settings::download_format());

    std::fs::create_dir_all(hof_furni::icons())?;
    asset_downloader::prepare_folder(HabboAssetFolder::FURNITURE).await?;

    outln!("Downloading furnidata...");
    let furnidata_xml = match common::get_string(asset_downloader::http(), &furnidata_url).await {
        Ok(text) => {
            outln!("Furnidata downloaded successfully.");
            text
        }
        Err(error) => {
            log(Color::Red, &format!("Error downloading furnidata: {error}"));
            return Ok(());
        }
    };

    let doc = match Document::parse(&furnidata_xml) {
        Ok(doc) => doc,
        Err(error) => {
            log(Color::Red, &format!("Error: Invalid furnidata XML format. {error}"));
            return Ok(());
        }
    };
    // NOTE(port): a non-numeric <revision> threw out of the C# method; it is returned as Err here.
    let Some(entries) = furniture_entries(&doc)? else {
        log(Color::Red, "Error: Invalid furnidata XML format.");
        return Ok(());
    };

    outln!("Found {} furniture entries.", entries.len());

    let libraries = furniture_libraries(&entries, &furniture_url);
    let counts = asset_downloader::download_libraries(HabboAssetFolder::FURNITURE, libraries).await?;

    let icon_count = download_icons(&entries, &furniture_url).await?;

    log(Color::Green, "Downloading furniture completed!");
    log(Color::Green, &asset_downloader::summary(&format!("furniture files and {icon_count} new icons"), &counts));
    Ok(())
}

/// The icons (`<icon name>_icon.png`) of these (classname, revision) entries that are not in
/// Habbo_Default/hof_furni/icons yet, config.ini `download_parallelism` at a time. Returns how
/// many were downloaded. The folder must exist.
pub async fn download_icons(entries: &[(String, i32)], furniture_url: &str) -> Result<usize> {
    let icon_count = AtomicUsize::new(0);
    futures::stream::iter(entries.iter().map(Ok::<_, anyhow::Error>))
        .try_for_each_concurrent(common::download_parallelism(), |(classname, revision)| {
            let icon_count = &icon_count;
            async move {
                let icon_name = icon_name(classname);
                let icon_path = Path::new(&hof_furni::icons()).join(format!("{icon_name}_icon.png"));
                if icon_path.is_file() {
                    return Ok(());
                }
                if let Some(part) = try_fetch(&format!("{furniture_url}/{revision}/{icon_name}_icon.png"), &icon_path).await {
                    if is_png(&part.read()?) {
                        part.commit()?;
                        icon_count.fetch_add(1, Ordering::SeqCst);
                        log(Color::Green, &format!("Downloaded: {icon_name}_icon.png"));
                    } else {
                        let _ = part.discard();
                    }
                }
                Ok(())
            }
        })
        .await?;
    Ok(icon_count.load(Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_libraries_and_icons() {
        let doc = Document::parse(
            r#"<furnidata><roomitemtypes>
<furnitype classname="chair*1"><revision>5</revision></furnitype>
<furnitype classname="Chair*2"><revision>9</revision></furnitype>
<furnitype classname=""><revision>1</revision></furnitype>
<furnitype classname="table"/>
</roomitemtypes><wallitemtypes><furnitype classname="poster*12"><revision>3</revision></furnitype></wallitemtypes></furnidata>"#,
        )
        .unwrap();
        let entries = furniture_entries(&doc).unwrap().unwrap();
        assert_eq!(entries, [("chair*1".into(), 5), ("Chair*2".into(), 9), ("table".into(), 0), ("poster*12".into(), 3)]);
        let libraries = furniture_libraries(&entries, "https://x/dcr/hof_furni");
        assert_eq!(
            libraries,
            [
                ("chair".to_string(), "https://x/dcr/hof_furni/9/chair".to_string()),
                ("table".into(), "https://x/dcr/hof_furni/0/table".into()),
                ("poster".into(), "https://x/dcr/hof_furni/3/poster".into()),
            ]
        );
        assert_eq!(icon_name("chair*1"), "chair_1");
        assert_eq!(icon_name("table"), "table");
        assert_eq!(icon_name("odd*"), "odd");

        assert!(furniture_entries(&Document::parse("<other/>").unwrap()).unwrap().is_none());
        let bad = Document::parse("<furnidata><roomitemtypes><furnitype classname=\"a\"><revision>x</revision></furnitype></roomitemtypes></furnidata>").unwrap();
        assert!(furniture_entries(&bad).is_err());
    }
}
