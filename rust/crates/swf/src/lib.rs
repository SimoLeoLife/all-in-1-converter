//! The built-in SWF reader (`NativeSwfExtractor`): exports images, binaryData and the
//! symbolClass CSV in FFDec's folder layout, so FFDec is only needed as a fallback.

use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::Result;
use habbo_core::outln;

mod native;

const DISABLE_ENV_VAR: &str = "CONFURTER_DISABLE_NATIVE_SWF";
const VERIFY_ENV_VAR: &str = "CONFURTER_VERIFY_NATIVE_SWF";

/// Exports the SWF into `output_directory` the way FFDec's
/// `-export image,binarydata,symbolClass` does. `Ok(false)` when this reader cannot
/// handle the file (the caller falls back to FFDec). `include_images: false` skips bitmap
/// decoding for callers that only read binaryData (the SQL generator).
pub fn extract(swf_file_path: &Path, output_directory: &Path, include_images: bool) -> Result<bool> {
    native::extract(swf_file_path, output_directory, include_images)
}

/// Native extraction, falling back to `ffdec_runner(output_directory)` when it fails.
/// `CONFURTER_DISABLE_NATIVE_SWF=1` always uses FFDec; `CONFURTER_VERIFY_NATIVE_SWF=1`
/// also runs FFDec into `<out>_ffdec_verify` and reports differences.
pub async fn extract_with_fallback<F, Fut>(
    swf_file_path: &Path,
    output_directory: &Path,
    ffdec_runner: F,
    include_images: bool,
) -> Result<()>
where
    F: Fn(PathBuf) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if std::env::var(DISABLE_ENV_VAR).as_deref() == Ok("1") {
        return ffdec_runner(output_directory.to_path_buf()).await;
    }

    let native_ok = match extract(swf_file_path, output_directory, include_images) {
        Ok(ok) => ok,
        Err(error) => {
            outln!(
                "⚠️ Native SWF extraction failed for {} ({error}); falling back to FFDEC.",
                habbo_core::paths::file_name(swf_file_path)
            );
            false
        }
    };

    if !native_ok {
        return ffdec_runner(output_directory.to_path_buf()).await;
    }

    if std::env::var(VERIFY_ENV_VAR).as_deref() == Ok("1") {
        let trimmed = output_directory.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
        let verify_directory = PathBuf::from(format!("{trimmed}_ffdec_verify"));
        if verify_directory.is_dir() {
            std::fs::remove_dir_all(&verify_directory)?;
        }
        std::fs::create_dir_all(&verify_directory)?;
        ffdec_runner(verify_directory.clone()).await?;
        native::compare_exports(&habbo_core::paths::file_name(swf_file_path), output_directory, &verify_directory);
    }
    Ok(())
}
