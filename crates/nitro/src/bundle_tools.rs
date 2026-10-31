//! Port of `BundleTools`: Hotel Tools (de)compile of .nitro and .hab bundles:
//!   decompile  NitroCompiler/extract/<type>/{nitro,hab}/*  -> NitroCompiler/extracted/<type>/<name>/
//!   compile    NitroCompiler/compile/<type>/<name>/     -> NitroCompiler/compiled/<type>/{nitro,hab}/
//! Types: furni, clothing, effects, pets and generic (room, place_holder, ...).
//!
//! Also holds the folder-listing and thread-pool helpers the other Hotel Tools ports share.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use habbo_core::outln;
use habbo_core::workspace::{runtime, WorkspaceAssetKind};

use crate::{hab, nitro_to_hab, writer};

const ASSET_TYPES: [&str; 5] = ["furni", "clothing", "effects", "pets", "generic"];

/// `BundleTools.DecompileAsync(".nitro")`.
pub async fn decompile_nitro() -> Result<()> {
    decompile(".nitro").await
}

/// `BundleTools.DecompileAsync(".hab")`.
pub async fn decompile_hab() -> Result<()> {
    decompile(".hab").await
}

/// `BundleTools.CompileAsync(".nitro")`.
pub async fn compile_nitro() -> Result<()> {
    compile(".nitro").await
}

/// `BundleTools.CompileAsync(".hab")`.
pub async fn compile_hab() -> Result<()> {
    compile(".hab").await
}

/// `BundleTools.DecompileAsync(extension)`.
pub async fn decompile(extension: &str) -> Result<()> {
    outln!("Decompiling {extension} bundles...");
    let stopwatch = Instant::now();
    let (mut done, mut failed) = (0usize, 0usize);
    let (mut input_bytes, mut output_bytes) = (0u64, 0u64);
    let ext = extension.trim_start_matches('.');

    for asset_type in ASSET_TYPES {
        let extract_root = asset_directory(asset_type, Path::new("NitroCompiler").join("extract").join(asset_type));
        let input_folder = writer::folder_for(&extract_root, extension);
        std::fs::create_dir_all(&input_folder)?;

        // A file dropped straight into extract/<type>/ (the old layout) counts too.
        let mut files = files_with_ext(&input_folder, ext);
        if input_folder != extract_root {
            files.extend(files_with_ext(&extract_root, ext));
        }
        if files.is_empty() {
            continue;
        }

        outln!("Extracting {} {asset_type} files...", files.len());

        for file in &files {
            let name = habbo_core::paths::file_stem(file);
            let result = async {
                let data = tokio::fs::read(file).await?;
                let entries = if hab::is_hab(&data) { hab::read(&data)?.1 } else { nitro_to_hab::read_nitro(&data)? };

                let output_folder = Path::new("NitroCompiler").join("extracted").join(asset_type).join(&name);
                std::fs::create_dir_all(&output_folder)?;

                // The files exactly as stored in the bundle.
                for (entry_name, bytes) in &entries {
                    tokio::fs::write(output_folder.join(habbo_core::paths::file_name(Path::new(entry_name))), bytes).await?;
                    output_bytes += bytes.len() as u64;
                }

                input_bytes += data.len() as u64;
                anyhow::Ok(output_folder)
            }
            .await;
            match result {
                Ok(output_folder) => {
                    done += 1;
                    outln!("✅ Extracted: {name}{extension} -> {}", output_folder.display());
                }
                Err(error) => {
                    failed += 1;
                    outln!("❌ {name}{extension}: {error}");
                }
            }
        }
    }

    if done + failed == 0 {
        outln!("⚠️ No {extension} files found. Put them in NitroCompiler/extract/<type>/{ext}/");
        outln!("   Types: {}. The folders have been created for you.", ASSET_TYPES.join(", "));
        return Ok(());
    }

    habbo_core::summary::print_summary(
        &format!("Decompile {extension} bundles"),
        done + failed,
        done,
        0,
        failed,
        input_bytes,
        output_bytes,
        stopwatch.elapsed(),
        &Path::new("NitroCompiler").join("extracted"),
        Some("json + sheet"),
    );
    Ok(())
}

/// `BundleTools.CompileAsync(extension)`.
pub async fn compile(extension: &str) -> Result<()> {
    writer::set_extension(extension);
    outln!("Compiling {} bundles...", writer::label());

    let stopwatch = Instant::now();
    let (mut compiled, mut failed) = (0usize, 0usize);
    let mut output_bytes = 0u64;

    for asset_type in ASSET_TYPES {
        let compile_folder = Path::new("NitroCompiler").join("compile").join(asset_type);
        let output_folder = asset_directory(asset_type, Path::new("NitroCompiler").join("compiled").join(asset_type));

        std::fs::create_dir_all(&compile_folder)?;
        let items = directories(&compile_folder);
        if items.is_empty() {
            continue;
        }

        outln!("Compiling {} {asset_type} items...", items.len());

        for item_folder in items {
            let name = habbo_core::paths::file_name(&item_folder);
            let output_folder = output_folder.clone();
            let task_name = name.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<u64> {
                let files = writer::read_files(&files_in(&item_folder))?;
                writer::write(&output_folder, &task_name, files, Some(&|path: &Path, data: &[u8]| habbo_core::workspace::write_output(path, data)))
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|r| r);
            match result {
                Ok(bytes) => {
                    output_bytes += bytes;
                    compiled += 1;
                }
                Err(error) => {
                    failed += 1;
                    outln!("❌ Error compiling {name} ({asset_type}): {error}");
                }
            }
        }
    }

    if compiled + failed == 0 {
        outln!("⚠️ Nothing to compile. Put one folder per asset (json + sheet) in NitroCompiler/compile/<type>/<name>/");
        outln!("   Types: {}. The folders have been created for you.", ASSET_TYPES.join(", "));
        return Ok(());
    }

    habbo_core::summary::print_summary(
        &format!("Compile {extension} bundles"),
        compiled + failed,
        compiled,
        0,
        failed,
        0,
        output_bytes,
        stopwatch.elapsed(),
        &Path::new("NitroCompiler").join("compiled"),
        Some(&writer::label()),
    );
    Ok(())
}

/// The Asset Workspace folder of a type when one is set up, else the local folder.
fn asset_directory(asset_type: &str, fallback: PathBuf) -> PathBuf {
    let kind = match asset_type {
        "furni" => WorkspaceAssetKind::Furniture,
        "clothing" => WorkspaceAssetKind::Clothing,
        "effects" => WorkspaceAssetKind::Effects,
        "pets" => WorkspaceAssetKind::Pets,
        _ => return fallback,
    };
    runtime::router().asset_directory(kind, fallback)
}

// ---------------------------------------------------------------- shared helpers

/// Directory entries in the order NTFS returns them (`Directory.GetFiles` on Windows):
/// case-insensitive by name.
fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(dir).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    entries.sort_by_cached_key(|p| (habbo_core::paths::file_name(p).to_uppercase(), habbo_core::paths::file_name(p)));
    entries
}

/// `Directory.GetFiles(dir)`.
pub(crate) fn files_in(dir: &Path) -> Vec<PathBuf> {
    sorted_entries(dir).into_iter().filter(|p| p.is_file()).collect()
}

/// `Directory.GetFiles(dir, "*.ext")` (extension without the dot, case-insensitive).
pub(crate) fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    files_in(dir).into_iter().filter(|p| habbo_core::paths::has_ext(p, ext)).collect()
}

/// `Directory.GetDirectories(dir)`.
pub(crate) fn directories(dir: &Path) -> Vec<PathBuf> {
    sorted_entries(dir).into_iter().filter(|p| p.is_dir()).collect()
}

/// `Directory.GetFiles(dir, "*", SearchOption.AllDirectories)`: .NET walks breadth-first,
/// a folder's files before the files of its subfolders.
pub(crate) fn files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut queue = VecDeque::from([dir.to_path_buf()]);
    while let Some(current) = queue.pop_front() {
        for entry in sorted_entries(&current) {
            if entry.is_dir() {
                queue.push_back(entry);
            } else {
                out.push(entry);
            }
        }
    }
    out
}

/// `Math.Max(1, Environment.ProcessorCount - 1)`.
pub(crate) fn processors_minus_one() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).saturating_sub(1).max(1)
}

/// `Parallel.ForEachAsync(items, MaxDegreeOfParallelism = threads, body)` for CPU-bound bodies:
/// a rayon pool of `threads` workers, run off the async runtime.
pub(crate) async fn for_each_parallel<T, F>(items: Vec<T>, threads: usize, body: F) -> Result<()>
where
    T: Send + Sync + 'static,
    F: Fn(&T) + Send + Sync + 'static,
{
    use rayon::prelude::*;
    let body = Arc::new(body);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
        pool.install(|| items.par_iter().for_each(|item| body(item)));
        Ok(())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_is_breadth_first_and_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a").join("deep")).unwrap();
        std::fs::write(dir.path().join("b.hab"), b"").unwrap();
        std::fs::write(dir.path().join("A.HAB"), b"").unwrap();
        std::fs::write(dir.path().join("a").join("c.hab"), b"").unwrap();
        std::fs::write(dir.path().join("a").join("deep").join("d.hab"), b"").unwrap();
        let names: Vec<String> = files_recursive(dir.path()).iter().map(|p| habbo_core::paths::file_name(p)).collect();
        assert_eq!(names, ["A.HAB", "b.hab", "c.hab", "d.hab"]);
        assert_eq!(files_with_ext(dir.path(), "hab").len(), 2);
        assert_eq!(directories(dir.path()).len(), 1);
    }

    #[tokio::test]
    async fn parallel_runs_every_item() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        for_each_parallel((0..10).collect(), 3, move |_| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
        .await
        .unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 10);
    }
}
