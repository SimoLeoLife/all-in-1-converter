//! The Database menu (MySQL / MariaDB fixes on items_base and catalog_items), the SQL
//! generator for new furni and the .nitro / .hab file extractor it uses.

pub mod backup;
pub mod config;
pub mod connection_test;
pub mod fix_interaction_count;
pub mod fix_item_settings;
pub mod fix_item_size;
pub mod fix_sprite_id_settings;
pub mod general_info;
pub mod hab_furni_scan;
pub mod nitro_file_extractor;
pub mod optimizer;
pub mod set_offer_id;
pub mod sql_generator;

mod dotnet;
mod progress;
mod rows;

pub use connection_test::test_connection;
pub use sql_generator::{generate_sql, move_legacy_furniture_folder, FurnitureSqlTarget};
