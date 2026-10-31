//! The "Rip Habbo catalogue (G-Earth)" operation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use habbo_core::console::n0;
use habbo_core::{outc, outln, Color};

use crate::export::{self, ItemsBase, RippedCatalog, RippedMode};
use crate::gearth::{self, ConnectionInfo, Event, ExtensionInfo, Session};
use crate::images;
use crate::packets::{self, CatalogIndex, CatalogPage, Headers};

/// Everything the operation can be tuned with. [`RipOptions::from_config`] reads config.ini.
#[derive(Debug, Clone)]
pub struct RipOptions {
    /// G-Earth extension ports to try, in order.
    pub ports: Vec<u16>,
    /// The folder the `<hotel>_<timestamp>` output folder is made in.
    pub output_root: PathBuf,
    /// Time between two requests to the server.
    pub delay: Duration,
    /// How long to wait for an answer before retrying / giving up.
    pub response_timeout: Duration,
    /// How long to wait for G-Earth to be running.
    pub gearth_wait: Duration,
    /// How long to wait for the hotel connection, the login and a first answer.
    pub hotel_wait: Duration,
    /// While waiting for a first answer, how often to ask again.
    pub resend_every: Duration,
    pub download_images: bool,
    /// Look product class ids up in the configured database's items_base.
    pub use_database: bool,
    pub catalog_url: String,
    pub icon_url: String,
    /// config.ini `catalog_rip_headers`.
    pub header_overrides: Option<String>,
    /// Catalogue modes to rip; the first one is required.
    pub modes: Vec<String>,
    pub extension: ExtensionInfo,
    /// When G-Earth is not running, offer to start it (downloading it into Tools/G-Earth first).
    pub offer_gearth: bool,
}

impl Default for RipOptions {
    fn default() -> Self {
        Self {
            ports: gearth::default_ports(),
            output_root: PathBuf::from("Catalog"),
            delay: Duration::from_millis(400),
            response_timeout: Duration::from_secs(10),
            gearth_wait: Duration::from_secs(5 * 60),
            hotel_wait: Duration::from_secs(10 * 60),
            resend_every: Duration::from_secs(20),
            download_images: true,
            use_database: true,
            catalog_url: "http://habboo-a.akamaihd.net/c_images/catalogue".into(),
            icon_url: "https://images.habbo.com/c_images/catalogue/icon_".into(),
            header_overrides: None,
            modes: vec!["NORMAL".into(), "BUILDERS_CLUB".into()],
            extension: ExtensionInfo::default(),
            offer_gearth: false,
        }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

impl RipOptions {
    /// The defaults, with config.ini's values: `catalogurl`, `catalogiconurl`,
    /// `catalog_rip_delay_ms` (default 400), `catalog_rip_timeout_ms` (default 10000),
    /// `catalog_rip_headers`, `gearth_extension_port` (only that port). The environment
    /// variables `HABBO_CATALOG_NO_IMAGES=1` and `HABBO_CATALOG_NO_DB=1` turn the image
    /// downloads and the items_base lookup off.
    pub fn from_config() -> Self {
        let mut options = Self::default();
        let settings = habbo_core::ini::app_settings().unwrap_or_else(|_| {
            habbo_core::ini::parse_str(habbo_core::ini::DEFAULT_CONFIG)
                .into_iter()
                .filter_map(|(k, v)| k.strip_prefix("AppSettings:").map(|k| (k.to_string(), v)))
                .collect()
        });
        let get = |key: &str| {
            settings
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        if let Some(url) = get("catalogurl") {
            options.catalog_url = url;
        }
        if let Some(url) = get("catalogiconurl") {
            options.icon_url = url;
        }
        if let Some(ms) = get("catalog_rip_delay_ms").and_then(|v| v.parse::<u64>().ok()) {
            options.delay = Duration::from_millis(ms.max(50));
        }
        if let Some(ms) = get("catalog_rip_timeout_ms").and_then(|v| v.parse::<u64>().ok()) {
            options.response_timeout = Duration::from_millis(ms.max(1000));
        }
        if let Some(port) = get("gearth_extension_port").and_then(|v| v.parse::<u16>().ok()) {
            options.ports = vec![port];
        }
        options.header_overrides = get("catalog_rip_headers");
        options.download_images = !env_flag("HABBO_CATALOG_NO_IMAGES");
        options.use_database = !env_flag("HABBO_CATALOG_NO_DB");
        options.offer_gearth = true;
        options
    }
}

/// What a finished rip wrote.
#[derive(Debug, Clone)]
pub struct RipReport {
    pub output_dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub pages: usize,
    pub pages_failed: usize,
    pub offers: usize,
    pub products: usize,
    pub frontpage_items: usize,
    pub images: images::ImageStats,
    pub sql: export::SqlStats,
    pub complete: bool,
}

/// `Catalog/<hotel>` name for a game host: game-nl.habbo.com -> habbo.nl, game-us -> habbo.com,
/// game-br -> habbo.com.br, game-tr -> habbo.com.tr, anything else the host itself.
pub fn hotel_name(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    let code = host
        .strip_prefix("game-")
        .and_then(|rest| rest.split_once('.'))
        .filter(|(_, domain)| domain.starts_with("habbo."))
        .map(|(code, _)| code);
    let name = match code {
        Some("us") => "habbo.com".to_string(),
        Some("br") => "habbo.com.br".to_string(),
        Some("tr") => "habbo.com.tr".to_string(),
        Some(code) if code.starts_with('s') && code.len() > 2 => format!("habbo.{code}"), // sandbox
        Some(code) => format!("habbo.{code}"),
        None if host.is_empty() => "hotel".to_string(),
        None => host,
    };
    images::safe_file_name(&name)
}

fn progress_label(text: &str) -> String {
    text.replace(['%', '[', ']', '\r', '\n'], " ")
}

// ---------------------------------------------------------------- connecting

async fn connect_gearth(options: &RipOptions) -> Result<Session> {
    let deadline = Instant::now() + options.gearth_wait;
    let mut last_note = Instant::now() - Duration::from_secs(3600);
    let ports = if options.ports.is_empty() {
        gearth::default_ports()
    } else {
        options.ports.clone()
    };
    if options.offer_gearth && !crate::gearth_install::is_running(&ports).await {
        crate::gearth_install::offer_to_start().await?;
    }
    let range = match (ports.first(), ports.last()) {
        (Some(a), Some(b)) if a != b => format!("{a}-{b}"),
        (Some(a), _) => a.to_string(),
        _ => String::new(),
    };
    loop {
        for &port in &ports {
            if let Ok(session) =
                Session::connect(port, &options.extension, Duration::from_secs(3)).await
            {
                outc!(Color::Green, "Connected to G-Earth on port {port}.");
                return Ok(session);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "G-Earth was not found on 127.0.0.1:{range} within {} s. Start G-Earth first (its Extensions tab shows the port; set gearth_extension_port= in config.ini if it is another one).",
                options.gearth_wait.as_secs()
            );
        }
        if now.duration_since(last_note) >= Duration::from_secs(15) {
            last_note = now;
            outln!(
                "Waiting for G-Earth on 127.0.0.1:{range} ... ({} s left)",
                deadline.saturating_duration_since(now).as_secs()
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn wait_for_hotel(session: &mut Session, options: &RipOptions) -> Result<ConnectionInfo> {
    let deadline = Instant::now() + options.hotel_wait;
    let mut accepted = false;
    let mut last_note = Instant::now();
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("No Habbo connection through G-Earth within {} s. Connect G-Earth to the hotel, then start your client and log in.", options.hotel_wait.as_secs());
        }
        if now.duration_since(last_note) >= Duration::from_secs(30) {
            last_note = now;
            outln!("Still waiting for the hotel connection ({} s left): connect G-Earth, then open your Habbo client and log in.", deadline.saturating_duration_since(now).as_secs());
        }
        let wait = deadline
            .saturating_duration_since(now)
            .min(Duration::from_secs(5));
        match tokio::time::timeout(wait, session.next_event()).await {
            Err(_) => continue,
            Ok(None) => bail!("The connection to G-Earth ended."),
            Ok(Some(Event::Init { connected })) => {
                accepted = true;
                outc!(
                    Color::Green,
                    "G-Earth accepted the extension (shown in its Extensions tab as \"{}\").",
                    options.extension.title
                );
                if !connected {
                    outln!("G-Earth is not connected to a hotel yet: press Connect in G-Earth, then open your Habbo client and log in.");
                }
            }
            Ok(Some(Event::ConnectionStart(info))) => return Ok(info),
            Ok(Some(Event::ConnectionEnd)) => {
                outln!("The hotel connection ended; waiting for a new one.")
            }
            Ok(Some(Event::Closed(reason))) => {
                if accepted {
                    bail!("G-Earth closed the connection: {reason}");
                }
                bail!(
                    "G-Earth refused the extension ({reason}). G-Earth asks once whether to allow an unknown extension; \
                     if \"No\" was remembered, restart G-Earth and answer Yes when it asks about \"{}\".",
                    options.extension.title
                );
            }
            Ok(Some(Event::Packet(_))) => {}
        }
    }
}

// ---------------------------------------------------------------- requesting

#[derive(Clone, Debug)]
enum Want {
    Index(String),
    Page(String, i32),
}

enum Pump {
    Got,
    ClientSent,
    Timeout,
}

struct Ripper<'a> {
    session: Session,
    headers: Headers,
    options: &'a RipOptions,
    indexes: HashMap<String, CatalogIndex>,
    pages: HashMap<(String, i32), CatalogPage>,
    /// Pages that answered with something unreadable.
    unreadable: HashMap<(String, i32), String>,
    client_sent: HashSet<u16>,
    /// Request headers the server has answered at least once.
    answered: HashSet<u16>,
    last_send: Option<Instant>,
}

impl<'a> Ripper<'a> {
    fn have(&self, want: &Want) -> bool {
        match want {
            Want::Index(mode) => self.indexes.contains_key(mode),
            Want::Page(mode, id) => {
                self.pages.contains_key(&(mode.clone(), *id))
                    || self.unreadable.contains_key(&(mode.clone(), *id))
            }
        }
    }

    fn handle(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Packet(p) if !p.to_server && p.packet.header == self.headers.index => {
                match packets::parse_index(&p.packet.body) {
                    Ok(index) => {
                        self.indexes.insert(index.mode.to_ascii_uppercase(), index);
                    }
                    Err(e) => outc!(Color::Yellow, "A catalogue index could not be read: {e:#}"),
                }
            }
            Event::Packet(p) if !p.to_server && p.packet.header == self.headers.page => {
                match packets::parse_page(&p.packet.body) {
                    Ok(page) => {
                        self.pages
                            .insert((page.mode.to_ascii_uppercase(), page.page_id), page);
                    }
                    Err(e) => {
                        if let Some((id, mode)) = packets::page_key(&p.packet.body) {
                            self.unreadable
                                .insert((mode.to_ascii_uppercase(), id), format!("{e:#}"));
                        } else {
                            outc!(Color::Yellow, "A catalogue page could not be read: {e:#}");
                        }
                    }
                }
            }
            Event::Packet(p) if p.to_server => {
                self.client_sent.insert(p.packet.header);
            }
            Event::Packet(_) | Event::Init { .. } | Event::ConnectionStart(_) => {}
            Event::ConnectionEnd => {
                bail!("the hotel connection ended (logged out or disconnected)")
            }
            Event::Closed(reason) => bail!("{reason}"),
        }
        Ok(())
    }

    async fn pump(
        &mut self,
        deadline: Instant,
        want: &Want,
        client_header: Option<u16>,
    ) -> Result<Pump> {
        loop {
            if self.have(want) {
                return Ok(Pump::Got);
            }
            if let Some(h) = client_header {
                if self.client_sent.remove(&h) {
                    return Ok(Pump::ClientSent);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(Pump::Timeout);
            }
            match tokio::time::timeout(deadline - now, self.session.next_event()).await {
                Err(_) => return Ok(Pump::Timeout),
                Ok(None) => bail!("the connection to G-Earth ended"),
                Ok(Some(event)) => self.handle(event)?,
            }
        }
    }

    async fn send(&mut self, packet: &crate::wire::Packet) -> Result<()> {
        if let Some(last) = self.last_send {
            let next = last + self.options.delay;
            let now = Instant::now();
            if next > now {
                // Keep reading meanwhile (answers to the client's own requests are cached too).
                let idle = Want::Index("\u{0}".into());
                self.pump(next, &idle, None).await?;
            }
        }
        self.last_send = Some(Instant::now());
        self.session.send_to_server(packet)
    }

    /// Sends a request and waits for its answer. The first time a request kind gets no answer,
    /// the user is told what to do and it is asked again until `hotel_wait` runs out; after
    /// that, a missing answer is retried once and then given up (`Ok(false)`).
    async fn request(
        &mut self,
        want: Want,
        packet: crate::wire::Packet,
        label: &str,
    ) -> Result<bool> {
        if self.have(&want) {
            return Ok(true);
        }
        let header = packet.header;
        self.send(&packet).await?;
        if let Pump::Got = self
            .pump(Instant::now() + self.options.response_timeout, &want, None)
            .await?
        {
            self.answered.insert(header);
            return Ok(true);
        }
        if self.answered.contains(&header) {
            outc!(
                Color::Yellow,
                "No answer for {label} within {:.1} s, asking once more.",
                self.options.response_timeout.as_secs_f64()
            );
            self.send(&packet).await?;
            let got = matches!(
                self.pump(Instant::now() + self.options.response_timeout, &want, None)
                    .await?,
                Pump::Got
            );
            return Ok(got);
        }

        // Never answered yet: not logged in yet, or G-Earth blocks the packet.
        outc!(
            Color::Yellow,
            "Habbo did not answer the {label} request yet."
        );
        outln!("  - Make sure your client is logged in (in the hotel view or a room).");
        outln!("  - G-Earth only forwards packets it knows: if its packet info did not load, open the catalogue in");
        outln!("    your client once and click any page (or turn on Developer mode in G-Earth's Extra tab).");
        outln!(
            "  Waiting up to {} s; the request is repeated every {} s.",
            self.options.hotel_wait.as_secs(),
            self.options.resend_every.as_secs()
        );
        let end = Instant::now() + self.options.hotel_wait;
        self.client_sent.remove(&header);
        loop {
            let now = Instant::now();
            if now >= end {
                bail!(
                    "Habbo never answered the {label} request (waited {} s).",
                    self.options.hotel_wait.as_secs()
                );
            }
            match self
                .pump(
                    (now + self.options.resend_every).min(end),
                    &want,
                    Some(header),
                )
                .await?
            {
                Pump::Got => {
                    self.answered.insert(header);
                    return Ok(true);
                }
                Pump::ClientSent => {
                    // The client sent this request itself, so G-Earth lets it through now.
                    self.send(&packet).await?;
                    let got = matches!(
                        self.pump(Instant::now() + self.options.response_timeout, &want, None)
                            .await?,
                        Pump::Got
                    );
                    self.answered.insert(header);
                    return Ok(got);
                }
                Pump::Timeout => {
                    self.send(&packet).await?;
                    if let Pump::Got = self
                        .pump(Instant::now() + self.options.response_timeout, &want, None)
                        .await?
                    {
                        self.answered.insert(header);
                        return Ok(true);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- database

fn value_i64(value: &mysql_async::Value) -> Option<i64> {
    match value {
        mysql_async::Value::Int(i) => Some(*i),
        mysql_async::Value::UInt(u) => i64::try_from(*u).ok(),
        mysql_async::Value::Bytes(b) => String::from_utf8_lossy(b).trim().parse().ok(),
        _ => None,
    }
}

fn value_string(value: &mysql_async::Value) -> String {
    match value {
        mysql_async::Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        mysql_async::Value::NULL => String::new(),
        other => value_i64(other).map(|v| v.to_string()).unwrap_or_default(),
    }
}

/// items_base of the database in config.ini.
pub async fn load_items_base() -> Result<ItemsBase> {
    use mysql_async::prelude::Queryable;
    let mut conn = tokio::time::timeout(Duration::from_secs(8), habbo_db::config::connect())
        .await
        .map_err(|_| anyhow!("connection timed out"))??;
    let rows: Vec<mysql_async::Row> = match conn
        .query("SELECT id, sprite_id, type, item_name, effect_id_male FROM items_base")
        .await
    {
        Ok(rows) => rows,
        Err(_) => conn
            .query("SELECT id, sprite_id, type, item_name, 0 FROM items_base")
            .await
            .context("reading items_base")?,
    };
    let mut items = ItemsBase::default();
    for row in rows {
        let cell = |i: usize| row.as_ref(i).cloned().unwrap_or(mysql_async::Value::NULL);
        let (Some(id), Some(sprite)) = (value_i64(&cell(0)), value_i64(&cell(1))) else {
            continue;
        };
        let effect = value_i64(&cell(4)).unwrap_or(0);
        items.add(
            id as u32,
            sprite as i32,
            &value_string(&cell(2)),
            &value_string(&cell(3)),
            effect as i32,
        );
    }
    let _ = conn.disconnect().await;
    Ok(items)
}

// ---------------------------------------------------------------- the operation

fn print_instructions(options: &RipOptions) {
    outc!(Color::Cyan, "Rip Habbo catalogue through G-Earth");
    outln!("This reads the catalogue the way your own client does: G-Earth relays the requests through your");
    outln!("logged-in session. Nothing is bought or changed; your game traffic passes through unchanged.");
    outln!();
    outln!("  1. Start G-Earth and connect it to the hotel (Connection tab). Not running? This offers to");
    outln!("     download the official G-Earth into Tools/G-Earth and start it.");
    outln!("  2. Start your Habbo client and log in as usual.");
    outln!(
        "  3. G-Earth may ask whether to allow the extension \"{}\": answer Yes.",
        options.extension.title
    );
    outln!("  4. Keep the catalogue window closed in your client while the rip runs.");
    outln!();
    outln!(
        "Requests are sent one at a time, every {} ms (config.ini catalog_rip_delay_ms).",
        options.delay.as_millis()
    );
    outln!();
}

/// Runs the rip with explicit options (tests, or a caller with its own settings).
pub async fn rip_catalog_with(options: RipOptions) -> Result<RipReport> {
    print_instructions(&options);
    let mut session = connect_gearth(&options).await?;
    let connection = wait_for_hotel(&mut session, &options).await?;
    outc!(
        Color::Green,
        "Hotel connection: {} ({} client, {})",
        connection.host,
        connection.client_type,
        connection.hotel_version
    );
    match connection.client_type.to_ascii_uppercase().as_str() {
        "SHOCKWAVE" => bail!("The Shockwave (Origins) client uses another protocol and has no catalogue pages to rip; use the Flash/AIR client."),
        "UNITY" => outc!(Color::Yellow, "The Unity client is not tested; trying with the Flash structures."),
        _ => {}
    }
    let headers =
        packets::resolve_headers(&connection.packets, options.header_overrides.as_deref())?;
    outln!(
        "Header ids: GetCatalogIndex {}, GetCatalogPage {}, CatalogIndex {}, CatalogPage {}",
        headers.get_index,
        headers.get_page,
        headers.index,
        headers.page
    );
    session.watch(false, headers.index);
    session.watch(false, headers.page);
    session.watch(true, headers.get_index);
    session.watch(true, headers.get_page);
    session.console_log(&format!(
        "[green]{}: ripping the catalogue (read-only)",
        options.extension.title
    ));

    let started = Instant::now();
    let mut ripper = Ripper {
        session,
        headers,
        options: &options,
        indexes: HashMap::new(),
        pages: HashMap::new(),
        unreadable: HashMap::new(),
        client_sent: HashSet::new(),
        answered: HashSet::new(),
        last_send: None,
    };

    let mut aborted: Option<anyhow::Error> = None;
    let mut modes: Vec<RippedMode> = Vec::new();
    for (i, mode) in options.modes.iter().enumerate() {
        let key = mode.to_ascii_uppercase();
        outln!("Requesting the {key} catalogue index ...");
        match ripper
            .request(
                Want::Index(key.clone()),
                packets::get_catalog_index(headers.get_index, mode),
                &format!("{key} index"),
            )
            .await
        {
            Ok(true) => {
                let index = ripper.indexes[&key].clone();
                let count = index.walk().len();
                outln!("{key}: {} pages in the index.", n0(count as i64));
                modes.push(RippedMode {
                    mode: key,
                    index,
                    pages: HashMap::new(),
                    failed: Vec::new(),
                });
            }
            Ok(false) if i == 0 => {
                aborted = Some(anyhow!("Habbo did not send the {key} catalogue index."));
                break;
            }
            Ok(false) => outc!(
                Color::Yellow,
                "{key}: no index (this account or hotel may not have it); skipped."
            ),
            Err(e) => {
                aborted = Some(e);
                break;
            }
        }
    }
    if modes.is_empty() {
        return Err(aborted.unwrap_or_else(|| anyhow!("no catalogue index was received")));
    }

    // Every requestable page, depth first, mode by mode.
    let mut jobs: Vec<(usize, i32, String)> = Vec::new();
    for (m, mode) in modes.iter().enumerate() {
        let mut seen = HashSet::new();
        for (node, _, _) in mode.index.walk() {
            if node.page_id > 0 && seen.insert(node.page_id) {
                let prefix = if mode.mode == "NORMAL" {
                    String::new()
                } else {
                    format!("{}: ", mode.mode)
                };
                let label = if node.localization.is_empty() {
                    node.name.clone()
                } else {
                    format!("{} ({})", node.localization, node.name)
                };
                jobs.push((m, node.page_id, progress_label(&format!("{prefix}{label}"))));
            }
        }
    }
    let total = jobs.len();
    let width = total.to_string().len();
    if aborted.is_none() {
        outln!("Requesting {} pages ...", n0(total as i64));
        for (n, (m, page_id, label)) in jobs.iter().enumerate() {
            let mode = modes[*m].mode.clone();
            outln!(
                "[{:>width$}/{total}] ({:.1}%) {label}",
                n + 1,
                (n + 1) as f64 * 100.0 / total.max(1) as f64
            );
            let want = Want::Page(mode.clone(), *page_id);
            match ripper
                .request(
                    want,
                    packets::get_catalog_page(headers.get_page, *page_id, &mode),
                    &format!("page {page_id}"),
                )
                .await
            {
                Ok(true) => {
                    let key = (mode.clone(), *page_id);
                    if let Some(page) = ripper.pages.get(&key) {
                        modes[*m].pages.insert(*page_id, page.clone());
                    } else if let Some(error) = ripper.unreadable.get(&key) {
                        outc!(Color::Red, "  page {page_id} could not be read: {error}");
                        modes[*m].failed.push(*page_id);
                    }
                }
                Ok(false) => {
                    outc!(Color::Red, "  page {page_id}: no answer, skipped.");
                    modes[*m].failed.push(*page_id);
                }
                Err(e) => {
                    aborted = Some(e);
                    break;
                }
            }
        }
    }
    drop(ripper);
    let rip_time = started.elapsed();

    let catalog = RippedCatalog {
        hotel: hotel_name(&connection.host),
        host: connection.host.clone(),
        hotel_version: connection.hotel_version.clone(),
        client_type: connection.client_type.clone(),
        ripped_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        complete: aborted.is_none(),
        modes,
    };
    let report = write_output(&catalog, &options).await?;
    print_summary(&report, rip_time);
    if let Some(e) = aborted {
        bail!(
            "{e:#}\nThe catalogue read so far was saved in {}.",
            report.output_dir.display()
        );
    }
    Ok(report)
}

async fn write_output(catalog: &RippedCatalog, options: &RipOptions) -> Result<RipReport> {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let output_dir = options
        .output_root
        .join(format!("{}_{stamp}", catalog.hotel));
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("creating {}", output_dir.display()))?;
    let mut files = Vec::new();

    let json_path = output_dir.join("catalog.json");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&export::catalog_json(catalog))?,
    )?;
    files.push(json_path);

    let (items, database_note) = if options.use_database {
        outln!("Reading items_base from the configured database ...");
        match load_items_base().await {
            Ok(items) => {
                outln!("items_base: {} rows.", n0(items.by_sprite.len() as i64));
                (Some(items), "item_ids were matched against items_base of the configured database; products it lacked are looked up again on import (subqueries).".to_string())
            }
            Err(e) => {
                outc!(Color::Yellow, "The database could not be read ({e:#}); item_ids are written as items_base subqueries resolved on import.");
                (None, format!("The database could not be read while ripping ({e:#}): item_ids are subqueries on items_base, resolved when the SQL is imported."))
            }
        }
    } else {
        (None, "The database was not used: item_ids are subqueries on items_base, resolved when the SQL is imported.".to_string())
    };
    let sql = export::arcturus_sql(catalog, items.as_ref());
    for (name, text) in [
        ("catalog_pages.sql", &sql.pages),
        ("catalog_items.sql", &sql.items),
    ] {
        let path = output_dir.join(name);
        std::fs::write(&path, text)?;
        files.push(path);
    }

    let image_stats = if options.download_images {
        outln!("Downloading page images and icons ...");
        let stats = images::download(
            catalog,
            &output_dir,
            &options.catalog_url,
            &options.icon_url,
        )
        .await;
        if stats.downloaded > 0 {
            files.push(output_dir.join("images"));
        }
        stats
    } else {
        images::ImageStats::default()
    };
    let images_text = if options.download_images {
        image_stats.describe()
    } else {
        "not downloaded (turned off)".to_string()
    };

    let readme_path = output_dir.join("README.txt");
    std::fs::write(
        &readme_path,
        export::readme(catalog, &sql.stats, &images_text, &database_note)
            .replace('\n', habbo_core::paths::NEWLINE),
    )?;
    files.push(readme_path);

    let mut report = RipReport {
        output_dir,
        files,
        pages: 0,
        pages_failed: 0,
        offers: 0,
        products: 0,
        frontpage_items: 0,
        images: image_stats,
        sql: sql.stats,
        complete: catalog.complete,
    };
    for mode in &catalog.modes {
        report.pages += mode.pages.len();
        report.pages_failed += mode.failed.len();
        for page in mode.pages.values() {
            report.offers += page.offers.len();
            report.products += page.offers.iter().map(|o| o.products.len()).sum::<usize>();
            report.frontpage_items += page.frontpage_items.len();
        }
    }
    Ok(report)
}

fn print_summary(report: &RipReport, elapsed: Duration) {
    let rule = "================================================================================";
    outln!();
    outc!(Color::Cyan, "{rule}");
    outc!(Color::Cyan, "                    CATALOGUE RIP SUMMARY");
    outc!(Color::Cyan, "{rule}");
    outln!(
        "  Time:                {:.1} min",
        elapsed.as_secs_f64() / 60.0
    );
    outln!("  Pages:               {}", n0(report.pages as i64));
    if report.pages_failed > 0 {
        outc!(
            Color::Yellow,
            "  Pages without answer: {}",
            n0(report.pages_failed as i64)
        );
    }
    outln!("  Offers:              {}", n0(report.offers as i64));
    outln!("  Products:            {}", n0(report.products as i64));
    outln!(
        "  Frontpage items:     {}",
        n0(report.frontpage_items as i64)
    );
    outln!(
        "  SQL rows:            {} catalog_pages, {} catalog_items",
        n0(report.sql.pages as i64),
        n0(report.sql.items as i64)
    );
    outln!(
        "  item_ids:            {} from the database, {} looked up on import",
        n0(report.sql.products_resolved as i64),
        n0(report.sql.products_unresolved as i64)
    );
    outln!("  Images:              {}", report.images.describe());
    let full =
        std::path::absolute(&report.output_dir).unwrap_or_else(|_| report.output_dir.clone());
    outln!("  Output Directory:    {}", full.display());
    for file in &report.files {
        outln!(
            "    {}",
            file.file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    if !report.complete {
        outc!(
            Color::Yellow,
            "  INCOMPLETE: the rip stopped early (see README.txt)."
        );
    }
    outc!(Color::Cyan, "{rule}");
    outln!();
}

/// `Path` helper for tests and callers: the newest rip folder under `root`.
pub fn latest_output(root: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotel_names() {
        assert_eq!(hotel_name("game-nl.habbo.com"), "habbo.nl");
        assert_eq!(hotel_name("game-us.habbo.com"), "habbo.com");
        assert_eq!(hotel_name("game-br.habbo.com"), "habbo.com.br");
        assert_eq!(hotel_name("game-tr.habbo.com"), "habbo.com.tr");
        assert_eq!(hotel_name("127.0.0.1"), "127.0.0.1");
        assert_eq!(hotel_name("my:retro"), "my_retro");
        assert_eq!(hotel_name(""), "hotel");
    }

    #[test]
    fn labels_cannot_fake_progress() {
        assert_eq!(progress_label("50% off [new]"), "50  off  new ");
    }
}
