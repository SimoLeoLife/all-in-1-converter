//! item_ids from a real items_base. Run by hand against a throwaway MariaDB: put a config.ini
//! with its DATABASE* lines in this crate's folder, set HABBO_CATALOG_TEST_KEEP=<folder> and run
//! `cargo test -p habbo-catalog --test database -- --ignored`. The SQL is written to
//! `<folder>/literal` for an import check.

mod support;

use std::collections::HashMap;

use habbo_catalog::export::{arcturus_sql, item_ids_sql, ItemsBase, RippedCatalog, RippedMode};
use habbo_catalog::packets::{parse_index, parse_page};
use support::*;

fn catalog() -> RippedCatalog {
    let index =
        parse_index(&index_packet(INDEX, "NORMAL", &normal_roots(), true, false).body).unwrap();
    let mut pages = HashMap::new();
    for id in [1, 2, 3, 4, 5, 6] {
        pages.insert(
            id,
            parse_page(&page_answer(id, "NORMAL").unwrap().body).unwrap(),
        );
    }
    RippedCatalog {
        hotel: "habbo.nl".into(),
        host: "game-nl.habbo.com".into(),
        hotel_version: "WIN63-test".into(),
        client_type: "FLASH".into(),
        ripped_at: "2026-01-01 00:00:00".into(),
        complete: true,
        modes: vec![RippedMode {
            mode: "NORMAL".into(),
            index,
            pages,
            failed: vec![7],
        }],
    }
}

#[test]
fn literal_item_ids_from_items_base() {
    let mut items = ItemsBase::default();
    items.add(20, 4001, "s", "rare_dragon", 0);
    items.add(10, 4001, "s", "rare_dragon_copy", 0); // lowest id wins
    items.add(11, 4002, "s", "table", 0);
    items.add(12, 5001, "i", "poster", 0);
    items.add(13, 0, "b", "ACH_Test1", 0);
    let catalog = catalog();
    let page = &catalog.modes[0].pages[&3];
    assert_eq!(item_ids_sql(&page.offers[0], Some(&items)).sql, "'10'");
    assert_eq!(item_ids_sql(&page.offers[1], Some(&items)).sql, "'13'");
    assert_eq!(
        item_ids_sql(&page.offers[2], Some(&items)).sql,
        "'11:2;12:1'"
    );
    // 3001 (wallpaper) is unknown: a subquery, the rest literal.
    let ids = item_ids_sql(&page.offers[3], Some(&items));
    assert_eq!((ids.resolved, ids.unresolved), (0, 1));
    let sql = arcturus_sql(&catalog, Some(&items));
    assert_eq!(sql.stats.products_resolved, 4);
    assert_eq!(sql.stats.products_unresolved, 3);
    assert!(sql.items.contains("(300, '10', 3,"), "{}", sql.items);
}

#[tokio::test]
#[ignore = "needs a MariaDB with items_base (config.ini in the crate folder)"]
async fn items_base_from_the_configured_database() {
    let items = habbo_catalog::rip::load_items_base().await.unwrap();
    assert!(!items.by_sprite.is_empty());
    let catalog = catalog();
    let sql = arcturus_sql(&catalog, Some(&items));
    if let Ok(keep) = std::env::var("HABBO_CATALOG_TEST_KEEP") {
        let keep = std::path::Path::new(&keep).join("literal");
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join("catalog_pages.sql"), &sql.pages).unwrap();
        std::fs::write(keep.join("catalog_items.sql"), &sql.items).unwrap();
    }
    println!("{:?}", sql.stats);
}
