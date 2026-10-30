//! Nitro and HAB asset bundles: reading, writing, PNG -> lossless WebP sheets, and the
//! Hotel Tools that work on whole bundle folders (compile/decompile, Nitro -> HAB,
//! size-32 merge).

pub mod bundle;
pub mod bundler;
pub mod hab;
pub mod reader;
pub mod sheet_webp;
pub mod writer;

/// A bundle entry: file name and bytes, in bundle order.
pub type FileEntry = (String, Vec<u8>);
