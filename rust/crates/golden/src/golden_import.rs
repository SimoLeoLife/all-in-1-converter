//! Rebuilds the expected files of one golden category from the C# tool's output:
//!
//! `golden-import <category> <folder with the C# .hab files>`
//!
//! writes `rust/golden/<category>/expected/` (json as-is, sheets decoded to lossless PNG) and
//! `rust/golden/<category>/manifest.json`. See `rust/golden/README.md`.

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [category, bundle_dir] = args.as_slice() else {
        anyhow::bail!("usage: golden-import <furniture|clothes|pets|effects> <folder with the C# .hab files>");
    };
    let category_dir = habbo_golden::golden_dir().join(category);
    let manifest = habbo_golden::import_bundles(&PathBuf::from(bundle_dir), &category_dir)?;
    for (name, entries) in &manifest {
        println!("{name}: {}", entries.join(", "));
    }
    println!("{} bundles -> {}", manifest.len(), category_dir.display());
    Ok(())
}
