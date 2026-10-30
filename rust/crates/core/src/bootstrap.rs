//! Startup concerns independent of the UI: the Java check and the runtime folder layout.

use std::path::Path;
use std::process::{Command, Stdio};

pub fn is_java_available() -> bool {
    let mut command = Command::new("java");
    command.arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.status().map(|s| s.success()).unwrap_or(false)
}

/// Creates every folder the tools read from or write to (relative to the working directory).
/// The caller moves the legacy Generate/Furniture folder first (see the SQL generator).
pub fn create_directories() -> std::io::Result<()> {
    let kinds = ["furni", "clothing", "effects", "pets"];
    let structure: Vec<(&str, Vec<String>)> = vec![
        ("NitroCompiler/compile", kinds.iter().map(|s| s.to_string()).collect()),
        ("NitroCompiler/compiled", kinds.iter().map(|s| s.to_string()).collect()),
        ("NitroCompiler/extract", kinds.iter().map(|s| s.to_string()).collect()),
        ("NitroCompiler/extracted", kinds.iter().map(|s| s.to_string()).collect()),
        (
            "Habbo_Default",
            [
                "badges", "clothes", "clothes/swf", "clothes/hab", "effects/swf", "effects/hab", "pets/swf", "pets/hab",
                "files/txt", "files/xml", "files/json", "hof_furni", "hof_furni/swf", "hof_furni/hab", "hof_furni/icons",
                "icons", "mp3", "quests", "reception", "reception/web_promo_small",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ),
        (
            "Merge",
            [
                "Original_Furnidata", "Import_Furnidata", "Merged_Furnidata", "Original_ClothesData", "Import_ClothesData",
                "Merged_ClothesData", "Original_Productdata", "Import_Productdata", "Merged_Productdata",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ),
        ("Generate", ["Furnidata", "default_furniture", "bc_furniture", "Output_SQL"].iter().map(|s| s.to_string()).collect()),
        (
            "SWFCompiler",
            ["clothes", "furniture", "import", "import/clothes", "import/furniture", "import/pets", "import/effects", "import/effects/CustomXML"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        ),
        ("Database", vec!["Variables".to_string()]),
        ("custom_downloads", ["clothes", "nitro_furniture", "nitro_furniture/icons"].iter().map(|s| s.to_string()).collect()),
    ];

    for (base, subs) in structure {
        let base = Path::new(base);
        std::fs::create_dir_all(base)?;
        for sub in subs {
            std::fs::create_dir_all(base.join(sub))?;
        }
    }
    Ok(())
}
