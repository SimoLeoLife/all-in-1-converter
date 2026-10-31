//! `OperationCatalog`: every operation the CLI menus and the Professional window can run.

use std::sync::OnceLock;

use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationCategory {
    HabboOriginal,
    NitroCustom,
    HotelTools,
    Database,
    General,
}

pub type Action = fn() -> BoxFuture<'static, Result<()>>;

pub struct OperationDefinition {
    pub id: &'static str,
    pub category: OperationCategory,
    pub title: &'static str,
    pub description: &'static str,
    pub action: Action,
    pub requires_input: bool,
    pub is_destructive: bool,
}

impl PartialEq for OperationDefinition {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub fn all() -> &'static [OperationDefinition] {
    static ALL: OnceLock<Vec<OperationDefinition>> = OnceLock::new();
    ALL.get_or_init(build)
}

pub fn for_category(category: OperationCategory) -> Vec<&'static OperationDefinition> {
    all().iter().filter(|op| op.category == category).collect()
}

/// The operation with this id; panics on an unknown id like C#'s `Single`.
pub fn get(id: &str) -> &'static OperationDefinition {
    find(id).unwrap_or_else(|| panic!("Unknown operation id '{id}'"))
}

pub fn find(id: &str) -> Option<&'static OperationDefinition> {
    all().iter().find(|op| op.id == id)
}

/// Wraps an `async fn() -> Result<()>` path as an [`Action`].
macro_rules! act {
    ($path:path) => {
        (|| $path().boxed()) as Action
    };
    ($body:block) => {
        (|| async move $body.boxed()) as Action
    };
}

/// A SWF conversion into .nitro or .hab; every sheet is WebP Lossless.
macro_rules! swf {
    ($extension:literal, $path:path) => {
        (|| {
            async {
                habbo_core::settings::set_spritesheet_format("webp");
                habbo_nitro::writer::set_extension($extension);
                $path().await
            }
            .boxed()
        }) as Action
    };
}

#[allow(clippy::too_many_arguments)]
fn op(
    id: &'static str,
    category: OperationCategory,
    title: &'static str,
    description: &'static str,
    action: Action,
    requires_input: bool,
    is_destructive: bool,
) -> OperationDefinition {
    OperationDefinition { id, category, title, description, action, requires_input, is_destructive }
}

fn build() -> Vec<OperationDefinition> {
    use OperationCategory::*;
    vec![
        op("habbo.badges", HabboOriginal, "Download badges", "Download official Habbo badge images.", act!(habbo_downloads::badges::download_badges), false, false),
        op("habbo.clothes", HabboOriginal, "Download clothes", "Download official FigureData.json and FigureMap.json.", act!(habbo_downloads::clothes::download_clothes), true, false),
        op("habbo.effects", HabboOriginal, "Download effects", "Download effect map metadata and avatar actions.", act!(habbo_downloads::effects::download_effects), true, false),
        op("habbo.furnidata", HabboOriginal, "Download furnidata", "Download and convert official furnidata to FurnitureData.json.", act!(habbo_downloads::furnidata::download_furnidata), false, false),
        op("habbo.furniture", HabboOriginal, "Download furniture", "Download official furniture as .swf and/or .hab (config.ini furniture_download_format), plus icons.", act!(habbo_downloads::furniture::download_furniture), false, false),
        op("habbo.icons", HabboOriginal, "Download catalogue icons", "Download official catalogue icon images.", act!(habbo_downloads::icons::download_icons), false, false),
        op("habbo.mp3", HabboOriginal, "Download MP3", "Download official sound assets.", act!(habbo_downloads::mp3::download_mp3s), false, false),
        op("habbo.productdata", HabboOriginal, "Download productdata", "Download official ProductData.json.", act!(habbo_downloads::productdata::download_product_data), false, false),
        op("habbo.quests", HabboOriginal, "Download quest images", "Download official quest banners and icons.", act!(habbo_downloads::quests::download_quests), false, false),
        op("habbo.reception", HabboOriginal, "Download reception images", "Download official reception and promotional artwork.", act!(habbo_downloads::reception::download_reception_images), false, false),
        op("habbo.texts", HabboOriginal, "Download texts", "Download official external text resources.", act!(habbo_downloads::flash_texts::download_texts), false, false),
        op("habbo.variables", HabboOriginal, "Download variables", "Download official external variable resources.", act!(habbo_downloads::variables::download_variables), false, false),
        op("habbo.pets", HabboOriginal, "Download pets", "Download official pet libraries as .swf and/or .hab (config.ini download_format).", act!(habbo_downloads::pets::download_pets), false, false),
        op("habbo.effect-icons", HabboOriginal, "Extract effect icons", "Extract the avatar effect icons (fx_icon_<type>) from Habbo's inventory bundle into fx_icons/.", act!(habbo_downloads::effect_icons::extract_effect_icons), false, false),
        op("habbo.import-new", HabboOriginal, "Import new Habbo furniture", "Find the furni Habbo added that your database does not have yet, download only those and write their SQL.", act!(habbo_pipeline::import_new_furniture), true, false),
        op("habbo.catalog-rip", HabboOriginal, "Rip Habbo catalogue (G-Earth)", "With G-Earth connected to your Habbo client, save the whole catalogue: page tree, layouts, texts, images, offers and prices, as JSON and Arcturus SQL.", act!(habbo_catalog::rip_catalog), true, false),
        op("habbo.all", HabboOriginal, "Download all", "Run the complete official asset download sequence.", act!(crate::ops::download_all), true, false),

        op("nitro.furniture", NitroCustom, "Download Nitro furniture", "Download custom Nitro furniture, icons and FurnitureData.json.", act!(habbo_downloads::nitro_furniture::download_nitro_furniture), true, false),
        op("nitro.clothes", NitroCustom, "Download Nitro clothes", "Download custom Nitro clothes, FigureData.json and FigureMap.json.", act!(habbo_downloads::nitro_clothes::download_custom_clothes), false, false),

        op("tools.merge-furnidata", HotelTools, "Merge furnidata", "Merge strict FurnitureData.json files without duplicate IDs or classnames.", act!(habbo_tools::merge_furnidata), true, false),
        op("tools.merge-productdata", HotelTools, "Merge productdata", "Merge strict ProductData.json files with conflict control.", act!(habbo_tools::merge_productdata), true, false),
        op("tools.merge-clothes", HotelTools, "Merge clothes", "Merge strict FigureData.json and FigureMap.json files.", act!(habbo_tools::merge_clothes), true, false),
        op("tools.generate-sql", HotelTools, "Generate SQL: Standaard furniture", "Generate items_base and catalog_items SQL from the furniture in Generate/default_furniture.", act!(habbo_db::sql_generator::generate_sql_standard), true, false),
        op("tools.generate-sql-bc", HotelTools, "Generate SQL: Build Club furniture", "Generate items_base and catalog_items_bc SQL from the furniture in Generate/bc_furniture.", act!(habbo_db::sql_generator::generate_sql_builders_club), true, false),
        op("tools.decompile-nitro", HotelTools, "Decompile NitroFiles", "Extract .nitro bundles into their json and sheet.", act!(habbo_nitro::bundle_tools::decompile_nitro), false, false),
        op("tools.compile-nitro", HotelTools, "Compile NitroFiles", "Pack json + sheet folders into .nitro bundles (WebP Lossless).", act!(habbo_nitro::bundle_tools::compile_nitro), false, false),
        op("tools.decompile-hab", HotelTools, "Decompile HabFiles", "Extract .hab bundles into their json and sheet.", act!(habbo_nitro::bundle_tools::decompile_hab), false, false),
        op("tools.compile-hab", HotelTools, "Compile HabFiles", "Pack json + sheet folders into .hab bundles (WebP Lossless).", act!(habbo_nitro::bundle_tools::compile_hab), false, false),
        op("tools.swf-furniture-nitro", HotelTools, "SWF Furniture to Nitro", "Convert furniture SWF files to .nitro (json + WebP Lossless).", swf!(".nitro", habbo_furni::convert_furniture), true, false),
        op("tools.swf-furniture-hab", HotelTools, "SWF Furniture to HAB", "Convert furniture SWF files to .hab (json + WebP Lossless).", swf!(".hab", habbo_furni::convert_furniture), true, false),
        op("tools.nitro-furniture-hab", HotelTools, "Nitro Furniture to HAB", "Repack furniture .nitro bundles as .hab (json + WebP Lossless).", act!(habbo_nitro::nitro_to_hab::furniture), true, false),
        op("tools.hab-furniture-32", HotelTools, "Add size 32 to HAB Furniture", "Copy the zoomed-out size 32 graphics from your .nitro furniture into Habbo's .hab furniture, which has none.", act!(habbo_nitro::hab_size32::furniture), true, false),
        op("tools.swf-clothes-nitro", HotelTools, "SWF Clothes to Nitro", "Convert clothing SWF files to .nitro (json + WebP Lossless).", swf!(".nitro", habbo_figure::convert_clothes), true, false),
        op("tools.swf-clothes-hab", HotelTools, "SWF Clothes to HAB", "Convert clothing SWF files to .hab (json + WebP Lossless).", swf!(".hab", habbo_figure::convert_clothes), true, false),
        op("tools.nitro-clothes-hab", HotelTools, "Nitro Clothes to HAB", "Repack clothing .nitro bundles as .hab (json + WebP Lossless).", act!(habbo_nitro::nitro_to_hab::clothes), true, false),
        op("tools.swf-pets-nitro", HotelTools, "SWF Pets to Nitro", "Convert pet SWF files to .nitro (json + WebP Lossless).", swf!(".nitro", habbo_furni::convert_pets), true, false),
        op("tools.swf-pets-hab", HotelTools, "SWF Pets to HAB", "Convert pet SWF files to .hab (json + WebP Lossless).", swf!(".hab", habbo_furni::convert_pets), true, false),
        op("tools.nitro-pets-hab", HotelTools, "Nitro Pets to HAB", "Repack pet .nitro bundles as .hab (json + WebP Lossless).", act!(habbo_nitro::nitro_to_hab::pets), true, false),
        op("tools.swf-effects-nitro", HotelTools, "SWF Effects to Nitro", "Convert effect SWF files to .nitro (json + WebP Lossless).", swf!(".nitro", habbo_figure::convert_effects), true, false),
        op("tools.swf-effects-hab", HotelTools, "SWF Effects to HAB", "Convert effect SWF files to .hab (json + WebP Lossless).", swf!(".hab", habbo_figure::convert_effects), true, false),
        op("tools.nitro-effects-hab", HotelTools, "Nitro Effects to HAB", "Repack effect .nitro bundles as .hab (json + WebP Lossless).", act!(habbo_nitro::nitro_to_hab::effects), true, false),
        op("tools.decompile-swf", HotelTools, "Decompile SWF Files", "Extract raw images, binaryData, symbols, ActionScript code and sounds from SWF files.", act!(habbo_swf::decompile), true, false),

        op("database.info", Database, "Database information", "Show server version, databases and sizes.", act!(habbo_db::general_info::show_database_version), false, false),
        op("database.optimize", Database, "Optimize database", "Optimize every table in the configured database.", act!(habbo_db::optimizer::optimize_database_tables), false, true),
        op("database.offer-id", Database, "Fix offer IDs", "Align database offer IDs with FurnitureData.json.", act!(habbo_db::set_offer_id::run), false, true),
        op("database.item-settings", Database, "Fix sit, lay and walk", "Align furniture interaction settings with FurnitureData.json.", act!(habbo_db::fix_item_settings::run), false, true),
        op("database.sprite-id", Database, "Fix sprite and item IDs", "Align sprite and item IDs with FurnitureData.json.", act!(habbo_db::fix_sprite_id_settings::run), false, true),
        op("database.interaction-count", Database, "Fix interaction count", "Write Generate/Update_Interaction_count_<date>.sql with the interaction_modes_count of each default floor/wall furni (no multiheight, no pets) from Habbo_Default/hof_furni/hab, plus Missing_Hab_files_<date>.txt.", act!(habbo_db::fix_interaction_count::run), false, false),
        op("database.item-size", Database, "Fix width, length and height", "Write Generate/Update_Item_size_<date>.sql with the width, length and stack_height of each floor/wall furni (no multiheight, no pets) from Habbo_Default/hof_furni/hab, plus Missing_Hab_files_<date>.txt.", act!(habbo_db::fix_item_size::run), false, false),

        op("general.version", General, "Current Habbo client version", "Fetch the current official Habbo client build.", act!(crate::ops::display_version), false, false),
        op("general.update", General, "Check for updates", "Look for a newer release of the converter on GitHub and install it.", act!(crate::update::check_and_install), true, false),
        op("general.credits", General, "Credits and contributors", "Show project authors and contributors.", act!({ crate::cli::credits::show(); Ok(()) }), false, false),
    ]
}

/// Runs one operation by id (habbo.all chains several).
pub async fn run(id: &str) -> Result<()> {
    (get(id).action)().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_known() {
        let mut ids: Vec<_> = all().iter().map(|op| op.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert_eq!(count, 52);
        for id in ["habbo.all", "tools.swf-furniture-hab", "tools.generate-sql", "database.info"] {
            assert!(find(id).is_some());
        }
        assert_eq!(for_category(OperationCategory::Database).len(), 7);
    }
}
