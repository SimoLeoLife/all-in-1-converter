//! Hotel Tools merge operations: FurnitureData, ProductData and FigureData/FigureMap merges
//! (Merge/Import_* + the original -> Merge/Merged_*).

pub mod compare_clothes_data;
pub mod compare_furnidata;
pub mod compare_product_data;
mod jtoken;

pub use compare_clothes_data::compare as merge_clothes;
pub use compare_furnidata::compare as merge_furnidata;
pub use compare_product_data::compare as merge_productdata;
