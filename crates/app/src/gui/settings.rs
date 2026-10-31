//! The config.ini editor of the Professional window.

use eframe::egui::{self, Color32, RichText};
use habbo_core::config::ConfigIniFile;
use habbo_core::hotels::{self, HabboHotel};

use super::i18n::{t, tf};
use super::theme::Palette;
use super::widgets::{card, chip, heading, label_block};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Url,
    Password,
    Port,
    Choice(&'static [(&'static str, &'static str)]),
}

struct Field {
    key: &'static str,
    label: &'static str,
    hint: &'static str,
    kind: Kind,
    /// What the converter uses when the key is missing; not written unless changed.
    default: &'static str,
}

const fn field(key: &'static str, label: &'static str, hint: &'static str, kind: Kind, default: &'static str) -> Field {
    Field { key, label, hint, kind, default }
}

const DOWNLOAD_FIELDS: &[Field] = &[
    field("download_format", "Download format", "Furniture, clothes, effects and pets from Habbo",
        Kind::Choice(&[("hab", ".hab only (recommended)"), ("swf", ".swf only"), ("both", ".swf and .hab")]), "both"),
    field("spritesheet_format", "Sprite sheets", "Sheet image inside .nitro and .hab bundles",
        Kind::Choice(&[("webp", "WebP lossless (recommended)"), ("png", "PNG")]), "webp"),
    field("webp_method", "WebP compression", "Every level is lossless; lower is faster with larger files",
        Kind::Choice(&[("6", "Maximum - smallest files (default)"), ("4", "Balanced"), ("1", "Fast - larger files")]), "6"),
    field("pet_libraries", "Pet libraries", "Comma separated. Empty: the built-in list of 36 pets", Kind::Text, ""),
];

const NITRO_FIELDS: &[Field] = &[
    field("nitro_furnidataJSON", "FurnitureData.json", "renderer-config.json: furnidata.url", Kind::Url, ""),
    field("nitro_furnitureurl", "Furniture folder", "renderer-config.json: furni.asset.url", Kind::Url, ""),
    field("nitro_furniture_icon_url", "Furniture icons", "renderer-config.json: furni.asset.icon.url", Kind::Url, ""),
    field("nitro_clothes_dir", "Clothes folder", "renderer-config.json: avatar.asset.url", Kind::Url, ""),
    field("nitro_figuredata", "FigureData.json", "renderer-config.json: avatar.figuredata.url", Kind::Url, ""),
    field("nitro_figuremap", "FigureMap.json", "renderer-config.json: avatar.figuremap.url", Kind::Url, ""),
];

const DATABASE_FIELDS: &[Field] = &[
    field("DATABASESERVER", "Server", "Host name or IP address", Kind::Text, ""),
    field("DATABASEPORT", "Port", "Usually 3306", Kind::Port, "3306"),
    field("DATABASEUSER", "User", "MariaDB / MySQL user", Kind::Text, ""),
    field("DATABASEPASSWORD", "Password", "Stored as plain text in config.ini", Kind::Password, ""),
    field("DATABASENAME", "Database", "Name of the hotel database", Kind::Text, ""),
];

const CDN_FIELDS: &[Field] = &[
    field("furnitureurl", "Furniture (hof_furni)", "SWF and .hab furniture", Kind::Url, ""),
    field("effecturl", "Gordon", "Clothes, effects and pets", Kind::Url, ""),
    field("catalogiconurl", "Catalogue icons", "Prefix, the icon number is appended", Kind::Url, ""),
    field("catalogurl", "Catalogue images", "c_images/catalogue", Kind::Url, ""),
    field("receptionurl", "Reception", "c_images/reception", Kind::Url, ""),
    field("promosmallurl", "Small promos", "c_images/web_promo_small", Kind::Url, ""),
    field("questsurl", "Quests", "c_images/Quests", Kind::Url, ""),
    field("soundmachineurl", "Sound machine", "Prefix of the MP3 samples", Kind::Url, ""),
];

fn all_fields() -> impl Iterator<Item = &'static Field> {
    DOWNLOAD_FIELDS.iter().chain(NITRO_FIELDS).chain(DATABASE_FIELDS).chain(CDN_FIELDS)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Success,
    Warning,
    Error,
}

pub struct SettingsPage {
    file: Option<ConfigIniFile>,
    values: Vec<String>,
    errors: Vec<Option<String>>,
    /// None = custom URLs (keep the current ones).
    hotel: Option<HabboHotel>,
    detected_hotel: Option<HabboHotel>,
    dirty: bool,
    result: (String, Tone),
    cdn_open: bool,
    runtime: tokio::runtime::Handle,
    test: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    test_result: Option<Result<String, String>>,
}

impl SettingsPage {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let mut page = Self {
            file: None,
            values: Vec::new(),
            errors: Vec::new(),
            hotel: None,
            detected_hotel: None,
            dirty: false,
            result: (String::new(), Tone::Muted),
            cdn_open: false,
            runtime,
            test: None,
            test_result: None,
        };
        page.reload();
        page
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Reads config.ini again and drops unsaved edits.
    pub fn reload(&mut self) {
        let file = ConfigIniFile::load(None).ok();
        self.values = all_fields()
            .map(|f| file.as_ref().and_then(|file| file.get(f.key)).unwrap_or_else(|| f.default.to_string()))
            .map(|value| value)
            .collect();
        // A choice the file does not list falls back to the first option, like SelectedIndex 0.
        for (index, f) in all_fields().enumerate() {
            if let Kind::Choice(choices) = f.kind {
                if !choices.iter().any(|(v, _)| v.eq_ignore_ascii_case(&self.values[index])) {
                    self.values[index] = choices[0].0.to_string();
                } else {
                    self.values[index] = self.values[index].to_lowercase();
                }
            }
        }
        self.errors = vec![None; self.values.len()];
        self.detected_hotel = file.as_ref().and_then(|file| hotels::detect(|key| file.get(key)));
        self.hotel = self.detected_hotel;
        self.file = file;
        self.set_dirty(false);
    }

    fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
        if !dirty && self.result.0 == t("Unsaved changes.") {
            self.result.0.clear();
        }
    }

    fn changed(&mut self) {
        self.set_dirty(true);
        self.result = (t("Unsaved changes.").into(), Tone::Warning);
    }

    /// Validates and writes only the changed values. Returns the status-bar message on success.
    pub fn save(&mut self) -> Option<String> {
        if !self.validate() {
            self.result = (t("Fix the highlighted fields first.").into(), Tone::Error);
            return None;
        }
        let file = match ConfigIniFile::load(None) {
            Ok(file) => file,
            Err(error) => {
                self.result = (tf("Could not save config.ini: {error}", &[("error", &error.to_string())]), Tone::Error);
                return None;
            }
        };
        let mut file = file;
        let mut changed = Vec::new();

        let outcome = (|| -> anyhow::Result<()> {
            for (index, f) in all_fields().enumerate() {
                let value = self.values[index].trim().to_string();
                let current = file.get(f.key);
                if current.is_none() && (value.is_empty() || value.eq_ignore_ascii_case(f.default)) {
                    continue;
                }
                if current.as_deref() == Some(value.as_str()) {
                    continue;
                }
                file.set(f.key, &value)?;
                changed.push(f.key.to_string());
            }
            if let Some(hotel) = self.hotel {
                if hotels::detect(|key| file.get(key)) != Some(hotel) {
                    for (key, url) in hotels::urls(&hotel) {
                        file.set(key, &url)?;
                        changed.push(key.to_string());
                    }
                }
            }
            if !changed.is_empty() {
                file.save()?;
            }
            Ok(())
        })();

        if let Err(error) = outcome {
            self.result = (tf("Could not save config.ini: {error}", &[("error", &error.to_string())]), Tone::Error);
            return None;
        }
        if changed.is_empty() {
            self.result = (t("Nothing to save.").into(), Tone::Muted);
            self.set_dirty(false);
            return Some(t("Nothing to save.").into());
        }

        habbo_core::settings::reload();
        self.reload();
        let message = tf("Saved {count} setting(s). The previous file is kept as config.ini.bak.", &[("count", &changed.len().to_string())]);
        self.result = (message.clone(), Tone::Success);
        Some(message)
    }

    fn validate(&mut self) -> bool {
        let mut valid = true;
        for (index, f) in all_fields().enumerate() {
            let value = self.values[index].trim();
            let message = if value.contains(['\r', '\n']) {
                Some(t("Must be on one line."))
            } else {
                match f.kind {
                    Kind::Url if !value.is_empty() && !is_http_url(value) => Some(t("Enter a full http:// or https:// address.")),
                    Kind::Port if !value.parse::<u32>().is_ok_and(|p| (1..=65535).contains(&p)) => Some(t("Enter a port between 1 and 65535.")),
                    _ => None,
                }
            };
            valid &= message.is_none();
            self.errors[index] = message.map(str::to_string);
        }
        valid
    }

    /// Draws the page. Returns a status-bar message when one should be shown.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let palette = Palette::current(ui.ctx());
        let mut status = None;

        // Toolbar
        card(ui, &palette, egui::Margin::symmetric(20, 16), |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(t("config.ini")).size(14.5).strong());
                    let path = ConfigIniFile::default_path();
                    let text = if path.is_file() { path.display().to_string() } else { tf("{path} (not found; saving creates it)", &[("path", &path.display().to_string())]) };
                    ui.label(RichText::new(text).size(12.0).color(palette.muted));
                    if !self.result.0.is_empty() {
                        let color = tone_color(&palette, self.result.1);
                        ui.label(RichText::new(&self.result.0).size(12.5).strong().color(color));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.dirty, super::widgets::primary_button(t("Save changes"))).clicked() {
                        status = self.save();
                    }
                    if ui.add(super::widgets::secondary_button(t("Reload"))).clicked() {
                        self.reload();
                        self.result = (t("Reloaded from disk.").into(), Tone::Muted);
                    }
                    if ui.add(super::widgets::secondary_button(t("Open file"))).clicked() {
                        let path = ConfigIniFile::default_path();
                        if !path.is_file() {
                            self.result = (t("config.ini does not exist yet; save first.").into(), Tone::Error);
                        } else if let Err(error) = super::open_path(&path) {
                            self.result = (tf("Could not open the file: {error}", &[("error", &error.to_string())]), Tone::Error);
                        }
                    }
                });
            });
        });

        self.language_card(ui, &palette);
        self.hotel_card(ui, &palette);

        let mut offset = 0;
        for (title, subtitle, fields, collapsible) in [
            ("Downloads", "How Habbo assets are saved", DOWNLOAD_FIELDS, false),
            ("Nitro retro", "Custom downloads from a Nitro hotel; copy the values from its renderer-config.json", NITRO_FIELDS, false),
            ("Database", "Used by the Database tools", DATABASE_FIELDS, false),
            ("Habbo CDN", "Image and asset hosts; these are the same for every hotel", CDN_FIELDS, true),
        ] {
            self.section(ui, &palette, title, subtitle, fields, offset, collapsible);
            offset += fields.len();
        }
        status
    }

    /// The window language: applied and saved to config.ini (`language`) right away.
    fn language_card(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        card(ui, palette, egui::Margin::same(20), |ui| {
            ui.horizontal(|ui| {
                fixed_column(ui, 240.0, |ui| {
                    label_block(ui, palette, t("Language"), t("Window language; operation output stays in English"))
                });
                let current = super::i18n::current();
                let mut chosen = current.code;
                egui::ComboBox::from_id_salt("language").width(300.0).selected_text(current.native_name).show_ui(ui, |ui| {
                    for lang in super::i18n::LANGUAGES {
                        ui.horizontal(|ui| {
                            let hotel = hotels::ALL.iter().find(|h| h.domain == lang.hotel_domain);
                            super::flags::flag(ui, hotel, 24.0);
                            ui.selectable_value(&mut chosen, lang.code, lang.native_name);
                        });
                    }
                });
                if chosen != current.code {
                    super::i18n::set(chosen);
                    let saved = ConfigIniFile::load(None).and_then(|mut file| {
                        file.set("language", chosen)?;
                        file.save()
                    });
                    if let Err(error) = saved {
                        self.result = (tf("Could not save config.ini: {error}", &[("error", &error.to_string())]), Tone::Error);
                    }
                }
            });
        });
    }

    fn hotel_card(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        card(ui, palette, egui::Margin::same(20), |ui| {
            heading(ui, palette, t("Habbo hotel"), t("Where Habbo Original downloads gamedata (furnidata, productdata, texts and variables) from"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                fixed_column(ui, 240.0, |ui| label_block(ui, palette, t("Hotel"), t("Sets the five gamedata URLs below")));
                let selected_text = match &self.hotel {
                    Some(hotel) => hotel.to_string(),
                    None => t("Custom URLs (keep the current ones)").to_string(),
                };
                let before = self.hotel;
                egui::ComboBox::from_id_salt("hotel")
                    .width(380.0)
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        if self.detected_hotel.is_none() {
                            ui.selectable_value(&mut self.hotel, None, t("Custom URLs (keep the current ones)"));
                        }
                        for hotel in hotels::ALL {
                            ui.horizontal(|ui| {
                                super::flags::flag(ui, Some(hotel), 24.0);
                                ui.selectable_value(&mut self.hotel, Some(*hotel), hotel.to_string());
                            });
                        }
                    });
                if self.hotel != before {
                    self.changed();
                }
            });

            ui.add_space(6.0);
            egui::Frame::new().fill(palette.hover).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                let urls: Vec<(&str, String)> = match &self.hotel {
                    Some(hotel) => hotels::urls(hotel),
                    None => hotels::urls(&hotels::ALL[0])
                        .into_iter()
                        .map(|(key, _)| (key, self.file.as_ref().and_then(|f| f.get(key)).unwrap_or_else(|| "(not set)".into())))
                        .collect(),
                };
                for (key, url) in urls {
                    ui.horizontal(|ui| {
                        fixed_column(ui, 150.0, |ui| ui.label(RichText::new(key).monospace().size(12.0).color(palette.muted)));
                        ui.add(egui::Label::new(RichText::new(url).monospace().size(12.0)).truncate());
                    });
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                chip(ui, t("NOTE"), palette.warning_back, palette.warning_text);
                ui.label(RichText::new(t("Item IDs and offer_id values are not the same across hotels.")).size(12.5).color(palette.muted));
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn section(&mut self, ui: &mut egui::Ui, palette: &Palette, title: &str, subtitle: &str, fields: &[Field], offset: usize, collapsible: bool) {
        card(ui, palette, egui::Margin::same(20), |ui| {
            if collapsible {
                ui.horizontal(|ui| {
                    heading(ui, palette, t(title), t(subtitle));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(super::widgets::secondary_button(if self.cdn_open { t("Hide") } else { t("Show") })).clicked() {
                            self.cdn_open = !self.cdn_open;
                        }
                    });
                });
                if !self.cdn_open {
                    return;
                }
            } else {
                heading(ui, palette, t(title), t(subtitle));
            }
            ui.add_space(8.0);
            for (i, f) in fields.iter().enumerate() {
                let index = offset + i;
                ui.horizontal(|ui| {
                    fixed_column(ui, 240.0, |ui| label_block(ui, palette, t(f.label), t(f.hint)));
                    ui.vertical(|ui| {
                        let changed = match f.kind {
                            Kind::Choice(choices) => {
                                let mut value = self.values[index].clone();
                                let selected = choices.iter().find(|(v, _)| *v == value).map_or(t(choices[0].1), |(_, l)| t(l));
                                egui::ComboBox::from_id_salt(f.key).width(300.0).selected_text(selected).show_ui(ui, |ui| {
                                    for (v, l) in choices {
                                        ui.selectable_value(&mut value, v.to_string(), t(l));
                                    }
                                });
                                let changed = value != self.values[index];
                                self.values[index] = value;
                                changed
                            }
                            kind => {
                                let width = if kind == Kind::Port { 140.0 } else { ui.available_width() };
                                ui.add(egui::TextEdit::singleline(&mut self.values[index]).password(kind == Kind::Password).desired_width(width))
                                    .changed()
                            }
                        };
                        if changed {
                            self.changed();
                        }
                        if let Some(error) = &self.errors[index] {
                            ui.label(RichText::new(error).size(12.0).color(palette.error_text));
                        }
                    });
                });
                ui.add_space(6.0);
            }
            if title == "Database" {
                self.connection_test_row(ui, palette);
            }
        });
    }

    /// "Test connection" with the values typed on the page (saved or not).
    fn connection_test_row(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        if let Some(receiver) = &self.test {
            if let Ok(result) = receiver.try_recv() {
                self.test_result = Some(result);
                self.test = None;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        ui.horizontal(|ui| {
            fixed_column(ui, 240.0, |_| {});
            let testing = self.test.is_some();
            if ui.add_enabled(!testing, super::widgets::secondary_button(t("Test connection"))).clicked() {
                let value = |key: &str| {
                    all_fields().position(|f| f.key == key).map(|i| self.values[i].trim().to_string()).unwrap_or_default()
                };
                let (server, port, user, password, database) =
                    (value("DATABASESERVER"), value("DATABASEPORT"), value("DATABASEUSER"), value("DATABASEPASSWORD"), value("DATABASENAME"));
                let (sender, receiver) = std::sync::mpsc::channel();
                self.runtime.spawn(async move {
                    let result = habbo_db::test_connection(&server, &port, &user, &password, &database).await;
                    let _ = sender.send(result.map_err(|e| format!("{e:#}")));
                });
                self.test = Some(receiver);
                self.test_result = None;
            }
            if testing {
                ui.spinner();
                ui.label(RichText::new(t("Connecting…")).size(12.5).color(palette.muted));
            } else if let Some(result) = &self.test_result {
                let (text, color) = match result {
                    Ok(text) => (format!("✅ {text}"), palette.success_text),
                    Err(error) => (format!("❌ {error}"), palette.error_text),
                };
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
            }
        });
    }
}

fn tone_color(palette: &Palette, tone: Tone) -> Color32 {
    match tone {
        Tone::Muted => palette.muted,
        Tone::Success => palette.success_text,
        Tone::Warning => palette.warning_text,
        Tone::Error => palette.error_text,
    }
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
}

/// A left-aligned column of exactly `width` points (labels line up whatever their length).
fn fixed_column<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        add(ui)
    })
    .inner
}
