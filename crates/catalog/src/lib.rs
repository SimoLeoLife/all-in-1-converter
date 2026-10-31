//! Catalogue ripper: reads Habbo's catalogue through a G-Earth extension connection.
//!
//! The user runs G-Earth and logs into Habbo with their own client. [`rip_catalog`] connects
//! to G-Earth as a network extension (127.0.0.1, port 9092 upwards), asks the session for the
//! catalogue index (NORMAL and BUILDERS_CLUB) and every page, one request at a time, and saves
//! `Catalog/<hotel>_<timestamp>/`: catalog.json, catalog_pages.sql + catalog_items.sql for
//! Arcturus, README.txt and the page images and icons. Every intercepted game packet is let
//! through unchanged at once; the only packets sent are GetCatalogIndex and GetCatalogPage.

pub mod gearth_install;
pub mod export;
pub mod gearth;
pub mod images;
pub mod packets;
pub mod rip;
pub mod wire;

pub use rip::{rip_catalog_with, RipOptions, RipReport};

/// "Rip Habbo catalogue (G-Earth)": the operation, with the settings from config.ini.
pub async fn rip_catalog() -> anyhow::Result<()> {
    rip_catalog_with(RipOptions::from_config())
        .await
        .map(|_| ())
}
