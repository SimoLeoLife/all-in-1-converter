//! STUB: replaced by the port of NativeSwfExtractor.cs. Until then every SWF falls back to FFDec.

use std::path::Path;

use anyhow::Result;

pub fn extract(_swf_file_path: &Path, _output_directory: &Path, _include_images: bool) -> Result<bool> {
    Ok(false)
}

pub fn compare_exports(_swf_name: &str, _native_directory: &Path, _ffdec_directory: &Path) {}
