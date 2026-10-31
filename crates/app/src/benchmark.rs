//! `BenchmarkRunner` (--benchmark): converts a spread of furniture SWFs to PNG and to lossless
//! WebP sheets, checks every pixel matches, and projects the saving over the whole catalogue.

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use habbo_core::console::n0;
use habbo_core::{outln, paths::hof_furni, settings};
use habbo_furni::{assets, ffdec_extractor, spritesheet};
use image::RgbaImage;
use indexmap::IndexMap;

pub async fn run(sample_count: usize) -> Result<()> {
    outln!("===============================================================================");
    outln!("        🚀 HABBO ALL-IN-1: WEBP LOSSLESS BENCHMARK & TEST SUITE                ");
    outln!("===============================================================================");

    let source_dir = hof_furni::swf_source();
    if !source_dir.is_dir() {
        outln!("❌ Source directory '{}' does not exist!", source_dir.display());
        return Ok(());
    }
    let all_swf_files = habbo_core::paths::files_with_ext(&source_dir, "swf");
    if all_swf_files.is_empty() {
        outln!("❌ No SWF files found in '{}'!", source_dir.display());
        return Ok(());
    }

    outln!("📦 Total SWF files in {}: {}", source_dir.display(), n0(all_swf_files.len() as i64));
    outln!("🔬 Benchmarking {sample_count} diverse furniture assets (PNG vs WebP Lossless)...\n");

    // A spread across the size range: every n-th file ordered by size.
    let mut by_size: Vec<_> = all_swf_files.iter().map(|f| (std::fs::metadata(f).map(|m| m.len()).unwrap_or(0), f.clone())).collect();
    by_size.sort_by_key(|(len, _)| *len);
    let step = (all_swf_files.len() / sample_count.max(1)).max(1);
    let sample_files: Vec<_> =
        by_size.into_iter().enumerate().filter(|(i, _)| i % step == 0).map(|(_, (_, f))| f).take(sample_count).collect();

    let (mut total_png_sheet, mut total_webp_sheet) = (0u64, 0u64);
    let (mut pixel_perfect, mut tested_pixel) = (0usize, 0usize);
    let mut rows = 0usize;
    let clock = Instant::now();

    let work_dir = std::env::temp_dir().join(format!("habbo_benchmark_{}", uuid_like()));
    std::fs::create_dir_all(&work_dir)?;

    for (i, swf_file) in sample_files.iter().enumerate() {
        let name = habbo_core::paths::file_stem(swf_file);
        let furni_dir = work_dir.join(&name);
        let binary_dir = furni_dir.join(format!("{name}_binaryData"));
        std::fs::create_dir_all(&furni_dir)?;
        let tag = format!("[{:02}/{:02}]", i + 1, sample_files.len());

        let result: Result<Option<(u64, u64, bool)>> = async {
            // 1. Extract the SWF assets
            ffdec_extractor::extract_swf(swf_file, &binary_dir).await?;
            if !binary_dir.join("binaryData").is_dir() {
                return Ok(None);
            }
            let canonical = assets::build_canonical_mapping(&binary_dir.join("symbolClass").join("symbols.csv"));
            let images_dir = binary_dir.join("images");
            spritesheet::restore_images_from_tmp(&binary_dir.join("tmp"), &images_dir, &assets::latest_image_mapping());
            let images = load_images(&images_dir);
            if images.is_empty() {
                return Ok(None);
            }

            // 2. PNG sheet, 3. WebP sheet
            let sheet = |format: &str, folder: &str| -> Result<Option<std::path::PathBuf>> {
                settings::set_spritesheet_format(format);
                let dir = furni_dir.join(folder);
                std::fs::create_dir_all(&dir)?;
                Ok(spritesheet::generate_sprite_sheet(&images, &dir, &name, &canonical, false, 10, 14000, 14000)?.map(|(path, _)| path))
            };
            let png_path = sheet("png", "png_out")?;
            let webp_path = sheet("webp", "webp_out")?;
            let size = |p: &Option<std::path::PathBuf>| p.as_ref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
            let (png_size, webp_size) = (size(&png_path), size(&webp_path));

            // 4. Bit-for-bit RGBA comparison
            let mut pixel_match = true;
            if let (Some(png), Some(webp)) = (&png_path, &webp_path) {
                let a = habbo_core::imaging::decode_rgba(&std::fs::read(png)?)?;
                let b = habbo_core::imaging::decode_rgba(&std::fs::read(webp)?)?;
                tested_pixel += 1;
                pixel_match = a.dimensions() == b.dimensions() && a.as_raw() == b.as_raw();
                if pixel_match {
                    pixel_perfect += 1;
                }
            }
            Ok(Some((png_size, webp_size, pixel_match)))
        }
        .await;

        match result {
            Ok(Some((png_size, webp_size, pixel_match))) => {
                total_png_sheet += png_size;
                total_webp_sheet += webp_size;
                rows += 1;
                let saving = if png_size > 0 { (1.0 - webp_size as f64 / png_size as f64) * 100.0 } else { 0.0 };
                outln!(
                    "  {tag} {name:<28} | PNG: {:6.1} KB -> WebP: {:6.1} KB | -{saving:5.1}% | {}",
                    png_size as f64 / 1024.0,
                    webp_size as f64 / 1024.0,
                    if pixel_match { "✅ 100% Lossless RGBA" } else { "❌ Diff" }
                );
            }
            Ok(None) => {}
            Err(error) => outln!("  {tag} {name:<28} | ❌ Error: {error}"),
        }
    }

    let elapsed = clock.elapsed();
    settings::set_spritesheet_format("webp");

    let mb = |b: f64| b / (1024.0 * 1024.0);
    let rule = "===============================================================================";
    outln!("\n{rule}");
    outln!("                            📊 BENCHMARK RESULTS                               ");
    outln!("{rule}");
    outln!("Sample furniture tested:      {rows}");
    outln!("Total Sample PNG size:        {:.2} MB", mb(total_png_sheet as f64));
    outln!("Total Sample WebP size:       {:.2} MB", mb(total_webp_sheet as f64));
    let saved = total_png_sheet as f64 - total_webp_sheet as f64;
    let saved_pct = if total_png_sheet > 0 { saved / total_png_sheet as f64 * 100.0 } else { 0.0 };
    outln!("Texture Space Saved:          {:.2} MB (-{saved_pct:.1}%)", mb(saved));
    let perfect_pct = if tested_pixel > 0 { pixel_perfect as f64 / tested_pixel as f64 * 100.0 } else { 100.0 };
    outln!("Pixel Perfection Check:       {pixel_perfect}/{tested_pixel} ({perfect_pct:.1}% bit-for-bit exact RGBA match)");
    outln!(
        "Speed:                        {:.2}s total ({:.0} ms/item)",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / rows.max(1) as f64
    );

    // Projection for the whole catalogue.
    let total = all_swf_files.len() as f64;
    let avg = |bytes: u64| if rows > 0 { bytes as f64 / rows as f64 } else { 0.0 };
    let png_total = mb(avg(total_png_sheet) * total);
    let webp_total = mb(avg(total_webp_sheet) * total);
    outln!("\n{rule}");
    outln!("      🔮 PROJECTION FOR FULL CATALOG ({} FURNITURE ITEMS)      ", n0(all_swf_files.len() as i64));
    outln!("{rule}");
    outln!("Full Catalog with PNG:        ~{png_total:.1} MB (~{:.2} GB)", png_total / 1024.0);
    outln!("Full Catalog with WebP:       ~{webp_total:.1} MB (~{:.2} GB)", webp_total / 1024.0);
    outln!(
        "⚡ Total Bandwidth & Disk Saved: ~{:.1} MB (~{:.2} GB Saved! -{saved_pct:.1}%)",
        png_total - webp_total,
        (png_total - webp_total) / 1024.0
    );
    outln!("{rule}\n");

    let _ = std::fs::remove_dir_all(&work_dir);
    Ok(())
}

/// Every *.png in the folder (sh_ included, unlike the converters), in directory order.
fn load_images(dir: &Path) -> IndexMap<String, RgbaImage> {
    let mut images = IndexMap::new();
    for file in habbo_furni::net::get_files(dir, "*.png").unwrap_or_default() {
        if let Ok(image) = std::fs::read(&file).map_err(anyhow::Error::from).and_then(|b| habbo_core::imaging::decode_rgba(&b)) {
            images.insert(habbo_core::paths::file_stem(&file), image);
        }
    }
    images
}

fn uuid_like() -> String {
    format!("{:x}{:x}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())
}
