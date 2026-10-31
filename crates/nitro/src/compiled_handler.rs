//! Port of `CompiledHandler`: lists or deletes every file under NitroCompiler/compiled.

use std::path::{Path, PathBuf};

use anyhow::Result;
use habbo_core::outln;

fn compiled_folder() -> PathBuf {
    Path::new("NitroCompiler").join("compiled")
}

/// `CompiledHandler.ListFiles`.
pub async fn list_files() -> Result<()> {
    let folder = compiled_folder();
    if !folder.is_dir() {
        outln!("Compiled folder does not exist.");
        return Ok(());
    }

    let files = crate::bundle_tools::files_recursive(&folder);
    if files.is_empty() {
        outln!("No files found in the compiled folder.");
        return Ok(());
    }

    outln!("Files in compiled folder:");
    for file in &files {
        outln!("{}", file.strip_prefix(&folder).unwrap_or(file).display());
    }
    Ok(())
}

/// `CompiledHandler.ClearFiles`. Folders are kept; only files are deleted.
pub async fn clear_files() -> Result<()> {
    let folder = compiled_folder();
    if !folder.is_dir() {
        outln!("Compiled folder does not exist.");
        return Ok(());
    }

    let files = crate::bundle_tools::files_recursive(&folder);
    if files.is_empty() {
        outln!("No files found in the compiled folder.");
        return Ok(());
    }

    for file in &files {
        std::fs::remove_file(file)?;
        outln!("Deleted: {}", file.display());
    }

    outln!("All files in the compiled folder have been deleted.");
    Ok(())
}
