//! `SWF_Furni_Compiler` and `SWF_Pets_Compiler`: furniture and pet SWFs → .nitro / .hab
//! (json + sprite sheet), with the same json text, sheet layout and pixels as the C# tool.
//!
//! Operations: [`convert_furniture`] (`SWF_Furni_To_Nitro.ConvertSwfFilesAsync`) and
//! [`convert_pets`] (`SWF_Pets_To_Nitro.ConvertSwfFilesAsync`). The benchmark tool uses
//! [`ffdec_extractor::extract_swf`], [`assets`] (`AssetNameMapper`, `AssetsMapper`) and
//! [`spritesheet`] (`SpriteSheetMapper`, `ImageRestorer`).

pub mod assets;
mod common;
pub mod ffdec_extractor;
pub mod furni;
pub mod index;
pub mod logic;
pub mod net;
pub mod pets;
pub mod spritesheet;
pub mod stj;
pub mod visualizations;
pub mod xml;

pub use furni::convert_furniture;
pub use pets::convert_pets;

/// `FfdecExtractor` (furniture).
pub mod ffdec {
    pub use crate::ffdec_extractor::{extract_swf, rebuild_images_from_csv};
}

/// `SpriteSheetMapper` + `ImageRestorer`.
pub mod sprite_sheet_mapper {
    pub use crate::spritesheet::{
        clean_asset_name, force_cf_upper, generate_sprite_sheet, load_images, pack_sprite_sheet, restore_images_from_tmp,
        SpriteSheetData,
    };
}

/// `AssetsMapper` + `AssetNameMapper`.
pub mod assets_mapper {
    pub use crate::assets::{
        build_canonical_mapping, build_mappings_in_memory, latest_image_mapping, parse_assets_file, remove_first_prefix,
        remove_swf_prefix, Asset, IgnoreCaseMap, ParsedAssets,
    };
}
