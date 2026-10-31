//! The main menu and the four sub-menus, with their "How to use" pages.

use anyhow::Result;
use futures::FutureExt;

use super::{show, MenuItem};

pub fn main_menu() -> Vec<MenuItem> {
    let current = super::HOST.lock().unwrap().mode;
    vec![
        MenuItem::new("1", "Habbo Original Downloads", || habbo_original().boxed()).help(
            "Open the Habbo Original Downloads sub-menu.\n\
Pulls official Habbo CDN assets (badges, clothes, effects, furnidata,\n\
furniture SWF, icons, MP3, productdata, quests, reception, texts,\n\
variables). Use this to bootstrap a fresh hotel with the latest\n\
official assets before customising. Target hotel (.nl / .com / etc.)\n\
is controlled by the URLs in config.ini.",
        ),
        MenuItem::new("2", "Nitro Custom Downloads", || nitro_custom().boxed()).help(
            "Open the Nitro Custom Downloads sub-menu.\n\
Pulls custom furniture / clothes packs from another retro by reading\n\
its renderer-config.json endpoints (set them in config.ini under the\n\
[Nitro Retro Hotels] block). FurnitureData.json, FigureData.json\n\
and FigureMap.json must be strict single JSON files.",
        ),
        MenuItem::new("3", "Hotel Tools", || hotel_tools().boxed()).help(
            "Open the Hotel Tools sub-menu.\n\
Hosts every transformation step of the classic asset pipeline: merge\n\
furnidata / productdata / clothesdata using strict JSON files,\n\
generate items_base +\n\
catalog_items SQL from .nitro / .hab / .swf, decompile / compile .nitro\n\
and .hab bundles, convert SWF -> Nitro or HAB and Nitro -> HAB for\n\
furniture / clothes / pets / effects. Every bundle is json + WebP Lossless.",
        ),
        MenuItem::new("4", "Database Tools", || database().boxed()).help(
            "Open the Database Tools sub-menu.\n\
Hits the configured MariaDB / MySQL (credentials in config.ini\n\
[Database Settings]) to inspect or repair the hotel DB: show\n\
version + size, OPTIMIZE every table, fix offer_id / sprite_id\n\
/ item_id / sit-lay-walk in items_base from FurnitureData.json.\n\
Use option 5 (Fix Sprite_ID) FIRST when adding new furniture.",
        ),
        MenuItem::op("version", "Fetch current Habbo client version", "general.version").help(
            "Hits https://www.habbo.com/gamedata/external_variables/1 and prints\n\
the current Flash client build directory name (e.g. \"PRODUCTION-...\").\n\
Useful to check whether the official Habbo assets have rotated since\n\
your last download pass.",
        ),
        MenuItem::op("update", "Check for updates", "general.update").help(
            "Looks for a newer release of the Rust converter on GitHub (config.ini
update_repo, default SimoLeoLife/all-in-1-converter) and installs it after
asking. The previous exe is kept as .old until the next start.",
        ),
        MenuItem::op("credits", "Credits & contributors", "general.credits").help(
            "Prints the authors and contributors of the tool: Life\n\
(.NET 11, strict JSON-only migration, Professional UI),\n\
medievalshell (.NET 10 modernization and CLI/TUI/GUI shells),\n\
duckietm (original upstream tool), plus Nitro Team Discord,\n\
AtlasOmega and Leet who contributed assets / effects.",
        ),
        MenuItem::new("s", format!("Switch UI mode (current: {})", current.to_string().to_uppercase()), move || {
            async move {
                super::mode_selector::prompt_and_request_switch(current);
                Ok(())
            }
            .boxed()
        })
        .help(
            "Closes the current UI and reopens the workstation in the mode you pick.\n\
Available targets: Professional (native dashboard),\n                  \
CLI (classic console, keyboard only; also over SSH).",
        ),
    ]
}

fn format_label() -> String {
    habbo_core::settings::download_format().to_uppercase()
}

pub async fn habbo_original() -> Result<()> {
    let format = format_label();
    show(
        "Habbo Original Downloads",
        vec![
            MenuItem::op("1", "Download Badges", "habbo.badges").help(
                "Pulls every badge .gif/.png from the official Habbo CDN.\n\
Output: Habbo_Default/badges/. Skips files already on disk.",
            ),
            MenuItem::op("2", format!("Download Clothes (figuredata + figuremap + {format})"), "habbo.clothes").help(
                "Downloads FigureData.json (palettes + setTypes) and FigureMap.json\n\
(libraries) from Habbo. Saves to Habbo_Default/files/json/ for use by\n\
Merge Clothes (option 3 of Hotel Tools).\n\
The clothes libraries go to Habbo_Default/clothes/swf and /hab (config.ini download_format).",
            ),
            MenuItem::op("3", format!("Download Effects (effectmap + HabboAvatarActions + {format})"), "habbo.effects").help(
                "Fetches effect map metadata and the HabboAvatarActions.json file.\n\
Required for SWF Effects to Nitro conversion.\n\
The effect libraries go to Habbo_Default/effects/swf and /hab (config.ini download_format).",
            ),
            MenuItem::op("4", "Download Furnidata -> FurnitureData.json", "habbo.furnidata").help(
                "Downloads the XML furniture catalog from Habbo and converts it to\n\
FurnitureData.json in Habbo_Default/files/json/. Used by Merge Furnidata\n\
and Generate SQL.",
            ),
            MenuItem::op("5", format!("Download Furniture ({format})"), "habbo.furniture").help(
                "Pulls Habbo's furniture into Habbo_Default/hof_furni/: .swf into swf/, .hab into hab/,\n\
icons into icons/. config.ini download_format picks swf, hab or both.\n\
A .hab gets a lossless WebP sheet (spritesheet_format=webp); its json is not changed.\n\
The swf/ files are what SWF Furniture to Nitro (Hotel Tools option 9/10, H mode) reads.",
            ),
            MenuItem::op("6", "Download Catalogue icons", "habbo.icons").help(
                "Downloads the small icon PNGs shown in the catalogue UI.\n\
Output: Habbo_Default/hof_furni/icons/ and Habbo_Default/icons/.",
            ),
            MenuItem::op("7", "Download MP3", "habbo.mp3").help(
                "Downloads sound samples (.mp3) used by traxmachine and various furni.\n\
Output: Habbo_Default/mp3/.",
            ),
            MenuItem::op("8", "Download Productdata", "habbo.productdata").help(
                "Downloads ProductData.json (catalogue product names + descriptions).\n\
Used by Merge Productdata in Hotel Tools.",
            ),
            MenuItem::op("9", "Download Quests images", "habbo.quests")
                .help("Downloads quest banner / icon images. Output: Habbo_Default/quests/."),
            MenuItem::op("10", "Download Reception images", "habbo.reception").help(
                "Downloads the rotating reception / promo art shown on the lobby page.\n\
Output: Habbo_Default/reception/ and reception/web_promo_small/.",
            ),
            MenuItem::op("11", "Download Texts", "habbo.texts").help(
                "Downloads external_flash_texts / external_texts (UI strings + chat lines).\n\
Now saved as JSON. Output: Habbo_Default/files/txt/ and .../json/.",
            ),
            MenuItem::op("12", "Download Variables", "habbo.variables").help(
                "Downloads external_variables (URLs, feature flags, host config).\n\
Useful for inspecting which CDN endpoints Habbo currently advertises.",
            ),
            MenuItem::op("13", format!("Download Pets ({format})"), "habbo.pets").help(
                "Downloads Habbo's pet libraries into Habbo_Default/pets/swf and /hab\n\
(config.ini download_format). The list can be changed with config.ini pet_libraries.\n\
A .hab gets a lossless WebP sheet (spritesheet_format=webp); its json is not changed.",
            ),
            MenuItem::op("14", "Extract Effect icons (fx_icon_<type> for the avatar editor)", "habbo.effect-icons").help(
                "Habbo has no CDN URL for the effect icons: they ship inside the Habbo Classic app at\n\
HabboClassicWin.zip > resources/app.asar > client/generated/habbo-inventory-com.hab.\n\
Put the zip, the app.asar, that .hab or a folder of fx_icon_<type> PNGs in Habbo_Default/import/effect_icons\n\
(or set config.ini effect_icons_source), or set config.ini effect_icons_url to a download link\n\
of one of them (for example a mirror). Output: Habbo_Default/fx_icons/fx_icon_<type>.png,\n\
for the client's avatareditor.effects.icon.url (${image.library.url}fx_icons/fx_icon_%type%.png).",
            ),
            MenuItem::op("new", "Import new Habbo furniture (download + SQL)", "habbo.import-new").help(
                "Downloads the current furnidata, compares it with items_base in your database
(or, without a database, with Habbo_Default/hof_furni) and downloads only the new
furni into Generate/default_furniture, then writes their SQL to Generate/Output_SQL.
The database itself is not changed: check the SQL and import it yourself.",
            ),
            MenuItem::op("all", "Download All (clothes + furni + product + ...)", "habbo.all").help(
                "Runs the full bootstrap sequence in order: clothes, furnidata, productdata,\n\
furniture SWF, variables, texts, icons. Useful when initializing a fresh\n\
hotel - everything Merge / Generate SQL / SWF->Nitro will need afterwards.",
            ),
        ],
        false,
    )
    .await
}

pub async fn nitro_custom() -> Result<()> {
    show(
        "Nitro Custom Downloads",
        vec![
            MenuItem::op("1", "Download NitroFurniture", "nitro.furniture").help(
                "Pulls every .nitro furniture file from a Nitro V3 retro into\n\
custom_downloads/nitro_furniture/.\n\
Reads three config.ini keys: nitro_furnidataJSON, nitro_furnitureurl,\n\
nitro_furniture_icon_url. nitro_furnidataJSON must point directly\n\
to one strict FurnitureData.json file.\n\
\n\
Replace ##DOMAIN## in config.ini with the retro's hostname before\n\
running.",
            ),
            MenuItem::op("2", "Download NitroClothes", "nitro.clothes").help(
                "Pulls FigureData.json + FigureMap.json from a Nitro V3 retro and then\n\
every .nitro clothing library listed in FigureMap, into\n\
custom_downloads/clothes/.\n\
Reads three config.ini keys: nitro_clothes_dir, nitro_figuredata,\n\
nitro_figuremap. FigureData and FigureMap must each be one JSON file.\n\
Skips hh_human_fx and hh_pets libraries (those\n\
belong to Effects / Pets respectively).",
            ),
        ],
        false,
    )
    .await
}

pub async fn hotel_tools() -> Result<()> {
    show(
        "Hotel Tools Menu",
        vec![
            MenuItem::op("1", "Merge Furnidata", "tools.merge-furnidata").help(
                "Combines Original_Furnidata + Import_Furnidata into Merged_Furnidata.\n\
Skips duplicates by classname OR by id (additive only, no override).\n\
Reads and writes one strict FurnitureData.json file.",
            ),
            MenuItem::op("2", "Merge Productdata", "tools.merge-productdata").help(
                "Combines Original_ProductData + Import_ProductData into Merged_ProductData.\n\
For each conflict on `code` you can answer (Y) replace, (A) yes-to-all,\n\
(N) skip, (Z) no-to-all. Reads and writes strict JSON files.",
            ),
            MenuItem::op("3", "Merge Clothes", "tools.merge-clothes").help(
                "Merges FigureData (palettes + setTypes) AND FigureMap (libraries)\n\
from Original_ClothesData + Import_ClothesData.\n\
Writes FigureData.json and FigureMap.json into Merged_ClothesData/.",
            ),
            MenuItem::op("4", "Generate SQL: Standaard furniture", "tools.generate-sql").help(
                "Reads FurnitureData.json from Generate/Furnidata/\n\
and every .nitro / .hab / .swf inside Generate/default_furniture/ (recursive).\n\
Asks: starting ID for items_base + catalog_items, plus the catalog_pages ID.\n\
Produces SQL files in Generate/Output_SQL/ with timestamp, one INSERT per item.\n\
Width / length / height / interactions are read from each .nitro / .hab automatically.",
            ),
            MenuItem::op("5", "Generate SQL: Build Club furniture", "tools.generate-sql-bc").help(
                "Same as (4), for Builders Club furniture.\n\
Reads every .nitro / .hab / .swf inside Generate/bc_furniture/ (recursive).\n\
Asks: starting ID for items_base + catalog_items_bc, plus the catalog_pages_bc ID.\n\
Writes items_base and catalog_items_bc INSERTs into Generate/Output_SQL/ (bc_items_<timestamp>.sql).",
            ),
            MenuItem::op("6", "Decompile NitroFiles", "tools.decompile-nitro").help(
                "Drop .nitro bundles into NitroCompiler/extract/{furni,clothing,effects,pets,generic}/nitro/\n\
Output: the json + sheet exactly as stored, in NitroCompiler/extracted/<type>/<name>/",
            ),
            MenuItem::op("7", "Compile NitroFiles", "tools.compile-nitro").help(
                "Inverse of (6). Packs each NitroCompiler/compile/<type>/<name>/ folder (json + sheet)\n\
into NitroCompiler/compiled/<type>/nitro/<name>.nitro. A PNG sheet becomes lossless WebP.",
            ),
            MenuItem::op("8", "Decompile HabFiles", "tools.decompile-hab").help(
                "Drop .hab bundles into NitroCompiler/extract/{furni,clothing,effects,pets,generic}/hab/\n\
Output: the json + sheet exactly as stored, in NitroCompiler/extracted/<type>/<name>/",
            ),
            MenuItem::op("9", "Compile HabFiles", "tools.compile-hab").help(
                "Inverse of (8). Packs each NitroCompiler/compile/<type>/<name>/ folder (json + sheet)\n\
into NitroCompiler/compiled/<type>/hab/<name>.hab. A PNG sheet becomes lossless WebP.",
            ),
            MenuItem::op("10", "SWF Furniture to Nitro", "tools.swf-furniture-nitro").help(
                "Convert furniture .swf files to .nitro (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/hof_furni/swf or (I) SWFCompiler/import/furniture.\n\
Output: SWFCompiler/furniture/nitro/. Skips files already converted.",
            ),
            MenuItem::op("11", "SWF Furniture to HAB", "tools.swf-furniture-hab").help(
                "Convert furniture .swf files to .hab (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/hof_furni/swf or (I) SWFCompiler/import/furniture.\n\
Output: SWFCompiler/furniture/hab/. Skips files already converted.",
            ),
            MenuItem::op("12", "Nitro Furniture to HAB", "tools.nitro-furniture-hab").help(
                "Repack furniture .nitro bundles as .hab with the same json; a PNG sheet becomes\n\
lossless WebP (every pixel checked). The .nitro files stay as they are.\n\
Reads SWFCompiler/furniture/nitro/ (or a custom folder). Output: SWFCompiler/furniture/hab/.",
            ),
            MenuItem::op("13", "SWF Clothes to Nitro", "tools.swf-clothes-nitro").help(
                "Convert clothes .swf files to .nitro (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/clothes/swf or (I) SWFCompiler/import/clothes.\n\
Skips hh_human_fx.swf (effects file).\n\
Output: SWFCompiler/clothes/nitro/. Skips files already converted.",
            ),
            MenuItem::op("14", "SWF Clothes to HAB", "tools.swf-clothes-hab").help(
                "Convert clothes .swf files to .hab (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/clothes/swf or (I) SWFCompiler/import/clothes.\n\
Skips hh_human_fx.swf (effects file).\n\
Output: SWFCompiler/clothes/hab/. Skips files already converted.",
            ),
            MenuItem::op("15", "Nitro Clothes to HAB", "tools.nitro-clothes-hab").help(
                "Repack clothes .nitro bundles as .hab with the same json; a PNG sheet becomes\n\
lossless WebP (every pixel checked). The .nitro files stay as they are.\n\
Reads SWFCompiler/clothes/nitro/ (or a custom folder). Output: SWFCompiler/clothes/hab/.",
            ),
            MenuItem::op("16", "SWF Pets to Nitro", "tools.swf-pets-nitro").help(
                "Convert pets .swf files to .nitro (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/pets/swf or (I) SWFCompiler/import/pets.\n\
Output: SWFCompiler/pets/nitro/. Skips files already converted.",
            ),
            MenuItem::op("17", "SWF Pets to HAB", "tools.swf-pets-hab").help(
                "Convert pets .swf files to .hab (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/pets/swf or (I) SWFCompiler/import/pets.\n\
Output: SWFCompiler/pets/hab/. Skips files already converted.",
            ),
            MenuItem::op("18", "Nitro Pets to HAB", "tools.nitro-pets-hab").help(
                "Repack pets .nitro bundles as .hab with the same json; a PNG sheet becomes\n\
lossless WebP (every pixel checked). The .nitro files stay as they are.\n\
Reads SWFCompiler/pets/nitro/ (or a custom folder). Output: SWFCompiler/pets/hab/.",
            ),
            MenuItem::op("19", "SWF Effects to Nitro", "tools.swf-effects-nitro").help(
                "Convert effects .swf files to .nitro (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/effects/swf or (I) SWFCompiler/import/effects.\n\
Custom XML can be dropped in SWFCompiler/import/effects/CustomXML/.\n\
Output: SWFCompiler/effects/nitro/. Skips files already converted.",
            ),
            MenuItem::op("20", "SWF Effects to HAB", "tools.swf-effects-hab").help(
                "Convert effects .swf files to .hab (json + WebP Lossless sheet).\n\
Source prompt: (H) Habbo_Default/effects/swf or (I) SWFCompiler/import/effects.\n\
Custom XML can be dropped in SWFCompiler/import/effects/CustomXML/.\n\
Output: SWFCompiler/effects/hab/. Skips files already converted.",
            ),
            MenuItem::op("21", "Nitro Effects to HAB", "tools.nitro-effects-hab").help(
                "Repack effects .nitro bundles as .hab with the same json; a PNG sheet becomes\n\
lossless WebP (every pixel checked). The .nitro files stay as they are.\n\
Reads SWFCompiler/effects/nitro/ (or a custom folder). Output: SWFCompiler/effects/hab/.",
            ),
            MenuItem::op("22", "Decompile SWF Files", "tools.decompile-swf").help(
                "Decompile SWF assets permanently to raw images, XMLs (binaryData),\n\
symbols (symbolClass), ActionScript (.as scripts), and audio.\n\
Reads from SWFCompiler/decompile/ (or import folders).\n\
Output: SWFCompiler/decompiled/<name>/.",
            ),
            MenuItem::op("23", "Add size 32 to HAB Furniture", "tools.hab-furniture-32").help(
                "Habbo's .hab furniture has no size 32 (the zoomed-out room). This copies the size 32\n\
sprites, assets and visualization from a .nitro (or .hab) of the same furni into it;\n\
the 64 graphics stay as they are. Only when both have the same 64 graphics.\n\
Reads Habbo_Default/hof_furni/hab/ and SWFCompiler/furniture/nitro/ (or custom folders).\n\
Output: SWFCompiler/furniture/hab/. Skips files already there.",
            ),
        ],
        false,
    )
    .await
}

pub async fn database() -> Result<()> {
    show(
        "Hotel Database Menu",
        vec![
            MenuItem::op("1", "Database General Information (version / databases)", "database.info").help(
                "Connects to the MariaDB / MySQL server defined in config.ini under\n\
[Database Settings] and prints: server version, list of databases\n\
with size in MB, current user table count. Read-only.\n\
Use this first to sanity-check that the credentials work before\n\
running any of the destructive options below.",
            ),
            MenuItem::op("2", "Optimize Database (OPTIMIZE TABLE on all tables)", "database.optimize").help(
                "Runs OPTIMIZE TABLE on every table of the configured database.\n\
This rebuilds indexes and reclaims fragmented space. Safe but slow\n\
on big tables (room_items, users_items, etc.). Best run during\n\
maintenance windows because OPTIMIZE locks each table while it\n\
rewrites it on disk.",
            ),
            MenuItem::op("3", "Fix Offer_ID in database and JSON", "database.offer-id").help(
                "Walks Generate/Variables/FurnitureData.json and aligns each\n\
items_base row's offer_id with the catalogue offer_id read from\n\
the JSON. Useful when offer_ids have drifted between your DB and\n\
the FurnitureData you actually serve to the client.\n\
Writes back into both the DB and the JSON (copy the JSON to the\n\
Nitro server afterwards).",
            ),
            MenuItem::op("4", "Fix Sit / Lay / Walk in items_base from JSON", "database.item-settings").help(
                "Reads canstandon / cansiton / canlayon from FurnitureData.json and\n\
writes them back into items_base.allow_walk / allow_sit / allow_lay\n\
for every matching classname. Run after adding new furniture so\n\
the emulator stops dropping users mid-room.",
            ),
            MenuItem::op("5", "Fix Sprite_ID + Item_IDS in items_base from JSON", "database.sprite-id").help(
                "Updates items_base.sprite_id and items_base.item_ids from the\n\
FurnitureData.json so the catalogue is in sync with the DB.\n\
RUN THIS BEFORE option 3 / 4 when you have just added new\n\
furniture - otherwise the offer_id / sit-lay-walk fixes have\n\
nothing to match against.",
            ),
            MenuItem::op("6", "Fix interaction count (SQL from Habbo_Default/hof_furni/hab)", "database.interaction-count").help(
                "Reads every floor and wall furni (type s or i; pets 'a0 ...' left out)\n\
with interaction_type 'default' and no multiheight, opens\n\
Habbo_Default/hof_furni/hab/<item_name>.hab and\n\
counts its interaction modes the same way Generate SQL does.\n\
Rows whose interaction_modes_count differs are written to\n\
Generate/Update_Interaction_count_<date>.sql; the .hab files that\n\
are missing (or unreadable) go to Generate/Missing_Hab_files_<date>.txt.\n\
The database is only read:\n\
check the file, then run it yourself.",
            ),
            MenuItem::op("7", "Fix width / length / height (SQL from Habbo_Default/hof_furni/hab)", "database.item-size").help(
                "Reads every floor and wall furni (type s or i; pets 'a0 ...' left out)\n\
without multiheight. A floor furni gets width / length / stack_height\n\
from logic.model.dimensions in Habbo_Default/hof_furni/hab/<item_name>.hab,\n\
the same way Generate SQL does; a wall furni is always 1 x 1 x 0.00.\n\
Rows that change are written to Generate/Update_Item_size_<date>.sql;\n\
the .hab files that are missing (or unreadable) go to\n\
Generate/Missing_Hab_files_<date>.txt. The database is only read:\n\
check the file, then run it yourself.",
            ),
        ],
        false,
    )
    .await
}
