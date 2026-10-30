//! The optional Nitro asset workspace (`nitro_assets_root` in config.ini): its folder layout,
//! the router that sends output there, and transactional writes with backups.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail, Result};

use crate::config::append_ext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetWorkspacePaths {
    pub root: PathBuf,
    pub furniture: PathBuf,
    pub pets: PathBuf,
    pub effects: PathBuf,
    pub clothing: PathBuf,
    pub game_data: PathBuf,
    pub backups: PathBuf,
}

impl AssetWorkspacePaths {
    pub fn from_root(root: &str) -> Result<Self> {
        if root.trim().is_empty() {
            bail!("Choose a Nitro assets folder.");
        }
        let normalized = std::path::absolute(root.trim())?;
        let bundled = normalized.join("bundled");
        Ok(Self {
            furniture: bundled.join("furniture"),
            pets: bundled.join("pet"),
            effects: bundled.join("effect"),
            clothing: bundled.join("figure"),
            game_data: normalized.join("gamedata"),
            backups: normalized.join("backups"),
            root: normalized,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAssetKind {
    Furniture,
    Pets,
    Effects,
    Clothing,
}

#[derive(Debug, Clone, Default)]
pub struct AssetWorkspaceRouter {
    pub paths: Option<AssetWorkspacePaths>,
}

impl AssetWorkspaceRouter {
    pub fn is_configured(&self) -> bool {
        self.paths.is_some()
    }

    pub fn asset_directory(&self, kind: WorkspaceAssetKind, fallback: impl Into<PathBuf>) -> PathBuf {
        match &self.paths {
            None => fallback.into(),
            Some(paths) => match kind {
                WorkspaceAssetKind::Furniture => paths.furniture.clone(),
                WorkspaceAssetKind::Pets => paths.pets.clone(),
                WorkspaceAssetKind::Effects => paths.effects.clone(),
                WorkspaceAssetKind::Clothing => paths.clothing.clone(),
            },
        }
    }

    pub fn game_data_file(&self, file_name: &str, fallback: impl Into<PathBuf>) -> PathBuf {
        match &self.paths {
            None => fallback.into(),
            Some(paths) => paths.game_data.join(file_name),
        }
    }
}

static ROUTER: RwLock<Option<Arc<AssetWorkspaceRouter>>> = RwLock::new(None);

/// `AssetWorkspaceRuntime`: the process-wide router.
pub mod runtime {
    use super::*;

    pub fn router() -> Arc<AssetWorkspaceRouter> {
        ROUTER.read().unwrap().clone().unwrap_or_default()
    }

    pub fn load(config_path: &Path) {
        let result = WorkspaceSettingsStore::new(config_path).and_then(|store| store.load_root());
        match result {
            Ok(Some(root)) => match AssetWorkspacePaths::from_root(&root.to_string_lossy()) {
                Ok(paths) => set(Some(paths)),
                Err(error) => {
                    crate::outln!("Workspace configuration ignored: {error}");
                    set(None);
                }
            },
            Ok(None) => set(None),
            Err(error) => {
                crate::outln!("Workspace configuration ignored: {error}");
                set(None);
            }
        }
    }

    pub fn set(paths: Option<AssetWorkspacePaths>) {
        *ROUTER.write().unwrap() = Some(Arc::new(AssetWorkspaceRouter { paths }));
    }
}

/// Reads and writes `nitro_assets_root` in `[AppSettings]`.
pub struct WorkspaceSettingsStore {
    config_path: PathBuf,
}

const SECTION: &str = "[AppSettings]";
const KEY: &str = "nitro_assets_root";

impl WorkspaceSettingsStore {
    pub fn new(config_path: &Path) -> Result<Self> {
        Ok(Self { config_path: std::path::absolute(config_path)? })
    }

    pub fn load_root(&self) -> Result<Option<PathBuf>> {
        if !self.config_path.is_file() {
            return Ok(None);
        }
        let mut section: Option<String> = None;
        for line in std::fs::read_to_string(&self.config_path)?.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                section = Some(trimmed.to_string());
                continue;
            }
            if !section.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(SECTION)) {
                continue;
            }
            let Some(separator) = trimmed.find('=') else { continue };
            if separator == 0 || !trimmed[..separator].trim().eq_ignore_ascii_case(KEY) {
                continue;
            }
            let value = trimmed[separator + 1..].trim();
            return Ok(if value.is_empty() { None } else { Some(std::path::absolute(value)?) });
        }
        Ok(None)
    }

    pub fn save_root(&self, root: &str) -> Result<()> {
        let normalized = AssetWorkspacePaths::from_root(root)?.root;
        let mut lines: Vec<String> = if self.config_path.is_file() {
            std::fs::read_to_string(&self.config_path)?.lines().map(str::to_string).collect()
        } else {
            Vec::new()
        };
        let section_index = match lines.iter().position(|l| l.trim().eq_ignore_ascii_case(SECTION)) {
            Some(index) => index,
            None => {
                if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(SECTION.to_string());
                lines.len() - 1
            }
        };
        let next_section = (section_index + 1..lines.len())
            .find(|&i| lines[i].trim_start().starts_with('['))
            .unwrap_or(lines.len());
        let key_index = (section_index + 1..next_section).find(|&i| {
            let trimmed = lines[i].trim();
            trimmed.find('=').is_some_and(|s| s > 0 && trimmed[..s].trim().eq_ignore_ascii_case(KEY))
        });

        let setting = format!("{KEY}={}", normalized.display());
        match key_index {
            Some(index) => lines[index] = setting,
            None => lines.insert(next_section, setting),
        }

        if let Some(directory) = self.config_path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        let temporary = append_ext(&self.config_path, &format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut text = lines.join(crate::paths::NEWLINE);
        text.push_str(crate::paths::NEWLINE);
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &self.config_path)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceFilePreview {
    pub relative_path: PathBuf,
    pub target_path: PathBuf,
    pub will_replace_existing: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceChangeResult {
    pub backup_directory: PathBuf,
    pub written_files: Vec<PathBuf>,
}

/// Writes a batch of files under the workspace root: existing files are backed up to
/// backups/<timestamp>/ first, and a failure part-way restores what was written.
pub struct WorkspaceFileTransaction {
    paths: AssetWorkspacePaths,
    clock: Box<dyn Fn() -> chrono::DateTime<chrono::Local> + Send + Sync>,
}

impl WorkspaceFileTransaction {
    pub fn new(paths: AssetWorkspacePaths) -> Self {
        Self { paths, clock: Box::new(chrono::Local::now) }
    }

    pub fn with_clock(paths: AssetWorkspacePaths, clock: impl Fn() -> chrono::DateTime<chrono::Local> + Send + Sync + 'static) -> Self {
        Self { paths, clock: Box::new(clock) }
    }

    pub fn preview<'a>(&self, relative_paths: impl IntoIterator<Item = &'a Path>) -> Result<Vec<WorkspaceFilePreview>> {
        relative_paths
            .into_iter()
            .map(|relative| {
                let target = self.resolve_target(relative)?;
                Ok(WorkspaceFilePreview {
                    relative_path: relative.to_path_buf(),
                    will_replace_existing: target.is_file(),
                    target_path: target,
                })
            })
            .collect()
    }

    pub fn apply(&self, changes: &[(PathBuf, Vec<u8>)]) -> Result<WorkspaceChangeResult> {
        let preview = self.preview(changes.iter().map(|(p, _)| p.as_path()))?;
        let backup_directory = self.paths.backups.join((self.clock)().format("%Y%m%d-%H%M%S").to_string());
        let mut temporary_files: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut written: Vec<PathBuf> = Vec::new();

        let result = (|| -> Result<()> {
            for file in preview.iter().filter(|f| f.will_replace_existing) {
                let backup = backup_directory.join(&file.relative_path);
                std::fs::create_dir_all(backup.parent().unwrap())?;
                if backup.exists() {
                    bail!("The backup file '{}' already exists.", backup.display());
                }
                std::fs::copy(&file.target_path, &backup)?;
            }
            for (relative, content) in changes {
                let target = self.resolve_target(relative)?;
                std::fs::create_dir_all(target.parent().unwrap())?;
                let temporary = append_ext(&target, &format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
                std::fs::write(&temporary, content)?;
                temporary_files.push((temporary, target));
            }
            for (temporary, target) in &temporary_files {
                std::fs::rename(temporary, target)?;
                written.push(target.clone());
            }
            Ok(())
        })();

        if let Err(error) = result {
            for target in written.iter().rev() {
                let file = preview.iter().find(|f| &f.target_path == target);
                if let Some(file) = file.filter(|f| f.will_replace_existing) {
                    let _ = std::fs::copy(backup_directory.join(&file.relative_path), target);
                } else if target.is_file() {
                    let _ = std::fs::remove_file(target);
                }
            }
            for (temporary, _) in &temporary_files {
                let _ = std::fs::remove_file(temporary);
            }
            return Err(error);
        }
        Ok(WorkspaceChangeResult { backup_directory, written_files: written })
    }

    fn resolve_target(&self, relative: &Path) -> Result<PathBuf> {
        if relative.as_os_str().is_empty() || relative.is_absolute() || relative.has_root() {
            bail!("Workspace changes require a relative path.");
        }
        let target = std::path::absolute(self.paths.root.join(relative))?;
        let target = normalize_dots(&target);
        if !is_inside(&self.paths.root, &target) {
            bail!("The target file is outside the configured workspace.");
        }
        Ok(target)
    }
}

/// `WorkspaceOutput.WriteAllBytesAsync`: inside the workspace goes through a transaction
/// (with backup); anywhere else is a plain write that creates the folder.
pub fn write_output(target: &Path, content: &[u8]) -> Result<()> {
    let router = runtime::router();
    if let Some(paths) = &router.paths {
        let full = normalize_dots(&std::path::absolute(target)?);
        if is_inside(&paths.root, &full) {
            let relative = full
                .strip_prefix(&paths.root)
                .map_err(|_| anyhow!("The target file is outside the configured workspace."))?
                .to_path_buf();
            WorkspaceFileTransaction::new(paths.clone()).apply(&[(relative, content.to_vec())])?;
            return Ok(());
        }
    }
    if let Some(directory) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(target, content)?;
    Ok(())
}

fn is_inside(root: &Path, target: &Path) -> bool {
    let root = normalize_dots(root);
    if cfg!(windows) {
        let root = root.to_string_lossy().to_lowercase();
        let root = root.trim_end_matches(['\\', '/']).to_string() + "\\";
        target.to_string_lossy().to_lowercase().starts_with(&root)
    } else {
        target.starts_with(&root) && target != root
    }
}

/// Resolves `.` and `..` without touching the disk (Path.GetFullPath semantics).
fn normalize_dots(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct AssetWorkspaceFolder {
    pub name: &'static str,
    pub path: PathBuf,
    pub exists: bool,
    pub file_count: usize,
}

/// `AssetWorkspaceInspector`: which workspace folders exist and how many files they hold.
pub fn inspect(paths: &AssetWorkspacePaths) -> Vec<AssetWorkspaceFolder> {
    [
        ("Furniture", &paths.furniture),
        ("Pets", &paths.pets),
        ("Effects", &paths.effects),
        ("Clothing", &paths.clothing),
        ("Gamedata", &paths.game_data),
    ]
    .into_iter()
    .map(|(name, path)| AssetWorkspaceFolder {
        name,
        path: path.clone(),
        exists: path.is_dir(),
        file_count: if path.is_dir() { crate::paths::files_recursive(path).len() } else { 0 },
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_backs_up_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AssetWorkspacePaths::from_root(&dir.path().to_string_lossy()).unwrap();
        std::fs::create_dir_all(&paths.furniture).unwrap();
        std::fs::write(paths.furniture.join("a.nitro"), b"old").unwrap();

        let fixed = chrono::Local::now();
        let tx = WorkspaceFileTransaction::with_clock(paths.clone(), move || fixed);
        let result = tx
            .apply(&[(PathBuf::from("bundled/furniture/a.nitro"), b"new".to_vec()), (PathBuf::from("bundled/furniture/b.nitro"), b"b".to_vec())])
            .unwrap();
        assert_eq!(std::fs::read(paths.furniture.join("a.nitro")).unwrap(), b"new");
        assert_eq!(std::fs::read(result.backup_directory.join("bundled/furniture/a.nitro")).unwrap(), b"old");
        assert!(tx.apply(&[(PathBuf::from("../x"), vec![])]).is_err());
    }

    #[test]
    fn settings_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.ini");
        std::fs::write(&config, "[AppSettings]\na=1\n[Other]\n").unwrap();
        let store = WorkspaceSettingsStore::new(&config).unwrap();
        assert_eq!(store.load_root().unwrap(), None);
        let root = dir.path().join("assets");
        store.save_root(&root.to_string_lossy()).unwrap();
        assert_eq!(store.load_root().unwrap(), Some(root));
    }
}
