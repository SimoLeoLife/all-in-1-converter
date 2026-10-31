//! The Habbo Original downloaders (badges, clothes, effects, effect icons, flash texts,
//! furnidata, furniture, icons, mp3, pets, productdata, quests, reception, variables), the Nitro
//! custom downloaders (furniture, clothes), the shared asset library downloader and the Habbo
//! release check. Port of `SourceCode/Download Classes` and `System Classes/HabboVersion.cs`.
//!
//! Every operation is `pub async fn ...() -> anyhow::Result<()>`; output and prompts go through
//! `habbo_core::console`. Paths are relative to the working directory, like the C# version.

pub mod asar;
pub mod asset_downloader;
pub mod badges;
pub mod clothes;
pub mod common;
pub mod effect_icons;
pub mod effects;
pub mod flash_texts;
pub mod furnidata;
pub mod furniture;
pub mod icons;
pub mod jsonw;
pub mod model;
pub mod mp3;
pub mod nitro_clothes;
pub mod nitro_furniture;
pub mod pets;
pub mod productdata;
pub mod quests;
pub mod reception;
pub mod resume;
pub mod variables;
pub mod version;
pub mod xml;

pub use version::check_version;
