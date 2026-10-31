//! The About page: the converter and its updates, the current Habbo client build, and the
//! credits. A page of its own rather than operation pages: nothing here needs a console.

use super::*;

const DISCORD: &str = "https://discord.gg/txfNucJv";

/// Name and part of each author, newest work first.
const AUTHORS: &[(&str, &str)] = &[
    ("Life", "Rust port of the whole converter, the desktop window with its Hotel and Professional looks, the catalogue ripper, .NET 11 upgrade and the strict JSON-only migration."),
    ("medievalshell", ".NET 10 modernization, cross-platform refactor, ImageSharp migration, the first CLI / TUI / GUI shells and the GitHub Actions pipeline."),
    ("duckietm", "Original all-in-1 downloader, SWF to Nitro pipeline, SQL generator and database tools; upstream maintainer, and author of the All-in-1 Installer whose look the Hotel theme follows."),
];

const CONTRIBUTORS: &[(&str, &str)] = &[
    ("Nitro Team", "Pets converter base"),
    ("AtlasOmega", "\"Among Us\" effects (enable 880-903)"),
    ("Leet", "Enables 500-688"),
];

const STACK: &[&str] = &[
    "Rust", "tokio", "reqwest", "serde_json", "mysql_async", "egui / eframe", "libwebp", "flate2", "JPEXS FFDec", "G-Earth",
    "Ubuntu + Pixelify Sans",
];

impl ProApp {
    pub(super) fn about_page(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let palette = Palette::current(ui.ctx());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            page_margin(ui, |ui| {
                self.about_header(ui, &palette, actions);
                ui.add_space(18.0);
                ui.columns(2, |columns| {
                    // Left-aligned text: columns justify it otherwise.
                    let plain = egui::Layout::top_down(egui::Align::Min);
                    columns[0].with_layout(plain, |ui| self.updates_card(ui, &palette, actions));
                    columns[1].with_layout(plain, |ui| self.habbo_client_card(ui, &palette, actions));
                });
                ui.add_space(18.0);
                credits_card(ui, &palette);
            });
        });
    }

    fn about_header(&self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        card(ui, palette, egui::Margin::same(22), |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
                super::super::icon::paint(ui.painter(), rect);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 230.0);
                    ui.label(title_text(t("All-in-1 Converter"), 22.0));
                    ui.label(
                        RichText::new(t("Download Habbo's assets, convert them to .nitro / .hab, generate their SQL and look after the hotel database, in one program."))
                            .size(13.0)
                            .color(palette.muted),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        chip(ui, &format!("v{}", crate::update::CURRENT_VERSION), palette.accent_soft, palette.accent_text);
                        chip(ui, "Rust", palette.hover, palette.muted);
                        chip(ui, "Windows · Linux", palette.hover, palette.muted);
                        chip(ui, "GPL-3.0", palette.hover, palette.muted);
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(secondary_button("Discord")).clicked() {
                        actions.push(Action::OpenUrl(DISCORD.to_string()));
                    }
                    if ui.add(secondary_button("GitHub")).clicked() {
                        actions.push(Action::OpenUrl(crate::update::repo_url()));
                    }
                });
            });
        });
    }

    fn updates_card(&self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        let update = &self.update;
        card(ui, palette, egui::Margin::same(20), |ui| {
            widgets::heading(ui, palette, t("Updates"), t("New versions of the converter come from its GitHub releases."));
            ui.add_space(12.0);
            value_row(ui, palette, t("Installed version"), |ui| {
                ui.label(RichText::new(crate::update::CURRENT_VERSION).size(15.0).strong());
            });
            value_row(ui, palette, t("Latest version"), |ui| {
                if update.checking.is_some() {
                    ui.spinner();
                    ui.label(RichText::new(t("Checking…")).size(12.5).color(palette.muted));
                } else if let Some(latest) = &update.latest {
                    ui.label(RichText::new(&latest.version).size(15.0).strong());
                    if update.available.is_some() {
                        chip(ui, t("NEW"), palette.warning_back, palette.warning_text);
                    } else {
                        chip(ui, t("UP TO DATE"), palette.success_back, palette.success_text);
                    }
                } else if update.check_error.is_some() {
                    ui.label(RichText::new(t("Could not check")).size(12.5).color(palette.error_text));
                } else if update.checked {
                    ui.label(RichText::new(t("No release published yet")).size(12.5).color(palette.muted));
                } else {
                    ui.label(RichText::new(t("Not checked yet")).size(12.5).color(palette.muted));
                }
            });
            if let Some(error) = &update.check_error {
                ui.label(RichText::new(error).size(11.5).color(palette.error_text));
            }
            if let Some(release) = &update.available {
                let notes = release.notes.trim();
                if !notes.is_empty() {
                    ui.add_space(6.0);
                    egui::Frame::new().fill(palette.hover).corner_radius(6).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let shown: String = notes.lines().take(8).collect::<Vec<_>>().join("\n");
                        ui.label(RichText::new(shown).size(12.0).color(palette.muted));
                    });
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if update.installed.is_some() {
                    if ui.add(primary_button(t("Restart to update"))).clicked() {
                        actions.push(Action::RestartUpdated);
                    }
                    ui.label(RichText::new(t("Update installed.")).size(12.0).color(palette.success_text));
                } else if update.installing.is_some() {
                    ui.spinner();
                    ui.label(RichText::new(t("Downloading the update…")).size(12.0).color(palette.muted));
                } else if let Some(release) = &update.available {
                    let button = primary_button(&tf("Update to {version}", &[("version", &release.version)]));
                    if ui.add_enabled(!self.is_running, button).clicked() {
                        actions.push(Action::InstallUpdate);
                    }
                }
                if update.installing.is_none() && update.installed.is_none() {
                    if ui.add_enabled(update.checking.is_none(), secondary_button(t("Check again"))).clicked() {
                        actions.push(Action::CheckUpdates);
                    }
                }
                if let Some(latest) = &update.latest {
                    if ui.add(secondary_button(t("Release notes"))).clicked() {
                        actions.push(Action::OpenUrl(latest.page_url.clone()));
                    }
                }
            });
            if let Some(error) = &update.error {
                ui.label(RichText::new(error).size(11.5).color(palette.error_text));
            }
        });
    }

    fn habbo_client_card(&self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        let client = &self.habbo_client;
        card(ui, palette, egui::Margin::same(20), |ui| {
            widgets::heading(ui, palette, t("Habbo client"), t("The build of the official Habbo client that habbo.com serves now."));
            ui.add_space(12.0);
            value_row(ui, palette, t("Current build"), |ui| match (&client.busy, &client.build) {
                (Some(_), _) => {
                    ui.spinner();
                    ui.label(RichText::new(t("Checking…")).size(12.5).color(palette.muted));
                }
                (None, Some(Ok(build))) => {
                    ui.label(RichText::new(build).size(14.0).strong().family(egui::FontFamily::Monospace));
                }
                (None, Some(Err(_))) => {
                    ui.label(RichText::new(t("Could not check")).size(12.5).color(palette.error_text));
                }
                (None, None) => {
                    ui.label(RichText::new(t("Not checked yet")).size(12.5).color(palette.muted));
                }
            });
            if let Some(Err(error)) = &client.build {
                ui.label(RichText::new(error).size(11.5).color(palette.error_text));
            }
            ui.label(
                RichText::new(t("Habbo Original downloads match this build; a new build usually means new furniture to import."))
                    .size(12.0)
                    .color(palette.muted),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(client.busy.is_none(), secondary_button(t("Check again"))).clicked() {
                    actions.push(Action::RefreshHabboClient);
                }
                if ui.add_enabled(!self.is_running, secondary_button(t("Import new Habbo furniture"))).clicked() {
                    actions.push(Action::OpenOperation(catalog::get("habbo.import-new")));
                }
            });
        });
    }
}

/// A label on the left and its value on the right of a fixed column.
fn value_row(ui: &mut egui::Ui, palette: &Palette, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(150.0, 24.0), egui::Sense::hover());
        ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), palette.muted);
        value(ui);
    });
    ui.add_space(4.0);
}

fn credits_card(ui: &mut egui::Ui, palette: &Palette) {
    card(ui, palette, egui::Margin::same(20), |ui| {
        widgets::heading(ui, palette, t("Credits and contributors"), t("The people behind the converter, from the first C# version to this Rust one."));
        ui.add_space(12.0);
        ui.columns(2, |columns| {
            let plain = egui::Layout::top_down(egui::Align::Min);
            columns[0].with_layout(plain, |ui| people(ui, palette, t("AUTHORS"), AUTHORS));
            columns[1].with_layout(plain, |ui| people(ui, palette, t("CONTRIBUTORS"), CONTRIBUTORS));
        });
        ui.add_space(6.0);
        ui.label(RichText::new(t("BUILT WITH")).size(10.5).strong().color(palette.subtle));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(ui.available_width());
            for name in STACK {
                chip(ui, name, palette.accent_soft, palette.accent_text);
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new(t("And the whole Habbo retro community. Everything here is free; don't get scammed.")).size(12.0).color(palette.muted));
    });
}

fn people(ui: &mut egui::Ui, palette: &Palette, title: &str, list: &[(&str, &str)]) {
    ui.label(RichText::new(title).size(10.5).strong().color(palette.subtle));
    ui.add_space(4.0);
    for (name, part) in list {
        ui.label(RichText::new(*name).size(13.5).strong());
        ui.label(RichText::new(t(part)).size(12.0).color(palette.muted));
        ui.add_space(6.0);
    }
}
