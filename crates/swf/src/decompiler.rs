//! Port of `SwfDecompiler.DecompileAsync`: native export (images, binaryData, symbolClass) plus
//! an FFDec run for scripts and sounds, for every .swf of the chosen folder (recursive).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use futures::StreamExt;
use habbo_core::{console, outln};

fn base_decompile_dir() -> PathBuf {
    Path::new("SWFCompiler").join("decompile")
}

fn base_output_dir() -> PathBuf {
    Path::new("SWFCompiler").join("decompiled")
}

/// `SwfDecompiler.DecompileAsync`.
pub async fn decompile() -> Result<()> {
    if let Err(error) = run().await {
        outln!("❌ Error during SWF decompilation: {error}");
    }
    Ok(())
}

async fn run() -> Result<()> {
    outln!("=================================================");
    outln!("             SWF DECOMPILER                      ");
    outln!("=================================================");
    outln!("Extracts raw images, binaryData (XMLs), symbolClass,");
    outln!("ActionScript source code (.as), and sounds from SWF.");
    outln!("-------------------------------------------------");
    outln!("Options:");
    outln!(" [1] Auto: SWFCompiler/decompile/ (recursive)");
    outln!(" [2] Imported Furniture (SWFCompiler/import/furniture)");
    outln!(" [3] Imported Clothes (SWFCompiler/import/clothes)");
    outln!(" [4] Imported Effects (SWFCompiler/import/effects)");
    outln!(" [5] Imported Pets (SWFCompiler/import/pets)");
    outln!(" [6] Habbo Default hof_furni (Habbo_Default/hof_furni/swf)");
    outln!(" [7] Custom folder path");
    habbo_core::out!("Select source [Default is 1]: ");

    let choice = console::read_line_opt().map(|s| s.trim().to_string()).unwrap_or_else(|| "1".into());
    let import = |kind: &str| Path::new("SWFCompiler").join("import").join(kind);
    let source_dir = match choice.as_str() {
        "2" => import("furniture"),
        "3" => import("clothes"),
        "4" => import("effects"),
        "5" => import("pets"),
        "6" => habbo_core::paths::hof_furni::swf_source(),
        "7" => ask_custom_directory(),
        _ => base_decompile_dir(),
    };
    let source_label = source_dir.display().to_string();

    if !source_dir.is_dir() {
        if source_dir.as_os_str().is_empty() {
            // Directory.CreateDirectory("") threw this.
            bail!("The value cannot be an empty string. (Parameter 'path')");
        }
        std::fs::create_dir_all(&source_dir)?;
        outln!("📁 Created source folder: {source_label}");
        outln!("ℹ️ Please drop your .swf files into '{source_label}' and run this tool again.");
        return Ok(());
    }

    let swf_files: Vec<PathBuf> = habbo_core::paths::files_recursive(&source_dir)
        .into_iter()
        .filter(|p| habbo_core::paths::has_ext(p, "swf"))
        .collect();
    if swf_files.is_empty() {
        outln!("⚠️ No .swf files found in '{source_label}'.");
        outln!("ℹ️ Place your SWF files inside '{source_label}' to decompile them.");
        return Ok(());
    }

    outln!("🔍 Found {} SWF file(s) in {source_label}. Starting decompilation...", swf_files.len());
    std::fs::create_dir_all(base_output_dir())?;

    let stopwatch = Instant::now();
    let processors = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let max_parallelism = ((processors as f64 * 0.8) as usize).max(1);
    let decompiled = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    futures::stream::iter(swf_files.iter().cloned())
        .map(|swf_file| {
            let source_dir = source_dir.clone();
            let (decompiled, failed) = (&decompiled, &failed);
            async move {
                let relative = swf_file.strip_prefix(&source_dir).unwrap_or(&swf_file).to_path_buf();
                let relative_dir = relative.parent().map(Path::to_path_buf).unwrap_or_default();
                let swf_name = habbo_core::paths::file_stem(&swf_file);
                let target_folder = base_output_dir().join(relative_dir).join(swf_name);

                // C# let a CreateDirectory failure escape the worker (ending the whole run);
                // here it counts as a failed file.
                let ok = match std::fs::create_dir_all(&target_folder) {
                    Ok(()) => decompile_single_swf(&swf_file, &target_folder).await,
                    Err(error) => {
                        outln!("❌ Failed to decompile {}: {error}", habbo_core::paths::file_name(&swf_file));
                        false
                    }
                };
                if ok {
                    decompiled.fetch_add(1, Ordering::SeqCst);
                    outln!("✅ Decompiled: {} -> {}", habbo_core::paths::file_name(&swf_file), target_folder.display());
                } else {
                    failed.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
        .buffer_unordered(max_parallelism)
        .collect::<Vec<()>>()
        .await;

    habbo_core::summary::print_summary(
        "SWF Decompiler & Asset Extractor",
        swf_files.len(),
        decompiled.load(Ordering::SeqCst),
        0,
        failed.load(Ordering::SeqCst),
        0,
        0,
        stopwatch.elapsed(),
        &base_output_dir(),
        Some("Raw Sprites + XML + AS3 + Audio"),
    );
    Ok(())
}

fn ask_custom_directory() -> PathBuf {
    habbo_core::out!("Enter custom directory path: ");
    let input = console::read_line_opt().map(|s| s.trim().to_string());
    match input {
        Some(input) if !input.trim().is_empty() => PathBuf::from(input.trim_matches(['"', '\''])),
        _ => base_decompile_dir(),
    }
}

async fn decompile_single_swf(swf_file_path: &Path, output_dir: &Path) -> bool {
    // First: fast native extraction for images and binary data (any failure is left to FFDec).
    let (swf, out) = (swf_file_path.to_path_buf(), output_dir.to_path_buf());
    let _ = tokio::task::spawn_blocking(move || crate::native::extract(&swf, &out, true)).await;

    // Second: run FFDec to extract all scripts (ActionScript), sounds, and complete symbolClass.
    let args: Vec<std::ffi::OsString> = vec![
        "-onerror".into(),
        "ignore".into(),
        "-export".into(),
        "image,binarydata,symbolClass,script,sound".into(),
        output_dir.as_os_str().to_owned(),
        swf_file_path.as_os_str().to_owned(),
    ];
    // 90 seconds timeout; on timeout the process is killed. The exit code is not checked.
    match habbo_core::ffdec::run(args, Duration::from_millis(90_000)).await {
        Ok(_) => true,
        Err(error) => {
            outln!("❌ Failed to decompile {}: {error}", habbo_core::paths::file_name(swf_file_path));
            false
        }
    }
}
