//! `Mapper/Palette`: `PaletteExtractor` and `PaletteData` (+ `CompactRgbConverter`).

use std::path::Path;

use anyhow::Result;
use habbo_core::outln;
use indexmap::IndexMap;

use crate::net::{self, parse_int, try_parse_bool};
use crate::stj::{Obj, J};
use crate::xml::{self, attr, elements};

const DEFAULT_COLOR_HEX: &str = "999999";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaletteData {
    pub id: i32,
    pub source: Option<String>,
    pub master: bool,
    pub tags: Vec<String>,
    pub breed: Option<i32>,
    pub color_tag: Option<i32>,
    pub color1: Option<String>,
    pub color2: Option<String>,
    pub rgb: Option<Vec<[u8; 3]>>,
}

impl PaletteData {
    pub fn to_json(&self) -> J {
        Obj::new()
            .put("id", J::int(self.id as i64))
            .opt("source", self.source.clone().map(J::Str))
            .put("master", J::Bool(self.master))
            .put("tags", J::Arr(self.tags.iter().map(|t| J::str(t.clone())).collect()))
            .opt("breed", self.breed.map(|v| J::int(v as i64)))
            .opt("colorTag", self.color_tag.map(|v| J::int(v as i64)))
            .opt("color1", self.color1.clone().map(J::Str))
            .opt("color2", self.color2.clone().map(J::Str))
            .opt(
                "rgb",
                self.rgb.as_ref().map(|rgb| J::Arr(rgb.iter().map(|[r, g, b]| J::RawItem(format!("[{r},{g},{b}]"))).collect())),
            )
            .build()
    }
}

pub fn palettes_json(palettes: &IndexMap<i32, PaletteData>) -> J {
    J::Obj(palettes.iter().map(|(k, v)| (k.to_string(), v.to_json())).collect())
}

fn default_ramp() -> Vec<[u8; 3]> {
    vec![[0x99, 0x99, 0x99]; 256]
}

fn read_palette_file(path: &Path) -> Result<Vec<[u8; 3]>> {
    let bytes = std::fs::read(path)?;
    Ok(bytes.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
}

/// `PaletteExtractor.ExtractPalettes`: `<palette>` entries of *_assets.bin with the colour
/// table of their `*_{source}.bin`; palettes without one reuse the master's table.
pub fn extract_palettes(binary_output_path: &Path) -> Result<IndexMap<i32, PaletteData>> {
    let binary_data = binary_output_path.join("binaryData");
    let Some(assets_file) = net::get_files(&binary_data, "*_assets.bin")?.into_iter().next() else {
        outln!("❌ No _assets.bin file found.");
        return Ok(IndexMap::new());
    };

    // NOTE(port): XElement.Load honours the XML encoding declaration; this reads UTF-8/UTF-16 by BOM.
    let text = net::read_all_text(&assets_file)?;
    let doc = xml::parse(&text)?;
    let all_bin_files = net::get_files(&binary_data, "*_*.bin")?;

    // The C# keeps object references: a deferred palette is still filled in (and reported)
    // even when a later palette with the same id replaced it in the dictionary.
    let mut arena: Vec<PaletteData> = Vec::new();
    let mut by_id: IndexMap<i32, usize> = IndexMap::new();
    let mut deferred: Vec<usize> = Vec::new();
    let mut reference_rgb: Option<Vec<[u8; 3]>> = None;
    let mut reference_color1: Option<String> = None;

    for palette in elements(doc.root_element(), "palette") {
        let id = parse_int(attr(palette, "id").unwrap_or("-1"))?;
        if id == -1 {
            continue;
        }
        let source = attr(palette, "source").unwrap_or("unknown").to_string();
        let master = try_parse_bool(attr(palette, "master")) == Some(true);
        let color1 = attr(palette, "color1").map(str::to_string);
        let color2 = attr(palette, "color2").map(str::to_string);
        let tags = attr(palette, "tags").unwrap_or("");
        let breed = attr(palette, "breed").map(parse_int).transpose()?;
        let color_tag = attr(palette, "colortag").map(parse_int).transpose()?;

        let suffix = format!("_{source}.bin");
        let palette_file = all_bin_files.iter().find(|f| f.to_string_lossy().ends_with(&suffix));

        arena.push(PaletteData {
            id,
            source: Some(source.clone()),
            master,
            tags: tags.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect(),
            breed,
            color_tag,
            color1: color1.clone(),
            color2: color2.clone(),
            rgb: None,
        });
        let slot = arena.len() - 1;
        by_id.insert(id, slot);

        let Some(palette_file) = palette_file else {
            deferred.push(slot);
            continue;
        };

        let rgb = read_palette_file(palette_file)?;
        let count = rgb.len();
        if reference_rgb.is_none() || master {
            reference_rgb = Some(rgb.clone());
            reference_color1 = color1.clone();
        }
        arena[slot].rgb = Some(rgb);

        outln!(
            "✅ Loaded palette {id}: {source} | Color1: {}, Color2: {}, RGB count: {count}",
            color1.as_deref().unwrap_or(""),
            color2.as_deref().unwrap_or("")
        );
    }

    for slot in deferred {
        let data = &mut arena[slot];
        match &reference_rgb {
            Some(reference) => {
                outln!(
                    "↺ Palette {} ({}): colour binary not exported — reusing the shared colour table (aliased palette).",
                    data.id,
                    data.source.as_deref().unwrap_or("")
                );
                data.rgb = Some(reference.clone());
                if data.color1.as_deref().is_none_or(str::is_empty) {
                    data.color1 = Some(reference_color1.clone().unwrap_or_else(|| DEFAULT_COLOR_HEX.into()));
                }
            }
            None => {
                outln!(
                    "⚠️ Palette {} ({}): no colour data anywhere in the SWF — using a default one-colour palette.",
                    data.id,
                    data.source.as_deref().unwrap_or("")
                );
                data.rgb = Some(default_ramp());
                if data.color1.as_deref().is_none_or(str::is_empty) {
                    data.color1 = Some(DEFAULT_COLOR_HEX.into());
                }
            }
        }
    }

    let mut palettes: IndexMap<i32, PaletteData> = by_id.into_iter().map(|(id, slot)| (id, arena[slot].clone())).collect();
    if palettes.is_empty() {
        outln!("⚠️ No palettes found in SWF — injecting a default one-colour palette (id 0) so the pet renders instead of turning black.");
        palettes.insert(
            0,
            PaletteData {
                id: 0,
                source: Some("default".into()),
                master: true,
                tags: Vec::new(),
                color1: Some(DEFAULT_COLOR_HEX.into()),
                rgb: Some(default_ramp()),
                ..Default::default()
            },
        );
    }
    Ok(palettes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_palettes() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("binaryData");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("1_dog_assets.bin"),
            r#"<assets><palette id="0" source="dog_palette1" master="true" tags="a,,b" breed="1" color1="FFFFFF"/><palette id="1" source="dog_palette2" colortag="3"/></assets>"#,
        )
        .unwrap();
        std::fs::write(bin.join("2_dog_palette1.bin"), [1u8, 2, 3, 4, 5, 6, 7]).unwrap();
        let palettes = extract_palettes(dir.path()).unwrap();
        assert_eq!(palettes.len(), 2);
        assert_eq!(palettes[&0].rgb, Some(vec![[1, 2, 3], [4, 5, 6]]));
        assert_eq!(palettes[&1].rgb, palettes[&0].rgb);
        assert_eq!(palettes[&1].color1.as_deref(), Some("FFFFFF"));
        let json = palettes_json(&palettes).to_string_indented().unwrap();
        let nl = habbo_core::paths::NEWLINE;
        assert!(json.contains(&format!("\"rgb\": [[1,2,3],[4,5,6]{nl}    ]")));
        assert!(json.contains("\"tags\": ["));
        assert!(!json.contains("\"color2\""));
    }

    #[test]
    fn injects_a_default_palette() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("binaryData");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("1_x_assets.bin"), "<assets/>").unwrap();
        let palettes = extract_palettes(dir.path()).unwrap();
        assert_eq!(palettes[&0].rgb.as_ref().unwrap().len(), 256);
        assert_eq!(palettes[&0].source.as_deref(), Some("default"));
    }
}
