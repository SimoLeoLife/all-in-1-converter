//! `SQLGenerator` (Tools/Generate_SQL.cs): turns the furni in Generate/default_furniture (or
//! Generate/bc_furniture) into items_base + catalog_items (or catalog_items_bc) INSERTs in
//! Generate/Output_SQL/<items|bc_items>_<yyyyMMddHHmmss>.sql.
//!
//! The SQL text is the C# text byte for byte: the same column lists, values inserted without
//! escaping (as the C# did), "\n" inside one INSERT (the C# verbatim strings) and
//! `Environment.NewLine` after every line (`File.WriteAllLines`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use habbo_core::paths::{file_name, file_stem, NEWLINE};
use habbo_core::{out, outln};
use serde_json::Value;

use crate::dotnet::{
    cmp_ignore_case, double_to_string, ignore_case_key, jtoken_to_bool, jtoken_to_i32, jtoken_to_string, parse_double, parse_int,
    try_parse_int,
};

/// Which catalog the generated furni go into; items_base is the same for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FurnitureSqlTarget {
    /// Generate/default_furniture -> catalog_items (page_id from catalog_pages).
    Standard,
    /// Generate/bc_furniture -> catalog_items_bc (page_id from catalog_pages_bc).
    BuildersClub,
}

/// The folder inside Generate/ whose furni a target turns into SQL.
pub fn furniture_folder(target: FurnitureSqlTarget) -> &'static str {
    match target {
        FurnitureSqlTarget::BuildersClub => "bc_furniture",
        FurnitureSqlTarget::Standard => "default_furniture",
    }
}

/// The folder Standaard furniture used before it was renamed to default_furniture.
pub const LEGACY_FURNITURE_FOLDER: &str = "Furniture";

pub fn catalog_items_table(target: FurnitureSqlTarget) -> &'static str {
    match target {
        FurnitureSqlTarget::BuildersClub => "catalog_items_bc",
        FurnitureSqlTarget::Standard => "catalog_items",
    }
}

/// Habbo's recolourable Builders Club furni (*_recolorable) keep their own colour on the server.
pub fn interaction_type_for(classname: &str) -> &'static str {
    let lower = classname.to_lowercase();
    if lower.ends_with("_recolorable") {
        "recolorable"
    } else {
        "default"
    }
}

/// The catalog row of one furni. catalog_items_bc has no price or offer columns, so a Builders
/// Club row keeps only the ones it has, with the same values.
pub fn catalog_item_insert(target: FurnitureSqlTarget, id: i32, page_id: i32, offer_id: i32, classname: &str) -> String {
    if target == FurnitureSqlTarget::BuildersClub {
        return format!(
            "INSERT INTO `catalog_items_bc` (`id`, `item_ids`, `page_id`, `catalog_name`, `order_number`, `extradata`) VALUES\n({id}, '{id}', {page_id}, '{classname}', 99, '');"
        );
    }
    format!(
        "INSERT INTO `catalog_items` (`id`, `item_ids`, `page_id`, `offer_id`, `song_id`, `order_number`, `catalog_name`, `cost_credits`, `cost_points`, `points_type`, `amount`, `limited_sells`, `limited_stack`, `extradata`, `have_offer`, `club_only`) VALUES\n({id}, '{id}', {page_id}, {offer_id}, 0, 99, '{classname}', 5, 0, 0, 1, 0, 0, '', '1', '0');"
    )
}

const ITEMS_BASE_COLUMNS: &str = "INSERT INTO `items_base` (`id`, `sprite_id`, `item_name`, `public_name`, `width`, `length`, `stack_height`, `allow_stack`, `allow_sit`, `allow_lay`, `allow_walk`, `allow_gift`, `allow_trade`, `allow_recycle`, `allow_marketplace_sell`, `allow_inventory_stack`, `type`, `interaction_type`, `interaction_modes_count`, `vending_ids`, `multiheight`, `customparams`, `effect_id_male`, `effect_id_female`, `clothing_on_walk`) VALUES\n";

/// One items_base INSERT. `size` and `flags` are written as given (already formatted).
#[allow(clippy::too_many_arguments)]
fn items_base_insert(id: i32, sprite_id: i32, classname: &str, size: [&str; 3], flags: [&str; 4], kind: &str, interaction_modes_count: i32) -> String {
    format!(
        "{ITEMS_BASE_COLUMNS}({id}, {sprite_id}, '{classname}', '{classname}', {}, {}, {}, '{}', '{}', '{}', '{}', '1', '1', '0', '1', '1', '{kind}', '{}', {interaction_modes_count}, '0', '0', '0', 0, 0, '0');",
        size[0],
        size[1],
        size[2],
        flags[0],
        flags[1],
        flags[2],
        flags[3],
        interaction_type_for(classname)
    )
}

/// Generate/Furniture became Generate/default_furniture; an existing folder is moved once.
pub fn move_legacy_furniture_folder(generate_dir: &Path) -> Result<()> {
    let legacy = generate_dir.join(LEGACY_FURNITURE_FOLDER);
    let current = generate_dir.join(furniture_folder(FurnitureSqlTarget::Standard));

    if !legacy.is_dir() {
        return Ok(());
    }

    // An empty default_furniture (made at start-up) does not stop the old folder moving in.
    if current.is_dir() && is_empty_dir(&current)? {
        std::fs::remove_dir(&current)?;
    }

    if !current.is_dir() {
        std::fs::rename(&legacy, &current)?;
        outln!("📁 Generate/{LEGACY_FURNITURE_FOLDER}/ is now Generate/{}/.", furniture_folder(FurnitureSqlTarget::Standard));
        return Ok(());
    }

    if is_empty_dir(&legacy)? {
        std::fs::remove_dir(&legacy)?;
    } else {
        outln!(
            "⚠️ Generate/{LEGACY_FURNITURE_FOLDER}/ still has files; move them to Generate/{}/ (Standaard) or Generate/{}/ (Build Club).",
            furniture_folder(FurnitureSqlTarget::Standard),
            furniture_folder(FurnitureSqlTarget::BuildersClub)
        );
    }
    Ok(())
}

fn is_empty_dir(dir: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

/// `Directory.GetFiles(dir, "*", SearchOption.AllDirectories)`: the files of a folder in
/// name order, then its subfolders breadth-first, as .NET enumerates them on NTFS.
pub(crate) fn get_files_all_directories(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut pending = VecDeque::from([dir.to_path_buf()]);
    let mut first = true;
    while let Some(current) = pending.pop_front() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if first => return Err(error.into()),
            Err(_) => continue,
        };
        first = false;
        let mut entries: Vec<(String, PathBuf, bool)> = entries
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), e.file_type().map(|t| t.is_dir()).unwrap_or(false)))
            .collect();
        entries.sort_by(|a, b| cmp_ignore_case(&a.0, &b.0).then_with(|| a.0.cmp(&b.0)));
        for (_, path, is_dir) in entries {
            if is_dir {
                pending.push_back(path);
            } else {
                out.push(path);
            }
        }
    }
    Ok(out)
}

fn furniture_file_rank(file: &Path) -> i32 {
    match file.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("swf") => 0,
        Some("nitro") => 1,
        Some("hab") => 2,
        _ => -1,
    }
}

/// One file per furni and folder: .swf before .nitro before .hab, so a .hab saved next to its
/// .swf (Habbo download) or its .nitro is not counted twice.
pub(crate) fn furniture_files(furniture_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for file in get_files_all_directories(furniture_dir)?.into_iter().filter(|f| furniture_file_rank(f) >= 0) {
        let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let key = ignore_case_key(&parent.join(file_stem(&file)).to_string_lossy());
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(file);
    }
    Ok(order
        .iter()
        .map(|key| {
            let group = &groups[key];
            // OrderBy is stable: the first file of the lowest rank.
            let best = group.iter().map(|f| furniture_file_rank(f)).min().unwrap();
            group.iter().find(|f| furniture_file_rank(f) == best).unwrap().clone()
        })
        .collect())
}

/// The stems used by more than one furni file (case-sensitive), in first-appearance order.
pub(crate) fn duplicate_names(files: &[PathBuf]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for file in files {
        let stem = file_stem(file);
        let count = counts.entry(stem.clone()).or_insert(0);
        if *count == 0 {
            order.push(stem);
        }
        *count += 1;
    }
    order.into_iter().filter(|name| counts[name] > 1).collect()
}

fn check_for_duplicate_files(furniture_dir: &Path) -> Result<bool> {
    let duplicates = duplicate_names(&furniture_files(furniture_dir)?);
    if !duplicates.is_empty() {
        outln!("❌ Error: Duplicate filenames detected:");
        for duplicate in duplicates {
            outln!(" - {duplicate}");
        }
        return Ok(true);
    }
    Ok(false)
}

/// The interaction_modes_count of a .nitro / .hab bundle json: the animation ids 0..49 of the
/// first visualization that has any; with an animation 0 that is the highest id + 1,
/// otherwise their number. 1 when there are none.
pub fn interaction_modes_count_from_nitro_json(root: &Value) -> i32 {
    let Some(visualizations) = root.as_object().and_then(|o| o.get("visualizations")).and_then(Value::as_array) else {
        return 1;
    };
    for viz in visualizations {
        let Some(animations) = viz.as_object().and_then(|o| o.get("animations")).and_then(Value::as_object) else {
            continue;
        };
        let ids: Vec<i32> = animations.keys().filter_map(|k| try_parse_int(k)).filter(|id| (0..50).contains(id)).collect();
        if !ids.is_empty() {
            return if ids.contains(&0) { ids.iter().max().unwrap() + 1 } else { ids.len() as i32 };
        }
    }
    1
}

/// Width, length and stack height of a .nitro / .hab bundle json: logic.model.dimensions
/// x, y and z (a missing axis is 1, 1 and 0). `None` when the bundle has no dimensions.
pub fn dimensions_from_nitro_json(root: &Value) -> Option<(f64, f64, f64)> {
    let dimensions = root.as_object()?.get("logic")?.as_object()?.get("model")?.as_object()?.get("dimensions")?.as_object()?;
    let axis = |name: &str, default: f64| dimensions.get(name).filter(|v| v.is_number()).and_then(Value::as_f64).unwrap_or(default);
    Some((axis("x", 1.0), axis("y", 1.0), axis("z", 0.00)))
}

/// `File.ReadAllText`: UTF-8 by default, a BOM picks UTF-8 / UTF-16.
fn read_all_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return Ok(String::from_utf8_lossy(rest).into_owned());
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> =
            rest.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) }).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return Ok(utf16(rest, true));
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return Ok(utf16(rest, false));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The attributes of every `<name>` element (XDocument `Descendants(name)`), after the whole
/// document parsed.
fn xml_elements_attributes(xml: &str, name: &str) -> Result<Vec<Vec<(String, String)>>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut roots = 0usize;
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if e.name().as_ref() == name.as_bytes() {
                    found.push(attributes(&e)?);
                } else {
                    attributes(&e)?;
                }
            }
            Event::Empty(e) => {
                if depth == 0 {
                    roots += 1;
                }
                if e.name().as_ref() == name.as_bytes() {
                    found.push(attributes(&e)?);
                } else {
                    attributes(&e)?;
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Text(t) if depth == 0 => {
                if !t.unescape()?.trim().is_empty() {
                    bail!("Data at the root level is invalid. Line 1, position 1.");
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 {
        bail!(if roots == 0 { "Root element is missing." } else { "There are multiple root elements." });
    }
    if depth != 0 {
        bail!("Unexpected end of file has occurred. The following elements are not closed.");
    }
    Ok(found)
}

fn attributes(e: &quick_xml::events::BytesStart) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for attribute in e.attributes() {
        let attribute = attribute?;
        out.push((String::from_utf8_lossy(attribute.key.as_ref()).into_owned(), attribute.unescape_value()?.into_owned()));
    }
    Ok(out)
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// `double.Parse(text, CultureInfo.InvariantCulture)`, which also takes "," group separators.
fn parse_xml_double(text: &str) -> Result<f64> {
    let integer_end = text.find(['.', 'e', 'E']).unwrap_or(text.len());
    if text[..integer_end].contains(',') {
        let cleaned = format!("{}{}", text[..integer_end].replace(',', ""), &text[integer_end..]);
        return parse_double(&cleaned).map_err(|_| anyhow!("The input string '{text}' was not in a correct format."));
    }
    parse_double(text)
}

/// The interaction_modes_count of a SWF visualization .bin (XML); 1 on any problem.
pub(crate) fn calculate_interaction_modes_count(visualization_file_path: &Path) -> i32 {
    if !visualization_file_path.is_file() {
        return 1;
    }
    let Ok(xml) = read_all_text(visualization_file_path) else {
        return 1;
    };
    interaction_modes_count_from_visualization_xml(&xml)
}

pub(crate) fn interaction_modes_count_from_visualization_xml(xml: &str) -> i32 {
    let Ok(animations) = xml_elements_attributes(xml, "animation") else {
        return 1;
    };
    let ids: Vec<i32> = animations
        .iter()
        .map(|attrs| attribute(attrs, "id").and_then(try_parse_int).unwrap_or(-1))
        .filter(|id| (0..50).contains(id))
        .collect();
    if ids.is_empty() {
        1
    } else if ids.contains(&0) {
        ids.iter().max().unwrap() + 1
    } else {
        ids.len() as i32
    }
}

/// logic.bin `<dimensions x y z>` (defaults 1, 1, 0).
pub(crate) fn dimensions_from_logic_xml(xml: &str) -> Result<(f64, f64, f64)> {
    let elements = xml_elements_attributes(xml, "dimensions")?;
    let (mut width, mut length, mut stack_height) = (1.0, 1.0, 0.00);
    if let Some(attrs) = elements.first() {
        if let Some(x) = attribute(attrs, "x") {
            width = parse_xml_double(x)?;
        }
        if let Some(y) = attribute(attrs, "y") {
            length = parse_xml_double(y)?;
        }
        if let Some(z) = attribute(attrs, "z") {
            stack_height = parse_xml_double(z)?;
        }
    }
    Ok((width, length, stack_height))
}

#[derive(Clone)]
struct FileSettings {
    interaction_modes_count: i32,
    width: f64,
    length: f64,
    height: f64,
    /// "SWF" or "Nitro".
    file_type: &'static str,
}

/// The state of one run (the C# static dictionaries, reset per run).
pub(crate) struct Generator<'a> {
    furnidata: &'a Value,
    target: FurnitureSqlTarget,
    page_id: i32,
    pub start_id: i32,
    room_items: HashSet<String>,
    wall_items: HashSet<String>,
    furni_by_classname: HashMap<String, &'a Value>,
    /// Keyed case-insensitively (StringComparer.OrdinalIgnoreCase).
    processed_file_settings: HashMap<String, FileSettings>,
    pub items_base_sql: Vec<String>,
    pub catalog_items_sql: Vec<String>,
    /// Where NitroBundle unpacks; `None` = NitroCompiler/extracted/furni (the C# folder).
    bundle_extract_dir: Option<PathBuf>,
}

fn furnitypes<'a>(furnidata: &'a Value, section: &str) -> Result<&'a Vec<Value>> {
    let null = || anyhow!("Object reference not set to an instance of an object.");
    furnidata.get(section).and_then(|s| s.get("furnitype")).ok_or_else(null)?.as_array().ok_or_else(|| anyhow!("Cannot access child value on {}.", section))
}

fn classname_of(item: &Value) -> Option<String> {
    item.get("classname").map(jtoken_to_string)
}

impl<'a> Generator<'a> {
    pub fn new(furnidata: &'a Value, target: FurnitureSqlTarget, start_id: i32, page_id: i32) -> Result<Self> {
        let set = |section: &str| -> Result<HashSet<String>> {
            furnitypes(furnidata, section)?
                .iter()
                .map(|item| classname_of(item).ok_or_else(|| anyhow!("Object reference not set to an instance of an object.")))
                .collect()
        };
        let room_items = set("roomitemtypes")?;
        let wall_items = set("wallitemtypes")?;

        let mut furni_by_classname = HashMap::new();
        for item in furnitypes(furnidata, "roomitemtypes")?.iter().chain(furnitypes(furnidata, "wallitemtypes")?) {
            if let Some(name) = classname_of(item) {
                furni_by_classname.entry(name).or_insert(item);
            }
        }

        Ok(Self {
            furnidata,
            target,
            page_id,
            start_id,
            room_items,
            wall_items,
            furni_by_classname,
            processed_file_settings: HashMap::new(),
            items_base_sql: Vec::new(),
            catalog_items_sql: Vec::new(),
            bundle_extract_dir: None,
        })
    }

    fn next_id(&mut self) -> i32 {
        let id = self.start_id;
        self.start_id = self.start_id.wrapping_add(1);
        id
    }

    fn type_of(&self, name: &str) -> &'static str {
        if self.room_items.contains(name) {
            "s"
        } else if self.wall_items.contains(name) {
            "i"
        } else {
            "unknown"
        }
    }

    /// `itemData["id"]?.ToObject<int>() ?? startId++`, then the offer id and the row id.
    fn ids_for(&mut self, item: &Value) -> Result<(i32, i32, i32)> {
        let sprite_id = match item.get("id") {
            Some(value) => jtoken_to_i32(value)?,
            None => self.next_id(),
        };
        let offer_id = match item.get("offerid") {
            Some(value) => jtoken_to_i32(value)?,
            None => -1,
        };
        let id = self.next_id();
        Ok((sprite_id, offer_id, id))
    }

    /// The physical files, then the colour variants of the furni they produced.
    pub async fn process(&mut self, furniture_dir: &Path) -> Result<()> {
        for file in furniture_files(furniture_dir)? {
            let file_name = file_stem(&file);
            let lower = file.to_string_lossy().to_lowercase();
            if lower.ends_with(".swf") {
                outln!("Processing SWF file: {file_name}");
                let extracted_dir = furniture_dir.join(format!("{file_name}_extracted"));
                extract_swf(&file, &extracted_dir).await?;
                self.process_extracted_swf(&extracted_dir, &file_name);
                outln!("✅ Completed processing SWF file: {file_name}");
                if extracted_dir.is_dir() {
                    std::fs::remove_dir_all(&extracted_dir)?;
                }
            } else if lower.ends_with(".nitro") || lower.ends_with(".hab") {
                self.process_nitro_file(&file).await;
                outln!("✅ Nitro file: {file_name} is done!");
            }
        }
        self.process_variants()
    }

    fn process_variants(&mut self) -> Result<()> {
        let furnidata = self.furnidata;
        let variants: Vec<&Value> = furnitypes(furnidata, "roomitemtypes")?
            .iter()
            .chain(furnitypes(furnidata, "wallitemtypes")?)
            .filter(|item| classname_of(item).is_some_and(|cn| !cn.trim().is_empty() && cn.contains('*')))
            .collect();

        for variant in variants {
            let variant_classname = classname_of(variant).unwrap();
            let base_name = variant_classname.split('*').next().unwrap_or("").to_string();
            let Some(settings) = self.processed_file_settings.get(&ignore_case_key(&base_name)).cloned() else {
                continue;
            };
            let kind = self.type_of(&base_name);
            if kind == "unknown" {
                outln!("Skipping unknown type for variant: {variant_classname}");
                continue;
            }
            let (sprite_id, offer_id, id) = self.ids_for(variant)?;
            let count = settings.interaction_modes_count;
            if settings.file_type.eq_ignore_ascii_case("SWF") {
                self.items_base_sql.push(items_base_insert(id, sprite_id, &variant_classname, ["1", "1", "0.00"], ["0", "0", "0", "0"], kind, count));
                self.catalog_items_sql.push(catalog_item_insert(self.target, id, self.page_id, offer_id, &variant_classname));
            } else if settings.file_type.eq_ignore_ascii_case("Nitro") {
                // Wall items ('i') don't use tile dimensions; the objectData x/y/z are client
                // rendering geometry, so store neutral server defaults.
                let (width, length, height) = if kind == "i" {
                    ("1".to_string(), "1".to_string(), "0.00".to_string())
                } else {
                    (double_to_string(settings.width), double_to_string(settings.length), double_to_string(settings.height))
                };
                self.items_base_sql.push(items_base_insert(id, sprite_id, &variant_classname, [&width, &length, &height], ["0", "0", "0", "0"], kind, count));
                self.catalog_items_sql.push(catalog_item_insert(self.target, id, self.page_id, offer_id, &variant_classname));
            }
        }
        Ok(())
    }

    fn remember(&mut self, file_name: &str, settings: FileSettings) {
        self.processed_file_settings.entry(ignore_case_key(file_name)).or_insert(settings);
    }

    fn process_extracted_swf(&mut self, extracted_dir: &Path, file_name: &str) {
        let all_files = get_files_all_directories(extracted_dir).unwrap_or_default();
        let ends_with = |path: &Path, suffix: &str| file_name_lower(path).ends_with(suffix);
        let logic_files: Vec<&PathBuf> = all_files.iter().filter(|f| ends_with(f, "_logic.bin")).collect();
        for logic_file in logic_files {
            if let Err(error) = self.process_logic_file(logic_file, &all_files, file_name) {
                outln!("⚠️ Error processing SWF logic file {}: {error}", logic_file.display());
            }
        }
    }

    /// One `*_logic.bin`; `Ok` also covers the "skip" cases.
    fn process_logic_file(&mut self, logic_file: &Path, all_files: &[PathBuf], file_name: &str) -> Result<()> {
        let stem = file_stem(logic_file);
        let type_attribute = stem.split('_').nth(1).ok_or_else(|| anyhow!("Index was outside the bounds of the array."))?.to_string();
        // Both exporters write into {extractedDir}/binaryData, so this recurses like the logic lookup.
        let Some(visualization_file_path) = all_files
            .iter()
            .filter(|f| file_name_lower(f).ends_with("_visualization.bin"))
            .find(|f| f.to_string_lossy().contains(&type_attribute))
        else {
            return Ok(());
        };

        let interaction_modes_count = calculate_interaction_modes_count(visualization_file_path);
        let xml = read_all_text(logic_file)?;
        let (mut width, mut length, mut stack_height) = dimensions_from_logic_xml(&xml)?;

        let Some(item_data) = self.furni_by_classname.get(file_name).copied() else {
            outln!("❌ {file_name} was not found in the FurnitureData");
            return Ok(());
        };

        let classname = classname_of(item_data).unwrap_or_default();
        let kind = self.type_of(file_name);
        if kind == "unknown" {
            outln!("⚠️ Skipping unknown type for SWF file: {file_name}");
            return Ok(());
        }

        let (sprite_id, offer_id, id) = self.ids_for(item_data)?;

        // Wall items ('i') don't use tile dimensions; store neutral server defaults.
        if kind == "i" {
            width = 1.0;
            length = 1.0;
            stack_height = 0.00;
        }

        let size = [double_to_string(width), double_to_string(length), double_to_string(stack_height)];
        self.items_base_sql.push(items_base_insert(id, sprite_id, &classname, [&size[0], &size[1], &size[2]], ["0", "0", "0", "0"], kind, interaction_modes_count));
        self.catalog_items_sql.push(catalog_item_insert(self.target, id, self.page_id, offer_id, &classname));

        self.remember(file_name, FileSettings { interaction_modes_count, width, length, height: stack_height, file_type: "SWF" });
        Ok(())
    }

    async fn process_nitro_file(&mut self, nitro_file_path: &Path) {
        let file_name = file_stem(nitro_file_path);
        if let Err(error) = self.process_nitro_inner(nitro_file_path, &file_name).await {
            outln!("Error processing Nitro file {file_name}: {error}");
        }
    }

    async fn process_nitro_inner(&mut self, nitro_file_path: &Path, file_name: &str) -> Result<()> {
        let Some(item_data) = self.furni_by_classname.get(file_name).copied() else {
            outln!("❌ {file_name} was not found in the FurnitureData");
            return Ok(());
        };

        let classname = classname_of(item_data).unwrap_or_default();
        let kind = self.type_of(file_name);
        if kind == "unknown" {
            outln!("Skipping unknown type for Nitro file: {file_name}");
            return Ok(());
        }

        let (sprite_id, offer_id, id) = self.ids_for(item_data)?;

        let temp_dir = std::env::temp_dir().join("NitroExtraction").join(file_name);
        if temp_dir.is_dir() {
            std::fs::remove_dir_all(&temp_dir)?;
        }
        std::fs::create_dir_all(&temp_dir)?;

        crate::nitro_file_extractor::extract_into(nitro_file_path, &temp_dir, self.bundle_extract_dir.clone()).await?;
        let nitro_json_path = temp_dir.join(format!("{file_name}.json"));
        let interaction_modes_count = calculate_interaction_modes_count_from_nitro_json(&nitro_json_path);
        let (mut width, mut length, mut height) = get_dimensions_from_nitro_json(&nitro_json_path);

        let flag = |key: &str| -> Result<bool> { item_data.get(key).map(jtoken_to_bool).transpose().map(|b| b.unwrap_or(false)) };
        let can_sit_on = flag("cansiton")?;
        let can_lay_on = flag("canlayon")?;
        let can_stand_on = flag("canstandon")?;

        // Stackable: height != 1.0 and not sittable or layable.
        let is_stackable = height != 1.0 && !can_sit_on && !can_lay_on;

        // Wall items ('i') don't use tile dimensions; store neutral server defaults.
        if kind == "i" {
            width = 1.0;
            length = 1.0;
            height = 0.00;
        }

        let bit = |b: bool| if b { "1" } else { "0" };
        let size = [double_to_string(width), double_to_string(length), double_to_string(height)];
        self.items_base_sql.push(items_base_insert(
            id,
            sprite_id,
            &classname,
            [&size[0], &size[1], &size[2]],
            [bit(is_stackable), bit(can_sit_on), bit(can_lay_on), bit(can_stand_on)],
            kind,
            interaction_modes_count,
        ));
        self.catalog_items_sql.push(catalog_item_insert(self.target, id, self.page_id, offer_id, &classname));

        self.remember(file_name, FileSettings { interaction_modes_count, width, length, height, file_type: "Nitro" });

        std::fs::remove_dir_all(&temp_dir)?;
        Ok(())
    }

    /// The lines of the .sql file.
    pub fn combined_sql(&self) -> Vec<String> {
        let mut combined = vec!["-- items_base inserts".to_string()];
        combined.extend(self.items_base_sql.iter().cloned());
        combined.push(format!("-- {} inserts", catalog_items_table(self.target)));
        combined.extend(self.catalog_items_sql.iter().cloned());
        combined
    }
}

fn file_name_lower(path: &Path) -> String {
    file_name(path).to_lowercase()
}

fn calculate_interaction_modes_count_from_nitro_json(path: &Path) -> i32 {
    if !path.is_file() {
        return 1;
    }
    match read_all_text(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
        Some(root) => interaction_modes_count_from_nitro_json(&root),
        None => 1,
    }
}

fn get_dimensions_from_nitro_json(path: &Path) -> (f64, f64, f64) {
    if !path.is_file() {
        return (1.0, 1.0, 0.00);
    }
    let parsed: Result<Value> = read_all_text(path).map_err(anyhow::Error::from).and_then(|text| Ok(serde_json::from_str(&text)?));
    match parsed {
        Ok(root) => dimensions_from_nitro_json(&root).unwrap_or((1.0, 1.0, 0.00)),
        Err(error) => {
            outln!("❌ Error reading dimensions from Nitro JSON: {error}");
            (1.0, 1.0, 0.00)
        }
    }
}

/// Extracts a SWF's binaryData with the native reader; FFDec (`-export binaryData`) is the
/// fallback for a SWF it cannot read.
async fn extract_swf(swf_file_path: &Path, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let swf = swf_file_path.to_path_buf();
    habbo_swf::extract_with_fallback(
        swf_file_path,
        output_dir,
        move |dir| {
            let swf = swf.clone();
            async move { run_ffdec_binary_data_export(&swf, &dir).await }
        },
        false,
    )
    .await
}

async fn run_ffdec_binary_data_export(swf_file_path: &Path, output_dir: &Path) -> Result<()> {
    let args = ["-onerror".as_ref(), "ignore".as_ref(), "-export".as_ref(), "binaryData".as_ref(), output_dir.as_os_str(), swf_file_path.as_os_str()];
    // A timed-out FFDec is killed; its output is read and dropped.
    if let Err(error) = habbo_core::ffdec::run(args, Duration::from_millis(60000)).await {
        outln!("⚠️ Could not start FFDEC for {} ({error}); this furni is skipped.", file_name(swf_file_path));
    }
    Ok(())
}

fn prompt_int(text: &str) -> Result<i32> {
    out!("{text}");
    let line = habbo_core::console::read_line_opt();
    parse_int(line.as_deref())
}

/// `SQLGenerator.GenerateSQL()` (the Standaard catalog).
pub async fn generate_sql_standard() -> Result<()> {
    generate_sql(FurnitureSqlTarget::Standard).await
}

/// `SQLGenerator.GenerateSQL(FurnitureSqlTarget.BuildersClub)`.
pub async fn generate_sql_builders_club() -> Result<()> {
    generate_sql(FurnitureSqlTarget::BuildersClub).await
}

/// `SQLGenerator.GenerateSQL(FurnitureSqlTarget)`.
pub async fn generate_sql(target: FurnitureSqlTarget) -> Result<()> {
    let base_dir = std::env::current_dir()?.join("Generate");
    let furnidata_dir = base_dir.join("Furnidata");
    move_legacy_furniture_folder(&base_dir)?;
    let furniture_dir = base_dir.join(furniture_folder(target));
    let output_dir = base_dir.join("Output_SQL");
    std::fs::create_dir_all(&output_dir)?;
    std::fs::create_dir_all(&furniture_dir)?;
    let catalog_table = catalog_items_table(target);

    if check_for_duplicate_files(&furniture_dir)? {
        return Ok(());
    }

    if target == FurnitureSqlTarget::BuildersClub {
        outln!("Build Club furniture: Generate/{}/ -> items_base + catalog_items_bc", furniture_folder(target));
    } else {
        outln!("Standaard furniture: Generate/{}/ -> items_base + catalog_items", furniture_folder(target));
    }
    let start_id = prompt_int(&format!("Enter the starting ID for items_base and {catalog_table}: "))?;
    let page_id = prompt_int(if target == FurnitureSqlTarget::BuildersClub {
        "Enter the Catalog_Page ID (catalog_pages_bc) for catalog_items_bc: "
    } else {
        "Enter the Catalog_Page ID (catalog_pages) for catalog_items: "
    })?;

    let furnidata_path = furnidata_dir.join(habbo_core::json::FURNIDATA_FILE);
    // Only a missing file is reported; every other load error ends the run (FileNotFoundException).
    if !furnidata_path.is_dir() && !furnidata_path.is_file() {
        outln!("FurnitureData.json is missing in Generate/Furnidata/.");
        return Ok(());
    }
    let furnidata = Value::Object(habbo_core::json::load_object(&furnidata_path)?);

    let mut generator = Generator::new(&furnidata, target, start_id, page_id)?;
    generator.process(&furniture_dir).await?;

    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let prefix = if target == FurnitureSqlTarget::BuildersClub { "bc_items" } else { "items" };
    let output_path = output_dir.join(format!("{prefix}_{timestamp}.sql"));
    std::fs::write(&output_path, write_all_lines_text(&generator.combined_sql()))?;
    outln!("📦 SQL file generated successfully:\n {}", output_path.display());
    Ok(())
}

/// `File.WriteAllLines`: every line followed by `Environment.NewLine`.
pub(crate) fn write_all_lines_text(lines: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str(NEWLINE);
    }
    text
}

#[cfg(test)]
pub(crate) fn set_bundle_extract_dir(generator: &mut Generator, dir: PathBuf) {
    generator.bundle_extract_dir = Some(dir);
}

#[cfg(test)]
#[path = "sql_generator_tests.rs"]
mod tests;
