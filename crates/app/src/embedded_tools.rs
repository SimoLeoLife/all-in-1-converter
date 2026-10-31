//! Unpacks the FFDec bundle embedded in the binary into ./Tools/ next to the executable,
//! once. Every path (e.g. Tools/ffdec/ffdec-cli.exe) then works without indirection.

use std::io::Cursor;
use std::path::Path;

static EMBEDDED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/EmbeddedTools.zip"));
const SENTINEL: &str = "Tools/ffdec/ffdec-cli.jar";

pub fn ensure_extracted() {
    let Some(base) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else { return };
    if base.join(SENTINEL).is_file() {
        return; // already unpacked on this machine
    }

    let mut archive = match zip::ZipArchive::new(Cursor::new(EMBEDDED)) {
        Ok(archive) if !archive.is_empty() => archive,
        _ => {
            eprintln!(
                "[embedded-tools] the FFDec bundle is missing from this build. Drop a Tools/ folder next to the binary manually."
            );
            return;
        }
    };
    if let Err(error) = archive.extract(&base) {
        eprintln!("[embedded-tools] extraction failed: {error}");
    }
}
