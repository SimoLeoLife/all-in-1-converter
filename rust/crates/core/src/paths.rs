//! `HabboAssetFolder` / `HofFurniPaths`: the Habbo_Default asset folders (relative to the
//! working directory, like the C# version).

use std::path::{Path, PathBuf};

#[cfg(windows)]
pub const NEWLINE: &str = "\r\n";
#[cfg(not(windows))]
pub const NEWLINE: &str = "\n";

/// A Habbo_Default asset folder: swf/ for the .swf files and hab/ for the .hab files.
#[derive(Debug, Clone, Copy)]
pub struct HabboAssetFolder {
    name: &'static str,
}

impl HabboAssetFolder {
    pub const FURNITURE: Self = Self { name: "hof_furni" };
    pub const CLOTHES: Self = Self { name: "clothes" };
    pub const EFFECTS: Self = Self { name: "effects" };
    pub const PETS: Self = Self { name: "pets" };

    pub fn root(&self) -> PathBuf {
        Path::new("Habbo_Default").join(self.name)
    }

    pub fn swf(&self) -> PathBuf {
        self.root().join("swf")
    }

    pub fn hab(&self) -> PathBuf {
        self.root().join("hab")
    }

    /// The .swf folder; the root itself for a download made before the swf/ folder existed.
    pub fn swf_source(&self) -> PathBuf {
        let swf = self.swf();
        if has_files_with_ext(&swf, "swf") { swf } else { self.root() }
    }
}

/// Habbo_Default/hof_furni: swf/, hab/ and icons/.
pub mod hof_furni {
    use super::*;

    pub fn root() -> PathBuf {
        HabboAssetFolder::FURNITURE.root()
    }
    pub fn swf() -> PathBuf {
        HabboAssetFolder::FURNITURE.swf()
    }
    pub fn hab() -> PathBuf {
        HabboAssetFolder::FURNITURE.hab()
    }
    pub fn icons() -> PathBuf {
        Path::new("Habbo_Default").join("hof_furni").join("icons")
    }
    pub fn swf_source() -> PathBuf {
        HabboAssetFolder::FURNITURE.swf_source()
    }
}

/// True when `dir` holds at least one file with this extension (case-insensitive, no dot).
pub fn has_files_with_ext(dir: &Path, ext: &str) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().any(|e| has_ext(&e.path(), ext) && e.path().is_file()))
        .unwrap_or(false)
}

/// Files directly in `dir` with this extension (no dot, case-insensitive), sorted by name.
/// Missing folder = empty list, like `Directory.GetFiles` guarded by `Directory.Exists`.
pub fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && has_ext(p, ext))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Every file under `dir` (recursive), sorted.
pub fn files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

pub fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext.trim_start_matches('.')))
}

/// `Path.GetFileNameWithoutExtension`.
pub fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `Path.GetFileName`.
pub fn file_name(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}
