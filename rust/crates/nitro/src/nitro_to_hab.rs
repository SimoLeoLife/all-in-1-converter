//! Port of `NitroToHabConverter`: Hotel Tools "Nitro ... to HAB" repacks the .nitro bundles of one
//! asset type as .hab, with the same json and a lossless WebP sheet (a PNG sheet is converted,
//! pixel-checked). The .nitro files are left untouched.
//! Default: SWFCompiler/<type>/nitro -> SWFCompiler/<type>/hab.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Result};
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use habbo_core::workspace::{runtime, WorkspaceAssetKind};
use habbo_core::{console, out, outln};

use crate::{writer, FileEntry};

/// `NitroToHabConverter.FurnitureAsync`.
pub async fn furniture() -> Result<()> {
    convert("Furniture", WorkspaceAssetKind::Furniture, Path::new("SWFCompiler").join("furniture")).await
}

/// `NitroToHabConverter.ClothesAsync`.
pub async fn clothes() -> Result<()> {
    convert("Clothes", WorkspaceAssetKind::Clothing, Path::new("SWFCompiler").join("clothes")).await
}

/// `NitroToHabConverter.PetsAsync`.
pub async fn pets() -> Result<()> {
    convert("Pets", WorkspaceAssetKind::Pets, Path::new("SWFCompiler").join("pets")).await
}

/// `NitroToHabConverter.EffectsAsync`.
pub async fn effects() -> Result<()> {
    convert("Effects", WorkspaceAssetKind::Effects, Path::new("SWFCompiler").join("effects")).await
}

async fn convert(label: &str, kind: WorkspaceAssetKind, fallback: PathBuf) -> Result<()> {
    let base_dir = runtime::router().asset_directory(kind, fallback);
    let default_source = writer::folder_for(&base_dir, ".nitro");

    outln!("Nitro {label} to HAB: json + WebP Lossless, output {}", writer::folder_for(&base_dir, ".hab").display());
    outln!(" [1] {}", default_source.display());
    outln!(" [2] Custom folder path");
    out!("Select source [Default is 1]: ");

    let mut source_dir = default_source;
    if console::read_line_opt().map(|s| s.trim().to_string()).as_deref() == Some("2") {
        out!("Enter custom directory path: ");
        if let Some(input) = console::read_line_opt() {
            let input = input.trim().trim_matches(['"', '\'']).to_string();
            if !input.trim().is_empty() {
                source_dir = PathBuf::from(input);
            }
        }
    }

    writer::set_extension(".hab");
    convert_directory(label, &source_dir, &base_dir).await
}

async fn convert_directory(label: &str, source_dir: &Path, base_dir: &Path) -> Result<()> {
    if !source_dir.is_dir() {
        std::fs::create_dir_all(source_dir)?;
        outln!("📁 Created source folder: {}", source_dir.display());
        outln!("ℹ️ Drop your .nitro files into '{}' and run this tool again.", source_dir.display());
        return Ok(());
    }

    let nitro_files: Vec<PathBuf> =
        crate::bundle_tools::files_recursive(source_dir).into_iter().filter(|p| habbo_core::paths::has_ext(p, "nitro")).collect();
    if nitro_files.is_empty() {
        outln!("⚠️ No .nitro files found in '{}'.", source_dir.display());
        return Ok(());
    }

    let output_dir = writer::folder_for(base_dir, ".hab");
    std::fs::create_dir_all(&output_dir)?;

    let stopwatch = Instant::now();
    let counters = Arc::new(Counters::default());

    let out_dir = output_dir.clone();
    let c = counters.clone();
    crate::bundle_tools::for_each_parallel(nitro_files.clone(), crate::bundle_tools::processors_minus_one(), move |nitro_file: &PathBuf| {
        let name = habbo_core::paths::file_stem(nitro_file);
        let hab_path = out_dir.join(format!("{name}.hab"));
        if hab_path.is_file() {
            c.skipped.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let result = (|| -> Result<(u64, u64)> {
            let nitro = std::fs::read(nitro_file)?;
            let hab = writer::build(".hab", &name, read_nitro(&nitro)?)?;
            habbo_core::workspace::write_output(&hab_path, &hab)?;
            Ok((nitro.len() as u64, hab.len() as u64))
        })();
        match result {
            Ok((input, output)) => {
                c.converted.fetch_add(1, Ordering::SeqCst);
                c.original_bytes.fetch_add(input, Ordering::SeqCst);
                c.output_bytes.fetch_add(output, Ordering::SeqCst);
            }
            Err(error) => {
                c.failed.fetch_add(1, Ordering::SeqCst);
                outln!("❌ {name}.nitro: {error}");
            }
        }
    })
    .await?;

    habbo_core::summary::print_summary(
        &format!("Nitro {label} -> HAB"),
        nitro_files.len(),
        counters.converted.load(Ordering::SeqCst),
        counters.skipped.load(Ordering::SeqCst),
        counters.failed.load(Ordering::SeqCst),
        counters.original_bytes.load(Ordering::SeqCst),
        counters.output_bytes.load(Ordering::SeqCst),
        stopwatch.elapsed(),
        &output_dir,
        Some(&writer::label()),
    );
    Ok(())
}

#[derive(Default)]
struct Counters {
    converted: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    original_bytes: AtomicU64,
    output_bytes: AtomicU64,
}

/// `NitroToHabConverter.ReadNitro`: every file of a .nitro; a damaged bundle errors instead of
/// giving a .hab with files missing.
pub fn read_nitro(data: &[u8]) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    let mut position = 0usize;

    let count = read_u16_be(data, &mut position)?;
    for _ in 0..count {
        let name_length = read_u16_be(data, &mut position)? as usize;
        let file_name = String::from_utf8_lossy(take(data, &mut position, name_length)?).into_owned();

        let length = read_i32_be(data, &mut position)?;
        if length < 0 {
            bail!("Invalid length for {file_name}.");
        }
        let bytes = decompress(take(data, &mut position, length as usize)?)?;
        files.push((file_name, bytes));
    }

    if files.is_empty() {
        bail!("The .nitro bundle is empty.");
    }
    Ok(files)
}

fn read_u16_be(data: &[u8], position: &mut usize) -> Result<u16> {
    let bytes = take(data, position, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i32_be(data: &[u8], position: &mut usize) -> Result<i32> {
    let bytes = take(data, position, 4)?;
    Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn take<'a>(data: &'a [u8], position: &mut usize, length: usize) -> Result<&'a [u8]> {
    if *position + length > data.len() {
        bail!("The .nitro bundle is cut short.");
    }
    let bytes = &data[*position..*position + length];
    *position += length;
    Ok(bytes)
}

/// Nitro bundles hold gzip (our converter) or zlib (the original Nitro tools) streams.
fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    if data.len() > 1 && data[0] == 0x1F && data[1] == 0x8B {
        GzDecoder::new(data).read_to_end(&mut output)?;
    } else if !data.is_empty() && data[0] == 0x78 {
        ZlibDecoder::new(data).read_to_end(&mut output)?;
    } else {
        DeflateDecoder::new(data).read_to_end(&mut output)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn entry(out: &mut Vec<u8>, name: &str, stored: &[u8]) {
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(stored.len() as i32).to_be_bytes());
        out.extend_from_slice(stored);
    }

    #[test]
    fn reads_gzip_zlib_and_raw_entries() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"one").unwrap();
        let mut zl = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zl.write_all(b"two").unwrap();
        let mut raw = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::none());
        raw.write_all(b"three").unwrap();

        let mut data = 3u16.to_be_bytes().to_vec();
        entry(&mut data, "a.json", &gz.finish().unwrap());
        entry(&mut data, "a.png", &zl.finish().unwrap());
        entry(&mut data, "b.txt", &raw.finish().unwrap());

        let files = read_nitro(&data).unwrap();
        assert_eq!(files, vec![
            ("a.json".to_string(), b"one".to_vec()),
            ("a.png".to_string(), b"two".to_vec()),
            ("b.txt".to_string(), b"three".to_vec()),
        ]);
        assert!(read_nitro(&data[..data.len() - 1]).is_err());
        assert!(read_nitro(&[0, 0]).is_err());

        // The existing NitroBundler output reads back.
        let mut bundler = crate::bundler::NitroBundler::new();
        bundler.add_file("x.json", b"{}".to_vec()).unwrap();
        assert_eq!(read_nitro(&bundler.to_buffer().unwrap()).unwrap()[0].1, b"{}");
    }
}
