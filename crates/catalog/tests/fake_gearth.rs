//! End to end: a fake G-Earth (the extension server side of the protocol) and a fake hotel
//! answering from the emulator-built packets; the real rip runs against them.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use habbo_catalog::gearth::{self, ids, ConnectionInfo, PacketInfo};
use habbo_catalog::wire::{Packet, Reader, Writer};
use habbo_catalog::{rip_catalog_with, RipOptions};
use support::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_frame(s: &mut TcpStream) -> Option<Packet> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.ok()?;
    let mut payload = vec![0u8; i32::from_be_bytes(len) as usize];
    s.read_exact(&mut payload).await.ok()?;
    Some(Packet {
        header: u16::from_be_bytes([payload[0], payload[1]]),
        body: payload[2..].to_vec(),
    })
}

#[derive(Default, Debug)]
struct Log {
    /// (header, page id or -1, mode, time) of every packet the extension sent to the server.
    requests: Vec<(u16, i32, String, Instant)>,
    title: String,
    intercepts_sent: usize,
    intercepts_echoed: usize,
    echo_mismatch: usize,
    other_messages: Vec<u16>,
}

struct Behaviour {
    /// Ignore GetCatalogIndex until the "client" has sent it itself (G-Earth without packet
    /// info / not logged in yet).
    index_locked_until_client: bool,
}

fn packet_infos() -> Vec<PacketInfo> {
    let p = |id: u16, name: &str, out: bool| PacketInfo {
        header_id: id as i32,
        hash: None,
        name: Some(name.into()),
        structure: None,
        to_server: out,
        source: "Sulek".into(),
    };
    vec![
        p(GET_INDEX, "GetCatalogIndex", true),
        p(GET_PAGE, "GetCatalogPage", true),
        p(INDEX, "CatalogIndex", false),
        p(PAGE, "CatalogPage", false),
        p(4000, "ClientHello", true),
        p(999, "Ping", false),
    ]
}

async fn fake_gearth(listener: TcpListener, log: Arc<Mutex<Log>>, behaviour: Behaviour) {
    let (mut s, _) = listener.accept().await.unwrap();
    s.write_all(&Writer::new(ids::INFO_REQUEST).to_bytes())
        .await
        .unwrap();
    let info = read_frame(&mut s).await.unwrap();
    assert_eq!(info.header, ids::EXTENSION_INFO);
    log.lock().unwrap().title = info.reader().string_latin1("title").unwrap();

    // Init (connected) with host info, then the connection start with the packet names.
    let init = Writer::new(ids::INIT)
        .bool(true)
        .string_latin1("G-Earth")
        .string_latin1("1.5.4")
        .i32(1)
        .string_latin1("theme")
        .string_latin1("G-Earth Dark");
    s.write_all(&init.to_bytes()).await.unwrap();
    let conn = ConnectionInfo {
        host: "game-nl.habbo.com".into(),
        port: 30000,
        hotel_version: "WIN63-202501011200-123".into(),
        client_identifier: "".into(),
        client_type: "FLASH".into(),
        packets: packet_infos(),
    };
    s.write_all(&conn.message().to_bytes()).await.unwrap();

    let mut index = 0;
    let mut pending: Vec<Vec<u8>> = Vec::new(); // intercept bodies waiting for their echo
    let mut send_intercept = |s_buf: &mut Vec<u8>,
                              to_server: bool,
                              packet: &Packet,
                              pending: &mut Vec<Vec<u8>>,
                              log: &Arc<Mutex<Log>>| {
        index += 1;
        let msg = gearth::intercept_message(to_server, index, packet);
        pending.push(msg.body.clone());
        log.lock().unwrap().intercepts_sent += 1;
        s_buf.extend_from_slice(&msg.to_bytes());
    };

    // Unrelated game traffic, with bytes that are not valid text, must come back unchanged.
    let mut out = Vec::new();
    send_intercept(
        &mut out,
        false,
        &Writer::new(999).raw(&[0xFF, 0x09, 0x00, 0x80]).packet(),
        &mut pending,
        &log,
    );
    send_intercept(
        &mut out,
        true,
        &Writer::new(4000).string("PRODUCTION").packet(),
        &mut pending,
        &log,
    );
    s.write_all(&out).await.unwrap();

    let mut slow_page_asked = 0;
    let mut client_opened_catalogue = false;
    let mut index_attempts = 0;
    while let Some(frame) = read_frame(&mut s).await {
        let mut out = Vec::new();
        match frame.header {
            ids::MANIPULATED_PACKET => {
                let mut l = log.lock().unwrap();
                l.intercepts_echoed += 1;
                if pending.is_empty() || pending.remove(0) != frame.body {
                    l.echo_mismatch += 1;
                }
            }
            ids::SEND_MESSAGE => {
                let mut r = Reader::new(&frame.body);
                assert_eq!(r.byte("side").unwrap(), 1, "only packets to the server");
                let len = r.i32("len").unwrap() as usize;
                let packet = Packet::from_bytes(r.bytes(len, "packet").unwrap()).unwrap();
                assert_eq!(r.i32("format").unwrap(), 0);
                let mut pr = packet.reader();
                let (page_id, mode) = if packet.header == GET_PAGE {
                    let id = pr.i32("id").unwrap();
                    assert_eq!(pr.i32("offer").unwrap(), -1);
                    (id, pr.string("mode").unwrap())
                } else {
                    (-1, pr.string("mode").unwrap())
                };
                log.lock().unwrap().requests.push((
                    packet.header,
                    page_id,
                    mode.clone(),
                    Instant::now(),
                ));
                match packet.header {
                    GET_INDEX => {
                        index_attempts += 1;
                        if behaviour.index_locked_until_client && !client_opened_catalogue {
                            if index_attempts == 2 {
                                // The user opens the catalogue: the client's own request goes by.
                                client_opened_catalogue = true;
                                send_intercept(
                                    &mut out,
                                    true,
                                    &packets_get_index("NORMAL"),
                                    &mut pending,
                                    &log,
                                );
                            }
                        } else if mode == "NORMAL" {
                            send_intercept(
                                &mut out,
                                false,
                                &index_packet(INDEX, "NORMAL", &normal_roots(), true, false),
                                &mut pending,
                                &log,
                            );
                        } else {
                            send_intercept(
                                &mut out,
                                false,
                                &index_packet(INDEX, &mode, &builders_roots(), false, false),
                                &mut pending,
                                &log,
                            );
                        }
                    }
                    GET_PAGE => {
                        if page_id == 6 && mode == "NORMAL" {
                            slow_page_asked += 1;
                            if slow_page_asked == 1 {
                                continue; // lost: answered only on the retry
                            }
                        }
                        if let Some(answer) = page_answer(page_id, &mode) {
                            send_intercept(&mut out, false, &answer, &mut pending, &log);
                        }
                    }
                    other => panic!("unexpected packet {other} sent to the server"),
                }
            }
            other => log.lock().unwrap().other_messages.push(other),
        }
        if !out.is_empty() && s.write_all(&out).await.is_err() {
            break;
        }
    }
}

fn packets_get_index(mode: &str) -> Packet {
    habbo_catalog::packets::get_catalog_index(GET_INDEX, mode)
}

/// A tiny HTTP server for the image downloads: `<name>.png` exists, `.gif` does not, except
/// for the names listed as gif; icons exist for 1-4.
async fn fake_cdn(listener: TcpListener) {
    loop {
        let Ok((mut s, _)) = listener.accept().await else {
            return;
        };
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let found = (path.starts_with("/c_images/catalogue/catalog_header")
                && path.ends_with(".gif"))
                || (path.starts_with("/c_images/catalogue/")
                    && path.ends_with(".png")
                    && !path.contains("missing"))
                || path.starts_with("/icons/icon_");
            let body = format!("image:{path}");
            let response = if found {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            };
            let _ = s.write_all(response.as_bytes()).await;
        });
    }
}

fn options(port: u16, root: &std::path::Path) -> RipOptions {
    RipOptions {
        ports: vec![port],
        output_root: root.to_path_buf(),
        delay: Duration::from_millis(20),
        response_timeout: Duration::from_millis(400),
        gearth_wait: Duration::from_secs(5),
        hotel_wait: Duration::from_secs(10),
        resend_every: Duration::from_millis(500),
        download_images: false,
        use_database: false,
        modes: vec!["NORMAL".into(), "BUILDERS_CLUB".into()],
        ..RipOptions::default()
    }
}

#[tokio::test]
async fn rips_through_a_fake_gearth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Arc::new(Mutex::new(Log::default()));
    let server = tokio::spawn(fake_gearth(
        listener,
        log.clone(),
        Behaviour {
            index_locked_until_client: false,
        },
    ));
    let cdn = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cdn_port = cdn.local_addr().unwrap().port();
    tokio::spawn(fake_cdn(cdn));

    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(port, dir.path());
    opts.download_images = true;
    opts.catalog_url = format!("http://127.0.0.1:{cdn_port}/c_images/catalogue");
    opts.icon_url = format!("http://127.0.0.1:{cdn_port}/icons/icon_");
    let report = rip_catalog_with(opts).await.unwrap();
    server.await.unwrap();

    // --- protocol
    let log = log.lock().unwrap();
    assert_eq!(log.title, "All-in-1 Converter");
    assert_eq!(log.echo_mismatch, 0);
    assert_eq!(
        log.intercepts_echoed, log.intercepts_sent,
        "every intercepted packet answered"
    );
    assert!(
        log.other_messages
            .iter()
            .all(|&h| h == ids::EXTENSION_CONSOLE_LOG),
        "{:?}",
        log.other_messages
    );
    let requests: Vec<(u16, i32, &str)> = log
        .requests
        .iter()
        .map(|(h, id, m, _)| (*h, *id, m.as_str()))
        .collect();
    assert_eq!(requests[0], (GET_INDEX, -1, "NORMAL"));
    assert_eq!(requests[1], (GET_INDEX, -1, "BUILDERS_CLUB"));
    let pages: Vec<(i32, &str)> = requests[2..]
        .iter()
        .map(|(h, id, m)| {
            assert_eq!(*h, GET_PAGE);
            (*id, *m)
        })
        .collect();
    // depth first; 6 retried once (answered), 7 retried once (never answered); -1 never asked
    assert_eq!(
        pages,
        vec![
            (1, "NORMAL"),
            (2, "NORMAL"),
            (3, "NORMAL"),
            (4, "NORMAL"),
            (5, "NORMAL"),
            (6, "NORMAL"),
            (6, "NORMAL"),
            (7, "NORMAL"),
            (7, "NORMAL"),
            (100, "BUILDERS_CLUB"),
            (4, "BUILDERS_CLUB")
        ]
    );
    for pair in log.requests.windows(2) {
        assert!(
            pair[1].3.duration_since(pair[0].3) >= Duration::from_millis(18),
            "requests are paced"
        );
    }

    // --- report
    assert!(report.complete);
    assert_eq!(report.pages, 8); // 1-6 NORMAL, 100 + 4 BUILDERS_CLUB
    assert_eq!(report.pages_failed, 1);
    assert_eq!(report.offers, 7);
    assert_eq!(report.frontpage_items, 3);
    assert_eq!(report.sql.pages, 9); // 1..7, the disabled page, 100 (4 is BOTH)
    assert_eq!(report.sql.synthetic_page_ids, 1);
    assert_eq!(report.sql.items, 7); // 5 shop offers + color chair + bc table
    let out = &report.output_dir;
    assert!(out
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("habbo.nl_"));

    // --- catalog.json
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(json["hotel"], "habbo.nl");
    assert_eq!(json["client_type"], "FLASH");
    let normal = &json["catalogs"][0];
    assert_eq!(normal["mode"], "NORMAL");
    assert_eq!(normal["failed_pages"], serde_json::json!([7]));
    let front = &normal["pages"][0];
    assert_eq!(front["layout"], "frontpage4");
    assert_eq!(front["frontpage_items"][1]["product_offer_id"], 300);
    let shop = &normal["pages"][1]["children"][0];
    assert_eq!(shop["id"], 3);
    assert_eq!(shop["parent_id"], 2);
    assert_eq!(shop["offers"][0]["products"][0]["limited_series_size"], 100);
    assert_eq!(shop["offers"][0]["products"][0]["limited_items_left"], 63);
    assert_eq!(shop["offers"][1]["products"][0]["product_type"], "b");
    assert_eq!(shop["offers"][2]["item_ids"], "12:2;13:1");
    let hidden = &normal["pages"][1]["children"][2];
    assert_eq!(hidden["visible"], false);
    assert_eq!(hidden["children"][0]["id"], 5);
    assert_eq!(normal["pages"][3]["page_status"], "no answer");
    assert_eq!(json["catalogs"][1]["mode"], "BUILDERS_CLUB");
    assert_eq!(json["catalogs"][1]["pages"][0]["packet_layout"], "classic");

    // --- SQL
    let pages_sql = std::fs::read_to_string(out.join("catalog_pages.sql")).unwrap();
    let items_sql = std::fs::read_to_string(out.join("catalog_items.sql")).unwrap();
    assert!(pages_sql.contains("(1, -1, 'front_page', 'Front Page', 'frontpage', 1, 1, 1, 1, '1', '1', '0', 'NORMAL', '0', 'catalog_frontpage_headline_shop_EN', 'front_teaser', '', 'Redeem a voucher code here:', 'Second text', '', 'Teaser text', 0, '')"), "{pages_sql}");
    assert!(pages_sql.contains("(3, 2, 'shop_3x3', 'Shop 3x3', 'default_3x3', 1, 3, 1, 1, '1', '1', '0', 'NORMAL', '0', 'catalog_header_roombuilder', 'cr_infopic3', 'catalog_special_txtbg1', 'It\\'s a \\\"shop\\\"\\nline 2', '', 'Details \\\\ backslash', 'Teaser', 0, '')"), "{pages_sql}");
    assert!(pages_sql.contains("(4, 2, 'rentables', 'Rentables', 'info_rentables', 1, 4, 1, 2, '1', '1', '0', 'BOTH', '0', 'catalog_rentables_header', '', '', 'Rent one||Rent two'"), "{pages_sql}");
    assert!(pages_sql.contains("(101, 2, 'hidden_cat', 'Hidden category', 'default_3x3', 1, 5, 1, 3, '0', '0', '0', 'NORMAL'"), "{pages_sql}");
    assert!(pages_sql.contains("(5, 101, 'hidden_child'"), "{pages_sql}"); // disabled page: new id above the highest
    // recolorable_furniture is in Polaris' enum (migration V20261014120000): kept as is.
    assert!(pages_sql.contains("(5, 101, 'hidden_child', 'Hidden child', 'recolorable_furniture'"), "{pages_sql}");
    assert!(!pages_sql.contains("layout 'recolorable_furniture' is not in"), "{pages_sql}");
    assert!(pages_sql.contains("(100, -1, 'bc_addons', 'BC Addons', 'builders_club_addons', 1, 9, 1, 1, '1', '1', '0', 'BUILDER'"), "{pages_sql}");
    assert!(pages_sql.contains("(6, -1, 'slow_page', 'Slow page', 'pets', 1, 7, 1, 3, '1', '1', '0', 'NORMAL', '0', 'pets_h', 'pets_t', '', 'p1', 'p2', 'pd', 'pt'"), "{pages_sql}");
    assert!(items_sql.contains("(300, CONCAT_WS(';', (SELECT `id` FROM `items_base` WHERE `type` = 's' AND `sprite_id` = 4001 ORDER BY `id` LIMIT 1)), 3, 'rare_dragon_ltd', 50, 0, 0, 1, 100, 37, 1, 300, 0, '', '1', '0')"), "{items_sql}");
    assert!(items_sql.contains("(301, CONCAT_WS(';', (SELECT `id` FROM `items_base` WHERE `type` = 'b' AND `item_name` = 'ACH_Test1' ORDER BY `id` LIMIT 1)), 3, 'badge_ACH_Test', 0, 25, 5, 1, 0, 0, 2, 301, 0, 'ACH_Test1', '1', '0')"), "{items_sql}");
    assert!(items_sql.contains("(302, CONCAT_WS(';', (SELECT CONCAT(`id`, ':2') FROM `items_base` WHERE `type` = 's' AND `sprite_id` = 4002 ORDER BY `id` LIMIT 1), (SELECT CONCAT(`id`, ':1') FROM `items_base` WHERE `type` = 'i' AND `sprite_id` = 5001 ORDER BY `id` LIMIT 1)), 3, 'bundle_o\\'brien', 25, 0, 0, 1, 0, 0, 3, 302, 0, '', '0', '1')"), "{items_sql}");
    assert!(
        items_sql.contains(", 3, 'chair_x5', 5, 0, 0, 5, 0, 0, 5, 304,"),
        "{items_sql}"
    );

    // --- images: header as gif, others png, icons, frontpage promos; missing ones counted
    let images = out.join("images");
    assert!(images.join("catalog_header_roombuilder.gif").is_file());
    assert!(images.join("cr_infopic3.png").is_file());
    assert!(images.join("icons/icon_3.png").is_file());
    assert!(images.join("frontpage/feature_cata_vert_new.png").is_file());
    assert_eq!(
        std::fs::read_to_string(images.join("frontpage/feature_cata_vert_new.png")).unwrap(),
        "image:/c_images/catalogue/feature_cata_vert_new.png"
    );
    assert_eq!(report.images.failed, 0);
    assert!(report.images.downloaded >= 20, "{:?}", report.images);

    let readme = std::fs::read_to_string(out.join("README.txt")).unwrap();
    assert!(readme.contains("Import catalog_pages.sql, then catalog_items.sql"));
    assert!(!readme.contains("recolorable_furniture (1)"));

    // Keep the files for a manual import check (see tests/database.rs).
    if let Ok(keep) = std::env::var("HABBO_CATALOG_TEST_KEEP") {
        let keep = std::path::Path::new(&keep).join("subquery");
        std::fs::create_dir_all(&keep).unwrap();
        for name in [
            "catalog.json",
            "catalog_pages.sql",
            "catalog_items.sql",
            "README.txt",
        ] {
            std::fs::copy(out.join(name), keep.join(name)).unwrap();
        }
    }
}

#[tokio::test]
async fn waits_until_the_client_opened_the_catalogue() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Arc::new(Mutex::new(Log::default()));
    let server = tokio::spawn(fake_gearth(
        listener,
        log.clone(),
        Behaviour {
            index_locked_until_client: true,
        },
    ));
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(port, dir.path());
    opts.modes = vec!["NORMAL".into()];
    let report = rip_catalog_with(opts).await.unwrap();
    server.await.unwrap();
    assert_eq!(report.pages, 6);
    let log = log.lock().unwrap();
    let index_requests = log.requests.iter().filter(|r| r.0 == GET_INDEX).count();
    assert_eq!(
        index_requests, 3,
        "first try, the periodic retry, then again after the client's own request"
    );
}

#[tokio::test]
async fn fails_clearly_without_gearth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(port, dir.path());
    opts.gearth_wait = Duration::from_millis(100);
    let err = rip_catalog_with(opts).await.unwrap_err().to_string();
    assert!(err.contains("G-Earth was not found"), "{err}");
}

#[tokio::test]
async fn hotel_disconnect_keeps_partial_output() {
    // G-Earth announces the connection, answers the index, then the hotel connection ends.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(&Writer::new(ids::INFO_REQUEST).to_bytes())
            .await
            .unwrap();
        read_frame(&mut s).await.unwrap();
        let conn = ConnectionInfo {
            host: "game-de.habbo.com".into(),
            port: 1,
            hotel_version: "v".into(),
            client_identifier: "".into(),
            client_type: "FLASH".into(),
            packets: packet_infos(),
        };
        s.write_all(
            &Writer::new(ids::INIT)
                .bool(true)
                .string_latin1("G-Earth")
                .string_latin1("1")
                .i32(0)
                .to_bytes(),
        )
        .await
        .unwrap();
        s.write_all(&conn.message().to_bytes()).await.unwrap();
        while let Some(frame) = read_frame(&mut s).await {
            if frame.header == ids::SEND_MESSAGE {
                let mut r = Reader::new(&frame.body);
                r.byte("side").unwrap();
                let len = r.i32("len").unwrap() as usize;
                let packet = Packet::from_bytes(r.bytes(len, "p").unwrap()).unwrap();
                if packet.header == GET_INDEX {
                    let msg = gearth::intercept_message(
                        false,
                        1,
                        &index_packet(INDEX, "NORMAL", &normal_roots(), false, false),
                    );
                    s.write_all(&msg.to_bytes()).await.unwrap();
                } else {
                    s.write_all(&Writer::new(ids::CONNECTION_END).to_bytes())
                        .await
                        .unwrap();
                }
            }
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let mut opts = options(port, dir.path());
    opts.modes = vec!["NORMAL".into()];
    let err = rip_catalog_with(opts).await.unwrap_err().to_string();
    assert!(err.contains("hotel connection ended"), "{err}");
    assert!(err.contains("saved in"), "{err}");
    let out = habbo_catalog::rip::latest_output(dir.path()).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(json["complete"], false);
    assert!(std::fs::read_to_string(out.join("README.txt"))
        .unwrap()
        .contains("INCOMPLETE"));
    assert!(out.join("catalog_pages.sql").is_file());
}
