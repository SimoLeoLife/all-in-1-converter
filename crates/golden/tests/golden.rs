//! Golden tests: the Rust converters must give exactly what the C# tool gave for the sample in
//! `tests/golden` (json byte for byte, sheets pixel for pixel, SQL text byte for byte).
//!
//! OS differences: json and SQL lines end with the platform newline in both tools (CRLF in the
//! Windows-made fixtures, LF expected on Linux, see `with_platform_newlines`). The sprite packer
//! uses directory listing order like the C# tool, which only NTFS returns sorted, so off Windows
//! the sheets are checked sprite by sprite (`diff_layout_independent`) instead of byte/pixel exact;
//! on Windows both checks run. The SQL generator sorts its input, so its text is exact everywhere.
//!
//! The converters work on relative paths and process-wide state (working directory, console,
//! settings), so every case takes [`LOCK`] and runs in its own temporary working directory.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use habbo_core::console::{self, Sink, Source};
use habbo_golden::{
    decode_image, diff_bytes, diff_images, diff_layout_independent, expected_file_name, golden_dir, is_image_entry,
    read_bundle, read_manifest, with_platform_newlines, FileEntry, STRICT_LAYOUT,
};

static LOCK: Mutex<()> = Mutex::new(());

/// Collects everything the converters print.
#[derive(Default)]
struct Capture(Mutex<String>);

impl Sink for Capture {
    fn write(&self, text: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push_str(text);
    }
}

/// Answers `console::read_line` from a fixed list (then end of input).
struct Answers(Mutex<VecDeque<String>>);

impl Source for Answers {
    fn read_line(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

/// A temporary working directory with the console and settings set up for one run; restores
/// the previous working directory and console on drop.
struct Workdir {
    dir: tempfile::TempDir,
    previous_dir: PathBuf,
    previous_sink: Arc<dyn Sink>,
    previous_source: Option<Arc<dyn Source>>,
    log: Arc<Capture>,
    _lock: MutexGuard<'static, ()>,
}

impl Workdir {
    fn new(answers: &[&str]) -> Workdir {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("temp dir");
        let previous_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(dir.path()).expect("enter temp dir");

        // The native SWF reader must handle every sample on its own (no FFDec fallback).
        std::env::remove_var("CONFURTER_DISABLE_NATIVE_SWF");
        std::env::remove_var("CONFURTER_VERIFY_NATIVE_SWF");
        habbo_core::workspace::runtime::set(None);
        habbo_core::settings::reload();
        // What the "... to HAB" menu operations set before they run.
        habbo_core::settings::set_spritesheet_format("webp");
        habbo_nitro::writer::set_extension(".hab");

        let log = Arc::new(Capture::default());
        let previous_sink = console::set_sink(log.clone());
        let previous_source =
            console::set_source(Some(Arc::new(Answers(Mutex::new(answers.iter().map(|a| a.to_string()).collect())))));
        Workdir { dir, previous_dir, previous_sink, previous_source, log, _lock: lock }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn log(&self) -> String {
        self.log.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn log_tail(&self) -> String {
        let log = self.log();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(40)..].join("\n")
    }

    fn run<F: Future<Output = anyhow::Result<()>>>(&self, operation: F) {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
        if let Err(error) = runtime.block_on(operation) {
            panic!("operation failed: {error:#}\n--- console ---\n{}", self.log_tail());
        }
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous_dir);
        console::set_sink(self.previous_sink.clone());
        console::set_source(self.previous_source.take());
    }
}

fn copy_dir(from: &Path, to: &Path) -> usize {
    std::fs::create_dir_all(to).unwrap();
    let mut count = 0;
    for entry in std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display())).flatten() {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            count += 1;
        }
    }
    count
}

/// Compares every bundle in `output_dir` with `tests/golden/<category>`; returns the problems.
fn compare_bundles(category: &str, output_dir: &Path) -> Vec<String> {
    let category_dir = golden_dir().join(category);
    let manifest = read_manifest(&category_dir.join("manifest.json")).unwrap();
    let mut problems = Vec::new();

    let mut produced: Vec<String> = std::fs::read_dir(output_dir)
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    produced.sort();
    for file in &produced {
        let stem = file.strip_suffix(".hab").unwrap_or(file);
        if !manifest.iter().any(|(name, _)| name == stem) {
            problems.push(format!("{file}: written, but the C# tool wrote no such bundle"));
        }
    }

    for (name, expected_entries) in &manifest {
        let bundle_path = output_dir.join(format!("{name}.hab"));
        if !bundle_path.is_file() {
            problems.push(format!("{name}: no {name}.hab was written"));
            continue;
        }
        let entries: Vec<FileEntry> = match read_bundle(&bundle_path) {
            Ok(entries) => entries,
            Err(error) => {
                problems.push(format!("{name}: unreadable bundle: {error:#}"));
                continue;
            }
        };
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        if names != expected_entries.iter().map(String::as_str).collect::<Vec<_>>() {
            problems.push(format!("{name}: bundle entries {names:?}, expected {expected_entries:?}"));
        }
        let mut expected_json = None;
        let mut expected_sheet = None;
        let mut actual_json = None;
        let mut actual_sheet = None;
        for (entry, bytes) in &entries {
            let expected_path = category_dir.join("expected").join(expected_file_name(entry));
            let Ok(expected) = std::fs::read(&expected_path) else {
                continue; // already reported as an entry list difference
            };
            if is_image_entry(entry) {
                match (decode_image(&expected), decode_image(bytes)) {
                    (Ok(expected), Ok(actual)) => {
                        if STRICT_LAYOUT {
                            if let Some(difference) = diff_images(&expected, &actual) {
                                problems.push(format!("{name} / {entry}: {difference}"));
                            }
                        }
                        expected_sheet = Some(expected);
                        actual_sheet = Some(actual);
                    }
                    (_, Err(error)) => problems.push(format!("{name} / {entry}: sheet does not decode: {error:#}")),
                    (Err(error), _) => problems.push(format!("{name} / {entry}: expected PNG does not decode: {error:#}")),
                }
            } else {
                if STRICT_LAYOUT {
                    if let Some(difference) = diff_bytes(&with_platform_newlines(&expected), bytes) {
                        problems.push(format!("{name} / {entry}: {difference}"));
                    }
                }
                if entry.to_ascii_lowercase().ends_with(".json") {
                    expected_json = Some(expected);
                    actual_json = Some(bytes.clone());
                }
            }
        }
        // Also everywhere else: the same content, wherever the packer put each sprite.
        if let (Some(expected_json), Some(actual_json)) = (expected_json, actual_json) {
            for difference in
                diff_layout_independent(&expected_json, expected_sheet.as_ref(), &actual_json, actual_sheet.as_ref())
            {
                problems.push(format!("{name} (layout-independent): {difference}"));
            }
        }
    }
    problems
}

fn report(category: &str, work: &Workdir, problems: Vec<String>) {
    let log = work.log();
    let mut problems = problems;
    if log.to_ascii_lowercase().contains("ffdec") {
        problems.push("the FFDec fallback was used (the native SWF reader must handle every sample)".into());
    }
    if problems.is_empty() {
        return;
    }
    let mut text = format!("{category}: {} difference(s) from the C# output:\n", problems.len());
    for problem in &problems {
        let _ = writeln!(text, "  - {problem}");
    }
    let _ = write!(text, "--- console (last lines) ---\n{}", work.log_tail());
    panic!("{text}");
}

/// Converts `tests/golden/<category>/swf` with the "(I) Imported" source and compares.
fn swf_case<F, Fut>(category: &str, import_folder: &str, output_folder: &str, operation: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let work = Workdir::new(&["I"]);
    let copied = copy_dir(
        &golden_dir().join(category).join("swf"),
        &work.path().join("SWFCompiler").join("import").join(import_folder),
    );
    assert!(copied > 0, "no input SWFs for {category}");
    work.run(operation());
    let output_dir = habbo_nitro::writer::folder(&Path::new("SWFCompiler").join(output_folder));
    let problems = compare_bundles(category, &work.path().join(output_dir));
    report(category, &work, problems);
}

#[test]
fn furniture_matches_csharp() {
    swf_case("furniture", "furniture", "furniture", habbo_furni::convert_furniture);
}

#[test]
fn clothes_match_csharp() {
    swf_case("clothes", "clothes", "clothes", habbo_figure::convert_clothes);
}

#[test]
fn pets_match_csharp() {
    swf_case("pets", "pets", "pets", habbo_furni::convert_pets);
}

#[test]
fn effects_match_csharp() {
    swf_case("effects", "effects", "effects", habbo_figure::convert_effects);
}

/// Generate SQL over the sampled furni: the .hab inputs are rebuilt from the furniture expected
/// files (the generator only reads their json), with the trimmed FurnitureData.json.
#[test]
fn generate_sql_matches_csharp() {
    let work = Workdir::new(&["90000", "777"]);
    let generate = work.path().join("Generate");
    let furnidata_dir = generate.join("Furnidata");
    let furniture_dir = generate.join("default_furniture");
    std::fs::create_dir_all(&furnidata_dir).unwrap();
    std::fs::create_dir_all(&furniture_dir).unwrap();
    let sql_dir = golden_dir().join("sql");
    std::fs::copy(sql_dir.join("FurnitureData.json"), furnidata_dir.join("FurnitureData.json")).unwrap();

    let furniture = golden_dir().join("furniture");
    for (name, entries) in read_manifest(&furniture.join("manifest.json")).unwrap() {
        let files: Vec<FileEntry> = entries
            .iter()
            .map(|entry| {
                let file = expected_file_name(entry);
                (file.clone(), std::fs::read(furniture.join("expected").join(&file)).unwrap())
            })
            .collect();
        let bundle = habbo_nitro::writer::build(".hab", &name, files).unwrap();
        std::fs::write(furniture_dir.join(format!("{name}.hab")), bundle).unwrap();
    }

    work.run(habbo_db::generate_sql(habbo_db::FurnitureSqlTarget::Standard));

    // The file name carries a timestamp: items_yyyyMMddHHmmss.sql.
    let outputs: Vec<PathBuf> = std::fs::read_dir(generate.join("Output_SQL"))
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let mut problems = Vec::new();
    match outputs.as_slice() {
        [path] => {
            let file = habbo_core::paths::file_name(path);
            let stamp = file.strip_prefix("items_").and_then(|s| s.strip_suffix(".sql"));
            if !stamp.is_some_and(|s| s.len() == 14 && s.bytes().all(|b| b.is_ascii_digit())) {
                problems.push(format!("output file name {file:?} is not items_<yyyyMMddHHmmss>.sql"));
            }
            let expected = with_platform_newlines(&std::fs::read(sql_dir.join("expected.sql")).unwrap());
            if let Some(difference) = diff_bytes(&expected, &std::fs::read(path).unwrap()) {
                problems.push(format!("{file}: {difference}"));
            }
        }
        other => problems.push(format!("expected one SQL file in Generate/Output_SQL, found {other:?}")),
    }
    report("generate-sql", &work, problems);
}

/// Every input SWF has its C# bundle and every manifest entry its expected file.
#[test]
fn fixtures_are_complete() {
    for category in ["furniture", "clothes", "pets", "effects"] {
        let dir = golden_dir().join(category);
        let manifest = read_manifest(&dir.join("manifest.json")).unwrap();
        for entry in std::fs::read_dir(dir.join("swf")).unwrap().flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let stem = file.strip_suffix(".swf").unwrap_or(&file);
            assert!(manifest.iter().any(|(name, _)| name == stem), "{category}/swf/{file} has no manifest entry");
        }
        for (name, entries) in &manifest {
            for entry in entries {
                let path = dir.join("expected").join(expected_file_name(entry));
                assert!(path.is_file(), "{category}: {name} needs {}", path.display());
            }
        }
    }
}
