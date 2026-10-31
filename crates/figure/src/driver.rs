//! The `Parallel.ForEachAsync` loop shared by both compilers: skip rules, counters, progress
//! lines and the final summary.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use futures::StreamExt;
use habbo_core::outln;
use habbo_nitro::writer;

pub(crate) struct LoopOptions {
    pub title: &'static str,
    /// The clothes compiler counts `hh_human_fx.swf` as skipped.
    pub skip_human_fx: bool,
    pub progress_every: usize,
    pub counter_width: usize,
}

pub(crate) fn is_human_fx(swf_file: &Path) -> bool {
    habbo_core::paths::file_name(swf_file).eq_ignore_ascii_case("hh_human_fx.swf")
}

/// `Math.Max(2, (int)(Environment.ProcessorCount * 0.9))`.
pub(crate) fn max_parallelism() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    2.max((cpus as f64 * 0.9) as usize)
}

/// Runs `process(swf_file)` for every file not yet converted. An `Err` from `process` is an
/// exception escaping the C# loop body: the loop stops and the error reaches the caller.
pub(crate) async fn run<F, Fut>(swf_files: Vec<PathBuf>, output_directory: PathBuf, options: LoopOptions, process: F) -> Result<()>
where
    F: Fn(PathBuf) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<bool>>,
{
    let total_files = swf_files.len();
    let processed = Arc::new(AtomicUsize::new(0));
    let converted = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let original_bytes = Arc::new(AtomicU64::new(0));
    let output_bytes = Arc::new(AtomicU64::new(0));
    let stopwatch = Instant::now();
    let process = Arc::new(process);
    let output_directory = Arc::new(output_directory);
    let handle = tokio::runtime::Handle::current();
    let options = Arc::new(options);

    let mut results = futures::stream::iter(swf_files)
        .map(|swf_file| {
            let (processed, converted, skipped, failed) =
                (processed.clone(), converted.clone(), skipped.clone(), failed.clone());
            let (original_bytes, output_bytes) = (original_bytes.clone(), output_bytes.clone());
            let (process, output_directory, handle, options) =
                (process.clone(), output_directory.clone(), handle.clone(), options.clone());
            // Each file runs on its own blocking thread (native SWF parsing and image work are
            // CPU bound); FFDec waits are driven by the runtime through block_on.
            tokio::task::spawn_blocking(move || -> Result<()> {
                let name = habbo_core::paths::file_stem(&swf_file);
                let target = writer::output_path(&output_directory, &name);
                let swf_length = std::fs::metadata(&swf_file).map(|m| m.len()).unwrap_or(0);
                original_bytes.fetch_add(swf_length, Ordering::SeqCst);
                let file_size = |path: &Path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

                if options.skip_human_fx && is_human_fx(&swf_file) {
                    skipped.fetch_add(1, Ordering::SeqCst);
                } else if writer::exists(&output_directory, &name) {
                    skipped.fetch_add(1, Ordering::SeqCst);
                    output_bytes.fetch_add(file_size(&target), Ordering::SeqCst);
                } else {
                    let ok = handle.block_on((process.as_ref())(swf_file.clone()))?;
                    if ok && target.is_file() {
                        converted.fetch_add(1, Ordering::SeqCst);
                        output_bytes.fetch_add(file_size(&target), Ordering::SeqCst);
                    } else {
                        failed.fetch_add(1, Ordering::SeqCst);
                    }
                }

                let current = processed.fetch_add(1, Ordering::SeqCst) + 1;
                if current % options.progress_every == 0 || current == total_files || current <= 10 {
                    let pct = current as f64 / total_files as f64 * 100.0;
                    let per_sec = current as f64 / stopwatch.elapsed().as_secs_f64().max(0.001);
                    outln!(
                        "⚡ [{current:>width$}/{total_files}] ({pct:>5.1}%) | {per_sec:>5.1} items/sec | Converted: {} | Skipped: {}",
                        converted.load(Ordering::SeqCst),
                        skipped.load(Ordering::SeqCst),
                        width = options.counter_width
                    );
                }
                Ok(())
            })
        })
        .buffer_unordered(max_parallelism());

    while let Some(result) = results.next().await {
        result.map_err(|e| anyhow!(e.to_string()))??;
    }
    drop(results);

    let elapsed = stopwatch.elapsed();
    let label = writer::label();
    habbo_core::summary::print_summary(
        &format!("{} -> {label}", options.title),
        total_files,
        converted.load(Ordering::SeqCst),
        skipped.load(Ordering::SeqCst),
        failed.load(Ordering::SeqCst),
        original_bytes.load(Ordering::SeqCst),
        output_bytes.load(Ordering::SeqCst),
        elapsed,
        &writer::folder(&output_directory),
        Some(&label),
    );
    Ok(())
}

/// `DeleteDirectory`: removes the per-file work folder, printing (not failing) on error.
pub(crate) fn delete_directory(directory: &Path) {
    if directory.is_dir() {
        if let Err(error) = std::fs::remove_dir_all(directory) {
            outln!("❌ Error deleting directory {}: {error}", directory.display());
        }
    }
}

/// `LoadImages`: every top-level PNG, keyed by file name (first one wins), in directory order.
pub(crate) fn load_images(images_directory: &Path) -> Result<Vec<(String, image::RgbaImage)>> {
    let mut images: Vec<(String, image::RgbaImage)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for image_file in crate::util::get_files(images_directory, "*.png")? {
        let image_name = habbo_core::paths::file_stem(&image_file);
        if seen.contains(&image_name) {
            continue;
        }
        match std::fs::read(&image_file).map_err(anyhow::Error::from).and_then(|b| habbo_core::imaging::decode_rgba(&b)) {
            Ok(image) => {
                seen.insert(image_name.clone());
                images.push((image_name, image));
            }
            Err(error) => outln!("⚠️ Error loading image {}: {error}", image_file.display()),
        }
    }
    Ok(images)
}

/// `GetAssetDataAsync`'s manifest lookup: the first `*_manifest.*` in binaryData.
pub(crate) fn find_manifest(binary_output_path: &Path) -> Result<Option<PathBuf>> {
    let binary_data_path = binary_output_path.join("binaryData");
    let manifests = crate::util::get_files(&binary_data_path, "*_manifest.*")?;
    if manifests.is_empty() {
        outln!("❌ Manifest file not found in {}", binary_data_path.display());
        return Ok(None);
    }
    Ok(manifests.into_iter().next())
}

/// `BundleNitroFileAsync`: json + sheet -> .nitro / .hab.
pub(crate) fn bundle_nitro_file(
    output_directory: &Path,
    file_name: &str,
    nitro_output_directory: &Path,
    sprite_sheet_path: Option<&Path>,
) -> Result<()> {
    let mut paths = vec![output_directory.join(format!("{file_name}.json"))];
    if let Some(sheet) = sprite_sheet_path {
        paths.push(sheet.to_path_buf());
    }
    let files = writer::read_files(&paths)?;
    writer::write(nitro_output_directory, file_name, files, None)?;
    Ok(())
}
