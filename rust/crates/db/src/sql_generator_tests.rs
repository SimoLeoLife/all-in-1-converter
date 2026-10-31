use super::*;
use serde_json::json;

const ITEMS: &str = "INSERT INTO `items_base` (`id`, `sprite_id`, `item_name`, `public_name`, `width`, `length`, `stack_height`, `allow_stack`, `allow_sit`, `allow_lay`, `allow_walk`, `allow_gift`, `allow_trade`, `allow_recycle`, `allow_marketplace_sell`, `allow_inventory_stack`, `type`, `interaction_type`, `interaction_modes_count`, `vending_ids`, `multiheight`, `customparams`, `effect_id_male`, `effect_id_female`, `clothing_on_walk`) VALUES\n";
const CATALOG: &str = "INSERT INTO `catalog_items` (`id`, `item_ids`, `page_id`, `offer_id`, `song_id`, `order_number`, `catalog_name`, `cost_credits`, `cost_points`, `points_type`, `amount`, `limited_sells`, `limited_stack`, `extradata`, `have_offer`, `club_only`) VALUES\n";

fn furnidata() -> Value {
    json!({
        "roomitemtypes": { "furnitype": [
            { "id": 100, "classname": "sqltest_chair", "offerid": 5, "cansiton": true },
            { "id": 101, "classname": "sqltest_chair*1" },
            { "classname": "sqltest_chair*2" },
            { "id": 300, "classname": "swfchair" }
        ]},
        "wallitemtypes": { "furnitype": [
            { "id": 200, "classname": "sqltest_poster" }
        ]}
    })
}

#[tokio::test]
async fn nitro_and_hab_furni_become_the_csharp_sql() {
    let dir = tempfile::tempdir().unwrap();
    let furniture_dir = dir.path().join("default_furniture");
    std::fs::create_dir_all(&furniture_dir).unwrap();

    let chair_json = json!({
        "name": "sqltest_chair",
        "logic": { "model": { "dimensions": { "x": 2, "y": 1, "z": 0.5 } } },
        "visualizations": [ { "size": 1 }, { "animations": { "0": {}, "1": {}, "2": {}, "x": {} } } ]
    });
    let hab = habbo_nitro::hab::write(
        "sqltest_chair",
        &[("sqltest_chair.json".into(), serde_json::to_vec(&chair_json).unwrap()), ("sqltest_chair.png".into(), vec![1, 2, 3])],
    )
    .unwrap();
    std::fs::write(furniture_dir.join("sqltest_chair.hab"), hab).unwrap();

    let mut bundler = habbo_nitro::bundler::NitroBundler::new();
    bundler.add_file("sqltest_poster.json", br#"{"name":"sqltest_poster"}"#.to_vec()).unwrap();
    let nitro = bundler.to_buffer().unwrap();
    std::fs::write(furniture_dir.join("sqltest_poster.nitro"), nitro).unwrap();
    std::fs::write(furniture_dir.join("sqltest_unknown.hab"), habbo_nitro::hab::write("x", &[("x.json".into(), b"{}".to_vec())]).unwrap()).unwrap();
    std::fs::write(furniture_dir.join("notes.txt"), b"ignored").unwrap();

    let data = furnidata();
    let mut generator = Generator::new(&data, FurnitureSqlTarget::Standard, 1000, 7).unwrap();
    set_bundle_extract_dir(&mut generator, dir.path().join("extracted"));
    generator.process(&furniture_dir).await.unwrap();

    let text = write_all_lines_text(&generator.combined_sql());
    let nl = NEWLINE;
    let expected = format!(
        "-- items_base inserts{nl}\
{ITEMS}(1000, 100, 'sqltest_chair', 'sqltest_chair', 2, 1, 0.5, '0', '1', '0', '0', '1', '1', '0', '1', '1', 's', 'default', 3, '0', '0', '0', 0, 0, '0');{nl}\
{ITEMS}(1001, 200, 'sqltest_poster', 'sqltest_poster', 1, 1, 0, '1', '0', '0', '0', '1', '1', '0', '1', '1', 'i', 'default', 1, '0', '0', '0', 0, 0, '0');{nl}\
{ITEMS}(1002, 101, 'sqltest_chair*1', 'sqltest_chair*1', 2, 1, 0.5, '0', '0', '0', '0', '1', '1', '0', '1', '1', 's', 'default', 3, '0', '0', '0', 0, 0, '0');{nl}\
{ITEMS}(1004, 1003, 'sqltest_chair*2', 'sqltest_chair*2', 2, 1, 0.5, '0', '0', '0', '0', '1', '1', '0', '1', '1', 's', 'default', 3, '0', '0', '0', 0, 0, '0');{nl}\
-- catalog_items inserts{nl}\
{CATALOG}(1000, '1000', 7, 5, 0, 99, 'sqltest_chair', 5, 0, 0, 1, 0, 0, '', '1', '0');{nl}\
{CATALOG}(1001, '1001', 7, -1, 0, 99, 'sqltest_poster', 5, 0, 0, 1, 0, 0, '', '1', '0');{nl}\
{CATALOG}(1002, '1002', 7, -1, 0, 99, 'sqltest_chair*1', 5, 0, 0, 1, 0, 0, '', '1', '0');{nl}\
{CATALOG}(1004, '1004', 7, -1, 0, 99, 'sqltest_chair*2', 5, 0, 0, 1, 0, 0, '', '1', '0');{nl}"
    );
    assert_eq!(text, expected);
    assert_eq!(generator.start_id, 1005);
    assert!(dir.path().join("extracted").join("sqltest_chair").join("sqltest_chair.png").is_file());
}

#[test]
fn swf_logic_and_visualization() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("binaryData");
    std::fs::create_dir_all(&binary).unwrap();
    std::fs::write(
        binary.join("1_swfchair_swfchair_logic.bin"),
        "\u{feff}<?xml version=\"1.0\"?><objectData type=\"swfchair\"><model><dimensions x=\"1\" y=\"2\" z=\"1.2\" /></model></objectData>",
    )
    .unwrap();
    std::fs::write(
        binary.join("2_swfchair_swfchair_visualization.bin"),
        "<visualizationData><graphics><visualization size=\"64\"><animations><animation id=\"1\"/><animation id=\"3\"/><animation id=\"60\"/></animations></visualization></graphics></visualizationData>",
    )
    .unwrap();

    let data = furnidata();
    let mut generator = Generator::new(&data, FurnitureSqlTarget::BuildersClub, 1, 2).unwrap();
    generator.process_extracted_swf(dir.path(), "swfchair");
    generator.process_variants().unwrap();
    assert_eq!(
        generator.items_base_sql,
        vec![format!("{ITEMS}(1, 300, 'swfchair', 'swfchair', 1, 2, 1.2, '0', '0', '0', '0', '1', '1', '0', '1', '1', 's', 'default', 2, '0', '0', '0', 0, 0, '0');")]
    );
    assert_eq!(
        generator.catalog_items_sql,
        vec!["INSERT INTO `catalog_items_bc` (`id`, `item_ids`, `page_id`, `catalog_name`, `order_number`, `extradata`) VALUES\n(1, '1', 2, 'swfchair', 99, '');".to_string()]
    );
    assert_eq!(generator.combined_sql()[2], "-- catalog_items_bc inserts");
}

#[test]
fn xml_counts_and_dimensions() {
    assert_eq!(interaction_modes_count_from_visualization_xml("<a><animation id=\"0\"/><animation id=\"4\"/></a>"), 5);
    assert_eq!(interaction_modes_count_from_visualization_xml("<a><animation id=\"2\"/><animation id=\"2\"/></a>"), 2);
    assert_eq!(interaction_modes_count_from_visualization_xml("<a></a>"), 1);
    assert_eq!(interaction_modes_count_from_visualization_xml("<a><animation id=\"0\">"), 1);
    assert_eq!(dimensions_from_logic_xml("<o><dimensions x=\"3\"/></o>").unwrap(), (3.0, 1.0, 0.0));
    assert_eq!(dimensions_from_logic_xml("<o/>").unwrap(), (1.0, 1.0, 0.0));
    assert!(dimensions_from_logic_xml("<o><dimensions x=\"abc\"/></o>").is_err());
    assert!(dimensions_from_logic_xml("<o></p>").is_err());
}

#[test]
fn nitro_json_counts_and_dimensions() {
    assert_eq!(interaction_modes_count_from_nitro_json(&json!({})), 1);
    assert_eq!(interaction_modes_count_from_nitro_json(&json!({"visualizations": [{"animations": {"1": {}, "2": {}, "99": {}}}]})), 2);
    assert_eq!(interaction_modes_count_from_nitro_json(&json!({"visualizations": [{"animations": {}}, {"animations": {"0": {}, "5": {}}}]})), 6);
    assert_eq!(dimensions_from_nitro_json(&json!({"logic": {"model": {}}})), None);
    assert_eq!(dimensions_from_nitro_json(&json!({"logic": {"model": {"dimensions": {"y": 3, "z": "x"}}}})), Some((1.0, 3.0, 0.0)));
}

#[test]
fn interaction_type_and_inserts() {
    assert_eq!(interaction_type_for("bc_block_RECOLORABLE"), "recolorable");
    assert_eq!(interaction_type_for("chair"), "default");
    assert_eq!(
        catalog_item_insert(FurnitureSqlTarget::Standard, 3, 4, -1, "o'neil"),
        format!("{CATALOG}(3, '3', 4, -1, 0, 99, 'o'neil', 5, 0, 0, 1, 0, 0, '', '1', '0');")
    );
    assert_eq!(write_all_lines_text(&["a".into(), "b".into()]), format!("a{NEWLINE}b{NEWLINE}"));
}

#[test]
fn furniture_files_rank_and_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    for name in ["b.hab", "b.swf", "a.nitro", "A.hab", "c.txt"] {
        std::fs::write(dir.path().join(name), b"").unwrap();
    }
    std::fs::write(sub.join("b.hab"), b"").unwrap();
    let files = furniture_files(dir.path()).unwrap();
    let names: Vec<String> = files.iter().map(|f| file_name(f)).collect();
    assert_eq!(names, vec!["a.nitro", "b.swf", "b.hab"]);
    assert_eq!(duplicate_names(&files), vec!["b".to_string()]);
}

#[test]
fn legacy_folder_moves_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Furniture")).unwrap();
    std::fs::write(dir.path().join("Furniture").join("x.swf"), b"").unwrap();
    std::fs::create_dir_all(dir.path().join("default_furniture")).unwrap();
    move_legacy_furniture_folder(dir.path()).unwrap();
    assert!(dir.path().join("default_furniture").join("x.swf").is_file());
    assert!(!dir.path().join("Furniture").exists());

    std::fs::create_dir_all(dir.path().join("Furniture")).unwrap();
    move_legacy_furniture_folder(dir.path()).unwrap();
    assert!(!dir.path().join("Furniture").exists());

    std::fs::create_dir_all(dir.path().join("Furniture")).unwrap();
    std::fs::write(dir.path().join("Furniture").join("y.swf"), b"").unwrap();
    move_legacy_furniture_folder(dir.path()).unwrap();
    assert!(dir.path().join("Furniture").join("y.swf").is_file());
}

#[tokio::test]
async fn only_filter_keeps_wanted_classnames_and_variants_of_skipped_bases() {
    let dir = tempfile::tempdir().unwrap();
    let furniture_dir = dir.path().join("default_furniture");
    std::fs::create_dir_all(&furniture_dir).unwrap();
    let chair_json = json!({ "name": "sqlonly_chair", "logic": { "model": { "dimensions": { "x": 2, "y": 1, "z": 0.5 } } } });
    let hab = habbo_nitro::hab::write("sqlonly_chair", &[("sqlonly_chair.json".into(), serde_json::to_vec(&chair_json).unwrap())]).unwrap();
    std::fs::write(furniture_dir.join("sqlonly_chair.hab"), hab).unwrap();

    // Its own furni name: the extraction folder in the system temp is per file name.
    let data = json!({
        "roomitemtypes": { "furnitype": [
            { "id": 100, "classname": "sqlonly_chair", "cansiton": true },
            { "id": 101, "classname": "sqlonly_chair*1" },
            { "classname": "sqlonly_chair*2" }
        ]},
        "wallitemtypes": { "furnitype": [] }
    });
    let mut generator = Generator::new(&data, FurnitureSqlTarget::Standard, 50, 3).unwrap();
    set_bundle_extract_dir(&mut generator, dir.path().join("extracted"));
    let only: HashSet<String> = ["SQLONLY_CHAIR*2".to_string()].into();
    generator.set_only(Some(&only));
    generator.process(&furniture_dir).await.unwrap();

    // The base row is skipped (and uses no id); its variant still gets the base dimensions.
    assert_eq!(
        generator.items_base_sql,
        vec![format!("{ITEMS}(51, 50, 'sqlonly_chair*2', 'sqlonly_chair*2', 2, 1, 0.5, '0', '0', '0', '0', '1', '1', '0', '1', '1', 's', 'default', 1, '0', '0', '0', 0, 0, '0');")]
    );
    assert_eq!(generator.catalog_items_sql, vec![format!("{CATALOG}(51, '51', 3, -1, 0, 99, 'sqlonly_chair*2', 5, 0, 0, 1, 0, 0, '', '1', '0');")]);
    assert_eq!(generator.start_id, 52);
}
