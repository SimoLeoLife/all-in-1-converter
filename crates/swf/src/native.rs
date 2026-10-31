//! Port of `NativeSwfExtractor.Extract` / `CompareExports`: a minimal SWF tag reader that writes
//! FFDec's `-export image,binarydata,symbolClass` layout (images/, binaryData/,
//! symbolClass/symbols.csv). Anything it cannot handle returns `Ok(false)` (FFDec fallback);
//! malformed data that made the C# code throw returns `Err`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use flate2::{Decompress, FlushDecompress, Status};
use habbo_core::outln;
use image::RgbaImage;

/// The message .NET gave for an out-of-range array index (the C# code let it escape).
const OUT_OF_RANGE: &str = "Index was outside the bounds of the array.";

pub fn extract(swf_file_path: &Path, output_directory: &Path, include_images: bool) -> Result<bool> {
    let Some(body) = read_body(swf_file_path)? else { return Ok(false) };

    let mut pos = 0usize;
    skip_header_rect(&body, &mut pos)?;
    pos += 4; // frame rate (u16) + frame count (u16)

    // Pass 1: collect the raw tags we care about.
    let mut symbols: Vec<(u16, String)> = Vec::new();
    let mut binary_tags: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut bitmap_tags: Vec<(u16, Vec<u8>)> = Vec::new();

    while pos + 2 <= body.len() {
        let code_and_length = read_u16(&body, &mut pos)?;
        let code = code_and_length >> 6;
        let mut length = (code_and_length & 0x3F) as usize;
        if length == 0x3F {
            if pos + 4 > body.len() {
                return Ok(false);
            }
            // NOTE(port): C# casts the u32 to int; a length >= 2^31 went negative there and threw.
            let long_length = read_u32(&body, &mut pos)?;
            if long_length > i32::MAX as u32 {
                bail!("Non-negative number required.");
            }
            length = long_length as usize;
        }
        if pos + length > body.len() {
            return Ok(false);
        }

        if code == 0 {
            break; // End tag
        }

        match code {
            76 => {
                // SymbolClass
                let mut p = pos;
                let count = read_u16(&body, &mut p)?;
                for _ in 0..count {
                    let tag_id = read_u16(&body, &mut p)?;
                    let name = read_null_terminated(&body, &mut p);
                    symbols.push((tag_id, name));
                }
            }
            87 => {
                // DefineBinaryData
                let mut p = pos;
                let char_id = read_u16(&body, &mut p)?;
                p += 4; // reserved u32
                let end = pos + length;
                let data = if p < end { body[p..end].to_vec() } else { Vec::new() };
                binary_tags.push((char_id, data));
            }
            // DefineBitsLossless, DefineBitsLossless2, DefineBitsJPEG2, DefineBitsJPEG3
            20 | 36 | 21 | 35 => {
                if include_images {
                    bitmap_tags.push((code, body[pos..pos + length].to_vec()));
                }
            }
            // DefineBits (needs JPEGTables), DefineBitsJPEG4
            6 | 90 if include_images => {
                return Ok(false); // let FFDEC handle these rare formats
            }
            _ => {}
        }

        pos += length;
    }

    // Name lookups: every symbol name pointing at each character id, in tag order.
    let mut names_by_id: HashMap<u16, Vec<String>> = HashMap::new();
    for (id, name) in &symbols {
        names_by_id.entry(*id).or_default().push(name.clone());
    }

    // Pass 2: write the export tree.
    let images_dir = if include_images { reset_directory(&output_directory.join("images"))? } else { PathBuf::new() };
    let binary_dir = reset_directory(&output_directory.join("binaryData"))?;
    let symbol_dir = reset_directory(&output_directory.join("symbolClass"))?;

    std::fs::write(symbol_dir.join("symbols.csv"), symbols_csv(&symbols))?;

    for (char_id, data) in &binary_tags {
        let name = match names_by_id.get(char_id).and_then(|names| names.first()) {
            Some(first) => format!("{char_id}_{first}"),
            None => char_id.to_string(),
        };
        std::fs::write(binary_dir.join(format!("{name}.bin")), data)?;
    }

    for (code, payload) in &bitmap_tags {
        let mut p = 0;
        let char_id = read_u16(payload, &mut p)?;

        let image = match code {
            20 => decode_lossless(payload, false)?,
            36 => decode_lossless(payload, true)?,
            21 => decode_jpeg2(payload),
            35 => decode_jpeg3(payload)?,
            _ => None,
        };
        let Some(image) = image else { return Ok(false) };

        let names = names_by_id.get(&char_id).map(Vec::as_slice).unwrap_or(&[]);
        let file_name = if names.len() == 1 { format!("{char_id}_{}.png", names[0]) } else { format!("{char_id}.png") };
        std::fs::write(images_dir.join(file_name), habbo_core::imaging::encode_png(&image)?)?;
    }

    Ok(true)
}

/// `File.WriteAllLines` of `id;"name"` records: UTF-8 without BOM, a newline after each line.
pub(crate) fn symbols_csv(symbols: &[(u16, String)]) -> String {
    let mut out = String::new();
    for (id, name) in symbols {
        out.push_str(&format!("{id};\"{name}\""));
        out.push_str(habbo_core::paths::NEWLINE);
    }
    out
}

// ---------------------------------------------------------------
// Container / primitives
// ---------------------------------------------------------------

/// The uncompressed SWF body after the 8-byte header; `None` for formats left to FFDec.
pub(crate) fn read_body(swf_file_path: &Path) -> Result<Option<Vec<u8>>> {
    let raw = std::fs::read(swf_file_path)?;
    body_of(&raw)
}

pub(crate) fn body_of(raw: &[u8]) -> Result<Option<Vec<u8>>> {
    if raw.len() < 9 {
        return Ok(None);
    }
    match &raw[..3] {
        b"FWS" => Ok(Some(raw[8..].to_vec())),
        b"CWS" => Ok(Some(inflate(raw, 8)?)),
        _ => Ok(None), // ZWS (LZMA) and everything else -> FFDEC fallback
    }
}

fn skip_header_rect(body: &[u8], pos: &mut usize) -> Result<()> {
    let first = *body.get(*pos).ok_or_else(|| anyhow::anyhow!(OUT_OF_RANGE))?;
    let nbits = (first >> 3) as usize;
    let total_bits = 5 + 4 * nbits;
    *pos += total_bits.div_ceil(8);
    Ok(())
}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16> {
    if *pos + 2 > data.len() {
        bail!(OUT_OF_RANGE);
    }
    let value = u16::from_le_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(value)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32> {
    if *pos + 4 > data.len() {
        bail!(OUT_OF_RANGE);
    }
    let value = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(value)
}

fn read_byte(data: &[u8], pos: &mut usize) -> Result<u8> {
    let value = *data.get(*pos).ok_or_else(|| anyhow::anyhow!(OUT_OF_RANGE))?;
    *pos += 1;
    Ok(value)
}

fn read_null_terminated(data: &[u8], pos: &mut usize) -> String {
    let start = (*pos).min(data.len());
    while *pos < data.len() && data[*pos] != 0 {
        *pos += 1;
    }
    let value = String::from_utf8_lossy(&data[start..(*pos).min(data.len())]).into_owned();
    *pos += 1; // consume terminator
    value
}

/// `ZLibStream` over `data[offset..]`: header and checksum errors fail, but a stream cut short
/// just ends (like .NET's `CopyTo`), and bytes after the end of the stream are ignored.
pub(crate) fn inflate(data: &[u8], offset: usize) -> Result<Vec<u8>> {
    if offset > data.len() {
        bail!("Offset and length were out of bounds for the array or count is greater than the number of elements from index to the end of the source collection.");
    }
    let input = &data[offset..];
    let mut decompress = Decompress::new(true);
    let mut output: Vec<u8> = Vec::with_capacity(input.len().saturating_mul(4).max(64));
    loop {
        if output.len() == output.capacity() {
            output.reserve(output.capacity().max(64));
        }
        let before_in = decompress.total_in();
        let before_out = decompress.total_out();
        let status = decompress
            .decompress_vec(&input[before_in as usize..], &mut output, FlushDecompress::None)
            .map_err(|e| anyhow::anyhow!("The archive entry was compressed using an unsupported compression method. ({e})"))?;
        if status == Status::StreamEnd {
            break;
        }
        let progressed = decompress.total_in() != before_in || decompress.total_out() != before_out;
        if !progressed && output.len() < output.capacity() {
            break; // input exhausted before the end of the stream
        }
    }
    Ok(output)
}

fn reset_directory(path: &Path) -> Result<PathBuf> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    }
    std::fs::create_dir_all(path)?;
    Ok(path.to_path_buf())
}

// ---------------------------------------------------------------
// Bitmap tag decoding
// ---------------------------------------------------------------

pub(crate) fn decode_lossless(payload: &[u8], has_alpha: bool) -> Result<Option<RgbaImage>> {
    let mut p = 2; // skip charId
    let format = read_byte(payload, &mut p)?;
    let width = read_u16(payload, &mut p)? as usize;
    let height = read_u16(payload, &mut p)? as usize;
    if width == 0 || height == 0 {
        return Ok(None);
    }

    let mut palette_count = 0usize;
    if format == 3 {
        palette_count = read_byte(payload, &mut p)? as usize + 1;
    }

    let data = inflate(payload, p)?;
    let mut pixels = vec![0u8; width * height * 4];

    match format {
        3 => {
            let entry_size = if has_alpha { 4 } else { 3 };
            let palette_bytes = palette_count * entry_size;
            let stride = (width + 3) & !3;
            if data.len() < palette_bytes + stride * height {
                return Ok(None);
            }
            for y in 0..height {
                for x in 0..width {
                    let mut index = data[palette_bytes + y * stride + x] as usize;
                    if index >= palette_count {
                        index = 0;
                    }
                    let e = index * entry_size;
                    let (r, g, b) = (data[e], data[e + 1], data[e + 2]);
                    let a = if has_alpha { data[e + 3] } else { 255 };
                    write_pixel(&mut pixels, (y * width + x) * 4, r, g, b, a, has_alpha);
                }
            }
        }
        5 => {
            let stride = width * 4;
            if data.len() < stride * height {
                return Ok(None);
            }
            for y in 0..height {
                for x in 0..width {
                    let s = y * stride + x * 4;
                    // stored as [A|reserved], R, G, B
                    let a = if has_alpha { data[s] } else { 255 };
                    let (r, g, b) = (data[s + 1], data[s + 2], data[s + 3]);
                    write_pixel(&mut pixels, (y * width + x) * 4, r, g, b, a, has_alpha);
                }
            }
        }
        4 if !has_alpha => {
            let stride = (width * 2 + 3) & !3;
            if data.len() < stride * height {
                return Ok(None);
            }
            for y in 0..height {
                for x in 0..width {
                    let s = y * stride + x * 2;
                    let pix = ((data[s] as u32) << 8) | data[s + 1] as u32; // PIX15 is big-endian
                    let r = expand5((pix >> 10) & 0x1F);
                    let g = expand5((pix >> 5) & 0x1F);
                    let b = expand5(pix & 0x1F);
                    write_pixel(&mut pixels, (y * width + x) * 4, r, g, b, 255, false);
                }
            }
        }
        _ => return Ok(None),
    }

    Ok(RgbaImage::from_raw(width as u32, height as u32, pixels))
}

fn expand5(value: u32) -> u8 {
    ((value << 3) | (value >> 2)) as u8
}

fn write_pixel(pixels: &mut [u8], offset: usize, mut r: u8, mut g: u8, mut b: u8, a: u8, premultiplied: bool) {
    if premultiplied {
        if a == 0 {
            r = 0;
            g = 0;
            b = 0;
        } else if a != 255 {
            let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            r = un(r);
            g = un(g);
            b = un(b);
        }
    }
    pixels[offset] = r;
    pixels[offset + 1] = g;
    pixels[offset + 2] = b;
    pixels[offset + 3] = a;
}

/// `Image.Load<Rgba32>`: format sniffed from the bytes (JPEG, or PNG/GIF in SWF 8+); `None` on failure.
fn load_image(bytes: &[u8]) -> Option<RgbaImage> {
    habbo_core::imaging::decode_rgba(bytes).ok()
}

fn decode_jpeg2(payload: &[u8]) -> Option<RgbaImage> {
    let data = if payload.len() > 2 { &payload[2..] } else { &[][..] };
    load_image(strip_to_jpeg_start(data))
}

fn decode_jpeg3(payload: &[u8]) -> Result<Option<RgbaImage>> {
    let mut p = 2;
    let alpha_offset = read_u32(payload, &mut p)?;
    // NOTE(port): C# read the offset as int; a value >= 2^31 went negative there (Take() gave nothing).
    let alpha_offset = if alpha_offset > i32::MAX as u32 { 0 } else { alpha_offset as usize };
    if p + alpha_offset > payload.len() {
        return Ok(None);
    }

    let Some(mut image) = load_image(strip_to_jpeg_start(&payload[p..p + alpha_offset])) else { return Ok(None) };

    let alpha = inflate(payload, p + alpha_offset)?;
    let (width, height) = (image.width() as usize, image.height() as usize);
    if alpha.len() < width * height {
        return Ok(None);
    }
    for (index, pixel) in image.pixels_mut().enumerate() {
        pixel.0[3] = alpha[index];
    }
    Ok(Some(image))
}

/// Flash-era JPEG data may be prefixed with a spurious EOI/SOI pair.
fn strip_to_jpeg_start(data: &[u8]) -> &[u8] {
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0xFF && data[i + 1] == 0xD8 && data[i + 2] == 0xFF {
            return &data[i..];
        }
        i += 1;
    }
    data
}

// ---------------------------------------------------------------
// Verification against FFDEC (CONFURTER_VERIFY_NATIVE_SWF=1)
// ---------------------------------------------------------------

pub fn compare_exports(swf_name: &str, native_directory: &Path, ffdec_directory: &Path) {
    let mut issues: Vec<String> = Vec::new();

    // symbols.csv: same (id, name) multiset.
    let mut native_symbols = read_csv_pairs(&native_directory.join("symbolClass").join("symbols.csv"));
    let mut ffdec_symbols = read_csv_pairs(&ffdec_directory.join("symbolClass").join("symbols.csv"));
    let (native_count, ffdec_count) = (native_symbols.len(), ffdec_symbols.len());
    native_symbols.sort();
    ffdec_symbols.sort();
    if native_symbols != ffdec_symbols {
        issues.push(format!("symbols.csv differs (native {native_count} vs ffdec {ffdec_count} records)"));
    }

    // binaryData: same file names and identical bytes.
    issues.extend(compare_files(native_directory, ffdec_directory, "binaryData", "bin", |a, b| {
        Ok(if std::fs::read(a)? == std::fs::read(b)? { None } else { Some("bytes differ".to_string()) })
    }));

    // images: same file names, pixel-equal within ±1 (premultiply rounding).
    issues.extend(compare_files(native_directory, ffdec_directory, "images", "png", |a, b| {
        let ia = habbo_core::imaging::decode_rgba(&std::fs::read(a)?)?;
        let ib = habbo_core::imaging::decode_rgba(&std::fs::read(b)?)?;
        if ia.dimensions() != ib.dimensions() {
            return Ok(Some(format!("size {}x{} vs {}x{}", ia.width(), ia.height(), ib.width(), ib.height())));
        }
        let off = ia
            .pixels()
            .zip(ib.pixels())
            .filter(|(pa, pb)| !(pa.0[3] == 0 && pb.0[3] == 0))
            .filter(|(pa, pb)| (0..4).any(|c| (pa.0[c] as i32 - pb.0[c] as i32).abs() > 1))
            .count();
        Ok(if off == 0 { None } else { Some(format!("{off} pixels differ by more than 1")) })
    }));

    if issues.is_empty() {
        outln!("✅ [verify] {swf_name}: native export matches FFDEC.");
    } else {
        outln!("❌ [verify] {swf_name}: {} difference(s):", issues.len());
        for issue in &issues {
            outln!("   - {issue}");
        }
    }
}

fn read_csv_pairs(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| l.trim().replace('"', "")).collect()
}

fn file_names(dir: &Path, ext: &str) -> BTreeSet<String> {
    habbo_core::paths::files_with_ext(dir, ext).iter().map(|p| habbo_core::paths::file_name(p)).collect()
}

fn compare_files(
    native_directory: &Path,
    ffdec_directory: &Path,
    sub_dir: &str,
    ext: &str,
    comparer: impl Fn(&Path, &Path) -> Result<Option<String>>,
) -> Vec<String> {
    let a = native_directory.join(sub_dir);
    let b = ffdec_directory.join(sub_dir);
    let a_files = file_names(&a, ext);
    let b_files = file_names(&b, ext);
    let mut out = Vec::new();

    for missing in b_files.difference(&a_files) {
        out.push(format!("{sub_dir}/{missing}: missing in native export"));
    }
    for extra in a_files.difference(&b_files) {
        out.push(format!("{sub_dir}/{extra}: only in native export"));
    }
    for name in a_files.intersection(&b_files) {
        let problem = match comparer(&a.join(name), &b.join(name)) {
            Ok(problem) => problem,
            Err(error) => Some(format!("compare failed: {error}")),
        };
        if let Some(problem) = problem {
            out.push(format!("{sub_dir}/{name}: {problem}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn tag(code: u16, payload: &[u8], long: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if long || payload.len() >= 0x3F {
            out.extend_from_slice(&((code << 6) | 0x3F).to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        } else {
            out.extend_from_slice(&((code << 6) | payload.len() as u16).to_le_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    /// A tiny SWF body: RECT (nbits 0 -> 1 byte), frame rate, frame count, then the tags.
    fn body() -> Vec<u8> {
        let mut body = vec![0x00, 0x00, 0x18, 0x01, 0x00];

        // DefineBinaryData id 1: reserved u32 then the bytes.
        let mut binary = 1u16.to_le_bytes().to_vec();
        binary.extend_from_slice(&[0, 0, 0, 0]);
        binary.extend_from_slice(b"<xml/>");
        body.extend(tag(87, &binary, false));

        // DefineBitsLossless2 id 2: 2x1 ARGB (format 5), premultiplied.
        let mut bitmap = 2u16.to_le_bytes().to_vec();
        bitmap.push(5);
        bitmap.extend_from_slice(&2u16.to_le_bytes());
        bitmap.extend_from_slice(&1u16.to_le_bytes());
        bitmap.extend(zlib(&[255, 10, 20, 30, 128, 64, 32, 0]));
        body.extend(tag(36, &bitmap, true));

        // DefineBinaryData id 3 with no symbol name.
        let mut unnamed = 3u16.to_le_bytes().to_vec();
        unnamed.extend_from_slice(&[0, 0, 0, 0, 9]);
        body.extend(tag(87, &unnamed, false));

        // SymbolClass: 1 -> chair_assets, 2 -> chair_icon, 0 -> main.
        let mut symbols = 3u16.to_le_bytes().to_vec();
        for (id, name) in [(1u16, "chair_assets"), (2, "chair_icon"), (0, "main")] {
            symbols.extend_from_slice(&id.to_le_bytes());
            symbols.extend_from_slice(name.as_bytes());
            symbols.push(0);
        }
        body.extend(tag(76, &symbols, false));
        body.extend(tag(0, &[], false));
        body
    }

    fn swf(compressed: bool) -> Vec<u8> {
        let body = body();
        let mut out = if compressed { b"CWS".to_vec() } else { b"FWS".to_vec() };
        out.push(10);
        out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        if compressed {
            out.extend(zlib(&body))
        } else {
            out.extend(body)
        }
        out
    }

    fn check_export(compressed: bool) {
        let dir = tempfile::tempdir().unwrap();
        let swf_path = dir.path().join("chair.swf");
        std::fs::write(&swf_path, swf(compressed)).unwrap();
        let out = dir.path().join("out");
        // A stale file is removed by the reset.
        std::fs::create_dir_all(out.join("images")).unwrap();
        std::fs::write(out.join("images").join("old.png"), b"x").unwrap();

        assert!(extract(&swf_path, &out, true).unwrap());

        let nl = habbo_core::paths::NEWLINE;
        assert_eq!(
            std::fs::read_to_string(out.join("symbolClass").join("symbols.csv")).unwrap(),
            format!("1;\"chair_assets\"{nl}2;\"chair_icon\"{nl}0;\"main\"{nl}")
        );
        assert_eq!(std::fs::read(out.join("binaryData").join("1_chair_assets.bin")).unwrap(), b"<xml/>");
        assert_eq!(std::fs::read(out.join("binaryData").join("3.bin")).unwrap(), [9]);
        assert!(!out.join("images").join("old.png").exists());

        let png = std::fs::read(out.join("images").join("2_chair_icon.png")).unwrap();
        let image = habbo_core::imaging::decode_rgba(&png).unwrap();
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.get_pixel(0, 0).0, [10, 20, 30, 255]);
        // 64,32,0 at alpha 128 un-premultiplied: (c*255 + 64) / 128.
        assert_eq!(image.get_pixel(1, 0).0, [128, 64, 0, 128]);
    }

    #[test]
    fn exports_uncompressed_fws() {
        check_export(false);
    }

    #[test]
    fn exports_zlib_cws() {
        check_export(true);
    }

    #[test]
    fn binary_only_skips_images() {
        let dir = tempfile::tempdir().unwrap();
        let swf_path = dir.path().join("a.swf");
        std::fs::write(&swf_path, swf(true)).unwrap();
        let out = dir.path().join("out");
        assert!(extract(&swf_path, &out, false).unwrap());
        assert!(!out.join("images").exists());
        assert!(out.join("binaryData").join("1_chair_assets.bin").is_file());
    }

    #[test]
    fn unsupported_inputs_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.swf");
        std::fs::write(&path, b"ZWS\x0a\0\0\0\0\0\0\0").unwrap();
        assert!(!extract(&path, dir.path(), true).unwrap());

        // DefineBitsJPEG4 -> FFDec when images are wanted.
        let mut raw = b"FWS\x0a\0\0\0\0".to_vec();
        raw.extend([0x00, 0x00, 0x18, 0x01, 0x00]);
        raw.extend(tag(90, &[1, 0], false));
        std::fs::write(&path, &raw).unwrap();
        assert!(!extract(&path, &dir.path().join("o"), true).unwrap());
        assert!(extract(&path, &dir.path().join("o"), false).unwrap());

        // A tag longer than the file.
        let mut raw = b"FWS\x0a\0\0\0\0".to_vec();
        raw.extend([0x00, 0x00, 0x18, 0x01, 0x00]);
        raw.extend_from_slice(&((87u16 << 6) | 10).to_le_bytes());
        raw.push(1);
        std::fs::write(&path, &raw).unwrap();
        assert!(!extract(&path, &dir.path().join("p"), true).unwrap());
    }

    #[test]
    fn palette_and_rgb15_lossless() {
        // DefineBitsLossless format 3: 2-colour RGB palette, 3x1 (stride 4).
        let mut payload = 7u16.to_le_bytes().to_vec();
        payload.push(3);
        payload.extend_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.push(1);
        payload.extend(zlib(&[1, 2, 3, 4, 5, 6, 0, 1, 9, 0]));
        let image = decode_lossless(&payload, false).unwrap().unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [1, 2, 3, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [4, 5, 6, 255]);
        assert_eq!(image.get_pixel(2, 0).0, [1, 2, 3, 255]); // index out of range -> 0

        // format 4: one PIX15 pixel, pure red.
        let mut payload = 7u16.to_le_bytes().to_vec();
        payload.push(4);
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend(zlib(&[0x7C, 0x00, 0, 0]));
        assert_eq!(decode_lossless(&payload, false).unwrap().unwrap().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert!(decode_lossless(&payload, true).unwrap().is_none());
    }

    #[test]
    fn inflate_tolerates_truncation() {
        let data = zlib(&[7u8; 1000]);
        assert_eq!(inflate(&data, 0).unwrap().len(), 1000);
        let cut = inflate(&data[..data.len() - 4], 0).unwrap();
        assert!(cut.len() <= 1000);
        assert!(inflate(&[1, 2, 3], 0).is_err());
        assert!(inflate(&[], 0).unwrap().is_empty());
    }

    #[test]
    fn compare_reports_differences() {
        let dir = tempfile::tempdir().unwrap();
        let swf_path = dir.path().join("a.swf");
        std::fs::write(&swf_path, swf(false)).unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        assert!(extract(&swf_path, &a, true).unwrap());
        assert!(extract(&swf_path, &b, true).unwrap());
        assert!(compare_files(&a, &b, "binaryData", "bin", |_, _| Ok(None)).is_empty());
        std::fs::write(b.join("binaryData").join("x.bin"), b"").unwrap();
        let issues = compare_files(&a, &b, "binaryData", "bin", |_, _| Ok(None));
        assert_eq!(issues, vec!["binaryData/x.bin: missing in native export".to_string()]);
        compare_exports("a.swf", &a, &b);
    }
}
