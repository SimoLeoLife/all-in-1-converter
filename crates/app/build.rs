//! Zips assets/ffdec into the binary as Tools/ffdec/… (the original C# build embedded it as
//! `EmbeddedTools.zip`), so the published exe needs no Tools/ folder beside it.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    // The exe icon and version information (windres on the GNU toolchain, rc.exe on MSVC).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/app.rc");
        println!("cargo:rerun-if-changed=resources/app.ico");
        embed_resource::compile("resources/app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let tools = manifest.join("../../assets");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("EmbeddedTools.zip");
    println!("cargo:rerun-if-changed={}", tools.display());

    let mut zip = zip::ZipWriter::new(File::create(&out).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let ffdec = tools.join("ffdec");
    if ffdec.is_dir() {
        add_dir(&mut zip, &tools, &ffdec, options);
    } else {
        println!("cargo:warning=assets/ffdec not found; the binary will not carry FFDec.");
    }
    zip.finish().unwrap();
}

fn add_dir(zip: &mut zip::ZipWriter<File>, base: &Path, dir: &Path, options: zip::write::SimpleFileOptions) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            add_dir(zip, base, &path, options);
            continue;
        }
        let relative = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        let mut bytes = Vec::new();
        File::open(&path).unwrap().read_to_end(&mut bytes).unwrap();
        zip.start_file(format!("Tools/{relative}"), options).unwrap();
        zip.write_all(&bytes).unwrap();
    }
}
