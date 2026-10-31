//! What `SWF_Furni_To_Nitro` and `SWF_Pets_To_Nitro` share: the `Parallel.ForEachAsync`
//! loop with its progress lines and summary, and small per-file helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use futures::StreamExt;
use habbo_core::console::{reset_color, set_color, Color};
use habbo_core::outln;
use habbo_nitro::writer;

use crate::net;

/// The per-file worker of [`run_loop`].
pub(crate) type ProcessFn = Arc<dyn Fn(&Path) -> Result<bool> + Send + Sync>;

pub(crate) struct LoopOptions {
    pub process_title: String,
    pub max_parallelism: usize,
    /// Print every N items...
    pub progress_every: usize,
    /// ...and the first N.
    pub progress_first: usize,
    /// `{current,W}` width.
    pub counter_width: usize,
}

/// The body of `ConvertSwfFilesAsync` after the file list is known. `process` is the
/// blocking `ProcessSwfFileAsync`; an `Err` from it is an exception escaping the loop.
pub(crate) async fn run_loop(
    swf_files: Vec<PathBuf>,
    output_directory: PathBuf,
    options: LoopOptions,
    process: ProcessFn,
) -> Result<()> {
    let total_files = swf_files.len();
    let processed = Arc::new(AtomicUsize::new(0));
    let converted = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let original_bytes = Arc::new(AtomicU64::new(0));
    let output_bytes = Arc::new(AtomicU64::new(0));
    let first_error: Arc<Mutex<Option<anyhow::Error>>> = Arc::new(Mutex::new(None));
    let stopwatch = Instant::now();
    let counter_width = options.counter_width;
    let progress_every = options.progress_every;
    let progress_first = options.progress_first;

    let tasks = swf_files.into_iter().map(|swf_file| {
        let output_directory = output_directory.clone();
        let process = process.clone();
        let (processed, converted, skipped, failed) = (processed.clone(), converted.clone(), skipped.clone(), failed.clone());
        let (original_bytes, output_bytes) = (original_bytes.clone(), output_bytes.clone());
        let first_error = first_error.clone();
        async move {
            if first_error.lock().unwrap().is_some() {
                return;
            }
            let joined = tokio::task::spawn_blocking(move || -> Result<()> {
                let name = habbo_core::paths::file_stem(&swf_file);
                let target = writer::output_path(&output_directory, &name);
                let length = std::fs::metadata(&swf_file).map(|m| m.len()).unwrap_or(0);
                original_bytes.fetch_add(length, Ordering::SeqCst);

                if writer::exists(&output_directory, &name) {
                    skipped.fetch_add(1, Ordering::SeqCst);
                    if let Ok(meta) = std::fs::metadata(&target) {
                        output_bytes.fetch_add(meta.len(), Ordering::SeqCst);
                    }
                } else {
                    let ok = process(&swf_file)?;
                    if ok && target.is_file() {
                        converted.fetch_add(1, Ordering::SeqCst);
                        if let Ok(meta) = std::fs::metadata(&target) {
                            output_bytes.fetch_add(meta.len(), Ordering::SeqCst);
                        }
                    } else {
                        failed.fetch_add(1, Ordering::SeqCst);
                    }
                }

                let current = processed.fetch_add(1, Ordering::SeqCst) + 1;
                if current % progress_every == 0 || current == total_files || current <= progress_first {
                    let pct = current as f64 / total_files as f64 * 100.0;
                    let per_sec = current as f64 / stopwatch.elapsed().as_secs_f64().max(0.001);
                    outln!(
                        "⚡ [{current:>counter_width$}/{total_files}] ({pct:>5.1}%) | {per_sec:>5.1} items/sec | Converted: {} | Skipped: {}",
                        converted.load(Ordering::SeqCst),
                        skipped.load(Ordering::SeqCst)
                    );
                }
                Ok(())
            })
            .await;
            let outcome = match joined {
                Ok(result) => result,
                Err(join_error) => Err(anyhow::anyhow!("{join_error}")),
            };
            if let Err(error) = outcome {
                let mut slot = first_error.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(error);
                }
            }
        }
    });

    futures::stream::iter(tasks).buffer_unordered(options.max_parallelism.max(1)).collect::<Vec<()>>().await;

    if let Some(error) = first_error.lock().unwrap().take() {
        return Err(error);
    }

    let elapsed = stopwatch.elapsed();
    let label = writer::label();
    habbo_core::summary::print_summary(
        &options.process_title.replace("{label}", &label),
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

/// `Math.Max(2, (int)(Environment.ProcessorCount * factor))`.
pub(crate) fn max_parallelism(factor: f64) -> usize {
    2.max((net::processor_count() as f64 * factor) as usize)
}

/// `Directory.GetFiles(binaryData, pattern).FirstOrDefault()`.
pub(crate) fn first_file(dir: &Path, pattern: &str) -> Result<Option<PathBuf>> {
    Ok(net::get_files(dir, pattern)?.into_iter().next())
}

/// The yellow "No *_x.bin file found" line.
pub(crate) fn warn_missing(pattern: &str, binary_output_path: &Path, what: &str) {
    set_color(Color::Yellow);
    outln!("No {pattern} file found in {}. Continuing without {what}.", binary_output_path.display());
    reset_color();
}

/// `DeleteDirectory`.
pub(crate) fn delete_directory(path: &Path) {
    if path.is_dir() {
        if let Err(error) = std::fs::remove_dir_all(path) {
            outln!("❌ Error deleting directory {}: {error}", path.display());
        }
    }
}

/// `BundleNitroFileAsync`: json + sheet into the .nitro/.hab bundle.
pub(crate) fn bundle_nitro_file(file_output_directory: &Path, file_name: &str, nitro_output_directory: &Path, sprite_sheet_path: &Path) -> Result<()> {
    let files = writer::read_files(&[file_output_directory.join(format!("{file_name}.json")), sprite_sheet_path.to_path_buf()])?;
    writer::write(nitro_output_directory, file_name, files, None)?;
    Ok(())
}

/// Runs an async step from a blocking worker thread (the per-file work runs in
/// `spawn_blocking`, like the CPU-bound body of the C# `Parallel.ForEachAsync`).
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(future),
        Err(_) => tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime").block_on(future),
    }
}
