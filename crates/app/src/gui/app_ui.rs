//! Drawing of the Professional window. Clicks are collected as [`Action`]s and applied after
//! the frame is laid out, so drawing only reads the state.

use super::*;
use super::i18n::{t, tf};

enum Action {
    Navigate(Page),
    OpenOperation(&'static OperationDefinition),
    Select(&'static OperationDefinition),
    Run,
    Queue,
    ClearQueue,
    Cancel,
    OpenBundle,
    ClosePreview,
    OpenLog(std::path::PathBuf),
    DroppedSwf(&'static str, &'static str),
    Done,
    Submit,
    CopyLog,
    Browse,
    SaveWorkspace,
    OpenWorkspaceFolder,
    ChooseMode(RunMode),
    ConfirmDialog,
    CancelDialog,
    OpenSwitch,
    Status(String),
    InstallUpdate,
    RestartUpdated,
}

impl eframe::App for ProApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply(ctx);
        super::screenshot::tick(self, ctx);
        self.take_pending_start(ctx);
        self.drain_events();
        self.poll_workspace();
        self.update.poll();
        if self.update.checking.is_some() || self.update.installing.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.is_running || self.workspace.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        if ctx.input(|i| i.viewport().close_requested()) && self.is_running {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.status_text = t("Wait for the active operation to finish before closing").into();
        }

        let mut actions = Vec::new();
        if self.selector {
            self.selector_ui(ctx, &mut actions);
        } else {
            self.sidebar(ctx, &mut actions);
            self.footer(ctx);
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Palette::current(ctx).surface)).show(ctx, |ui| {
                self.header(ui, &mut actions);
                match self.page {
                    Page::Dashboard => self.dashboard(ui, &mut actions),
                    Page::Workspace => self.workspace_page(ui, &mut actions),
                    Page::Settings => {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            page_margin(ui, |ui| {
                                if let Some(message) = self.settings.ui(ui) {
                                    actions.push(Action::Status(message));
                                }
                            });
                        });
                    }
                    Page::Category(_) => self.operation_page(ui, &mut actions),
                }
            });
            self.dialog_ui(ctx, &mut actions);
            self.preview_ui(ctx, &mut actions);
            self.handle_dropped_files(ctx);
        }

        for action in actions {
            self.apply(ctx, action);
        }
    }
}

fn page_margin<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().inner_margin(egui::Margin { left: 32, right: 32, top: 0, bottom: 18 }).show(ui, add).inner
}

impl ProApp {
    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Navigate(page) => self.navigate(page),
            Action::OpenOperation(operation) => self.open_operation(operation),
            Action::Select(operation) => {
                if !self.is_running {
                    self.clear_log();
                    self.selected = Some(operation);
                }
            }
            Action::Run => {
                if let Some(operation) = self.selected {
                    self.request(operation, After::Run);
                }
            }
            Action::Queue => {
                if let Some(operation) = self.selected {
                    self.request(operation, After::Queue);
                }
            }
            Action::ClearQueue => {
                self.queue.clear();
                self.status_text = t("Queue cleared").into();
            }
            Action::OpenBundle => {
                if let Some(file) = rfd::FileDialog::new().set_title(t("Open a .nitro or .hab bundle")).add_filter("Bundles", &["nitro", "hab"]).pick_file() {
                    self.open_preview(ctx, &file);
                }
            }
            Action::ClosePreview => self.preview = None,
            Action::OpenLog(path) => {
                if let Err(error) = open_path(&path) {
                    self.status_text = tf("Could not open {path}: {error}", &[("path", &path.display().to_string()), ("error", &error.to_string())]);
                }
            }
            Action::DroppedSwf(kind, extension) => {
                if let Some(Dialog::DroppedSwf(files)) = self.dialog.take() {
                    self.convert_dropped(files, kind, extension);
                }
            }
            Action::Cancel => {
                if self.is_running {
                    self.runner.cancel();
                    self.status_text = t("Cancelling…").into();
                }
            }
            Action::Done => self.clear_log(),
            Action::Submit => self.submit_input(),
            Action::CopyLog => {
                let count = self.log.line_count();
                ctx.copy_text(self.log.text().to_string());
                self.status_text = if count == 0 { t("Nothing to copy").into() } else { tf("Copied {count} lines", &[("count", &habbo_core::console::n0(count as i64))]) };
            }
            Action::Browse => {
                if let Some(folder) = rfd::FileDialog::new().set_title(t("Select your nitro-assets folder")).pick_folder() {
                    self.workspace.root_path = folder.display().to_string();
                }
            }
            Action::SaveWorkspace => self.refresh_workspace(true),
            Action::OpenWorkspaceFolder => {
                if let Some(paths) = &self.workspace.paths {
                    if paths.root.is_dir() {
                        let _ = open_path(&paths.root);
                    }
                }
            }
            Action::ChooseMode(mode) => match mode {
                RunMode::Professional => {
                    self.dialog = None;
                    if self.selector {
                        self.selector = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(1040.0, 700.0)));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(1380.0, 860.0)));
                    }
                }
                RunMode::Cli => {
                    crate::cli::request_switch(RunMode::Cli);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                RunMode::Quit => {
                    if self.selector {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    self.dialog = None;
                }
            },
            Action::OpenSwitch => {
                if !self.is_running {
                    self.dialog = Some(Dialog::SwitchInterface);
                }
            }
            Action::ConfirmDialog => match self.dialog.take() {
                Some(Dialog::Workspace(operation, after)) => {
                    if operation.is_destructive {
                        self.dialog = Some(Dialog::Destructive(operation, after));
                    } else {
                        self.confirmed(operation, after, Vec::new());
                    }
                }
                Some(Dialog::Destructive(operation, after)) => self.confirmed(operation, after, Vec::new()),
                Some(Dialog::LeaveSettings(then)) => then(self),
                Some(Dialog::SwitchInterface) | Some(Dialog::DroppedSwf(_)) | None => {}
            },
            Action::CancelDialog => self.dialog = None,
            Action::InstallUpdate => {
                if !self.is_running {
                    self.update.install(&self.runtime);
                }
            }
            Action::RestartUpdated => {
                if let Some(exe) = &self.update.installed {
                    match crate::update::restart(exe, &["--professional"]) {
                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(error) => self.status_text = tf("Could not restart: {error}", &[("error", &error.to_string())]),
                    }
                }
            }
            Action::Status(message) => {
                self.status_text = message;
                self.refresh_hotel();
            }
        }
    }

    // ------------------------------------------------------------ shell

    fn sidebar(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::SidePanel::left("sidebar")
            .exact_width(236.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(Fixed::sidebar()).inner_margin(egui::Margin::symmetric(14, 22)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::hover());
                    logo_tile(ui, rect, 18.0);
                    ui.vertical(|ui| {
                        let name = if theme::hotel() {
                            title_text(t("All-in-1 Converter"), 15.0).color(Fixed::sidebar_accent())
                        } else {
                            RichText::new(t("All-in-1 Converter")).size(15.0).strong().color(Color32::WHITE)
                        };
                        ui.label(name);
                        ui.label(RichText::new(t("Habbo asset workstation")).size(11.5).color(Fixed::sidebar_muted()));
                    });
                });
                ui.add_space(22.0);

                let nav = |ui: &mut egui::Ui, actions: &mut Vec<Action>, icon: &str, label: &'static str, page: Page| {
                    if nav_button(ui, icon, t(label), self.page == page, !self.is_running).clicked() {
                        actions.push(Action::Navigate(page));
                    }
                };
                nav(ui, actions, "🏠", "Dashboard", Page::Dashboard);
                nav(ui, actions, "📁", "Asset Workspace", Page::Workspace);
                sidebar_label(ui, super::i18n::t("MODULES"));
                for (category, label, _, icon, _) in MODULES {
                    nav(ui, actions, icon, label, Page::Category(*category));
                }
                sidebar_label(ui, super::i18n::t("GENERAL"));
                nav(ui, actions, "⚙", "Settings", Page::Settings);
                nav(ui, actions, "ℹ", "About", Page::Category(OperationCategory::General));

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if nav_button(ui, "↔", t("Switch to classic CLI"), false, !self.is_running).clicked() {
                        actions.push(Action::OpenSwitch);
                    }
                    ui.add_space(4.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 0.0, Fixed::sidebar_hover());
                });
            });
    }

    fn header(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let palette = Palette::current(ui.ctx());
        let (title, subtitle) = self.title();
        egui::Frame::new().inner_margin(egui::Margin { left: 32, right: 32, top: 26, bottom: 20 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(title_text(title, 26.0));
                    ui.label(RichText::new(subtitle).size(13.5).color(palette.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.update_banner(ui, &palette, actions);
                    let (text, back, fore) = if self.is_running {
                        (t("Running"), palette.info_back, palette.info_text)
                    } else if !self.has_finished_run {
                        (t("Ready"), palette.hover, palette.muted)
                    } else if self.last_run_succeeded {
                        (t("Completed"), palette.success_back, palette.success_text)
                    } else {
                        (t("Failed"), palette.error_back, palette.error_text)
                    };
                    // The hotel's pale chips need an edge to stand out on its blue surface.
                    let edge = if theme::hotel() { egui::Stroke::new(1.5_f32, theme::with_alpha(fore, 0x80)) } else { egui::Stroke::NONE };
                    egui::Frame::new().fill(back).stroke(edge).corner_radius(14).inner_margin(egui::Margin::symmetric(11, 5)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            dot(ui, fore);
                            ui.label(RichText::new(text).size(12.0).strong().color(fore));
                        });
                    });
                });
            });
        });
    }

    fn footer(&self, ctx: &egui::Context) {
        let palette = Palette::current(ctx);
        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::new().fill(palette.card).stroke(egui::Stroke::new(1.0_f32, palette.card_border)).inner_margin(egui::Margin::symmetric(32, 9)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(&self.status_text).size(12.0).color(palette.muted)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let workspace = match &workspace_runtime::router().paths {
                            None => t("No asset workspace").to_string(),
                            Some(paths) => tf("Workspace: {path}", &[("path", &paths.root.display().to_string())]),
                        };
                        let hotel = match &self.hotel {
                            Some(hotel) => format!("{} ({})", hotel.name, hotel.language),
                            None => t("Custom hotel URLs").into(),
                        };
                        let text = if self.hotel_known { format!("{hotel}   •   {workspace}") } else { workspace };
                        ui.label(RichText::new(text).size(12.0).color(palette.muted));
                        if self.hotel_known {
                            flags::flag(ui, self.hotel.as_ref(), 18.0);
                        }
                    });
                });
            });
    }

    // ------------------------------------------------------------ dashboard

    fn dashboard(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let palette = Palette::current(ui.ctx());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            page_margin(ui, |ui| {
                if theme::hotel() {
                    super::banner::hotel_banner(
                        ui,
                        &palette,
                        t("Welcome to your hotel!"),
                        t("Download, convert and import Habbo assets for your retro: furniture, clothes, pets, effects, the catalogue and the database."),
                    );
                    ui.add_space(22.0);
                }
                // Module tiles
                ui.columns(4, |columns| {
                    for (column, (category, label, caption, icon, color)) in columns.iter_mut().zip(MODULES) {
                        let color = theme::hex(color);
                        let response = card_frame(&palette)
                            .inner_margin(egui::Margin::symmetric(20, 18))
                            .show(column, |ui| {
                                ui.set_width(ui.available_width());
                                let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, 10.0, theme::with_alpha(color, 0x26));
                                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *icon, egui::FontId::proportional(19.0), color);
                                ui.add_space(12.0);
                                ui.label(RichText::new(catalog::for_category(*category).len().to_string()).size(28.0).strong());
                                ui.label(title_text(t(label), 14.0));
                                ui.label(RichText::new(t(caption)).size(12.0).color(palette.muted));
                            })
                            .response
                            .interact(egui::Sense::click());
                        if response.clicked() && !self.is_running {
                            actions.push(Action::Navigate(Page::Category(*category)));
                        }
                        if response.hovered() {
                            column.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
                ui.add_space(18.0);

                ui.columns(2, |columns| {
                    card(&mut columns[0], &palette, egui::Margin::same(12), |ui| {
                        card_heading(ui, &palette, t("Quick actions"), t("Frequently used operations"));
                        for id in ["habbo.import-new", "habbo.all", "tools.swf-furniture-hab", "tools.generate-sql", "database.info"] {
                            let operation = catalog::get(id);
                            if operation_row(ui, &palette, operation, false, true, true).clicked() {
                                actions.push(Action::OpenOperation(operation));
                            }
                        }
                    });
                    card(&mut columns[1], &palette, egui::Margin::same(12), |ui| {
                        card_heading(ui, &palette, t("Recent activity"), t("Last runs; click one to open its log"));
                        if self.history.entries.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(18.0);
                                ui.label(RichText::new(t("🕓")).size(26.0).color(palette.subtle));
                                ui.label(RichText::new(t("No operations run yet.")).size(12.5).color(palette.muted));
                                ui.add_space(18.0);
                            });
                        }
                        for run in self.history.entries.iter().take(8) {
                            let response = ui
                                .horizontal(|ui| {
                                    ui.add_space(12.0);
                                    let color = if run.succeeded {
                                        palette.success_text
                                    } else if run.cancelled {
                                        palette.warning_text
                                    } else {
                                        palette.error_text
                                    };
                                    dot(ui, color);
                                    ui.label(RichText::new(t(&run.title)).size(13.0));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add_space(12.0);
                                        let when = run.finished_local().map_or_else(String::new, |t| {
                                            if t.date_naive() == chrono::Local::now().date_naive() {
                                                t.format("%H:%M").to_string()
                                            } else {
                                                t.format("%d/%m %H:%M").to_string()
                                            }
                                        });
                                        ui.label(RichText::new(when).size(12.0).color(palette.muted));
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if let (true, Some(log)) = (response.clicked(), &run.log_file) {
                                actions.push(Action::OpenLog(log.clone()));
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
                ui.add_space(18.0);

                ui.columns(2, |columns| {
                    card(&mut columns[0], &palette, egui::Margin::same(18), |ui| {
                        ui.horizontal(|ui| {
                            flags::flag(ui, self.hotel.as_ref(), 48.0);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.set_max_width(ui.available_width() - 110.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(self.hotel.map_or(t("Custom hotel URLs"), |h| h.name)).size(14.5).strong());
                                    if let Some(hotel) = &self.hotel {
                                        chip(ui, &hotel.language.to_uppercase(), palette.accent_soft, palette.accent_text);
                                    }
                                });
                                let text = match &self.hotel {
                                    Some(hotel) => tf("{region}. Habbo Original downloads gamedata from this hotel.", &[("region", hotel.region)]),
                                    None if self.hotel_known => t("The gamedata URLs in config.ini point at no single Habbo hotel.").into(),
                                    None => t("config.ini could not be read.").into(),
                                };
                                ui.label(RichText::new(text).size(12.5).color(palette.muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(secondary_button(t("Change"))).clicked() {
                                    actions.push(Action::Navigate(Page::Settings));
                                }
                            });
                        });
                    });
                    card(&mut columns[1], &palette, egui::Margin::same(18), |ui| {
                        let router = workspace_runtime::router();
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(42.0, 42.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 10.0, palette.accent_soft);
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(20.0), palette.accent_text);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.set_max_width(ui.available_width() - 150.0);
                                ui.label(RichText::new(t("Asset workspace")).size(14.5).strong());
                                let text = match &router.paths {
                                    None => t("Not connected. Operations write to their own output folders next to the converter.").to_string(),
                                    Some(paths) => tf("Connected to {path}. Compatible operations read and write there, with backups.", &[("path", &paths.root.display().to_string())]),
                                };
                                ui.label(RichText::new(text).size(12.5).color(palette.muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(secondary_button(if router.paths.is_none() { t("Connect folder") } else { t("Manage") })).clicked() {
                                    actions.push(Action::Navigate(Page::Workspace));
                                }
                            });
                        });
                    });
                });
            });
        });
    }

    // ------------------------------------------------------------ asset workspace

    fn workspace_page(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let palette = Palette::current(ui.ctx());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            page_margin(ui, |ui| {
                card(ui, &palette, egui::Margin::same(20), |ui| {
                    ui.label(RichText::new(t("Nitro assets folder")).size(15.0).strong());
                    ui.label(RichText::new(t("Choose the nitro-assets root. Known folders are detected automatically.")).size(12.5).color(palette.muted));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let buttons_width = 260.0;
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workspace.root_path)
                                .hint_text(r"E:\Users\you\Nitro-Files\nitro-assets")
                                .desired_width(ui.available_width() - buttons_width),
                        );
                        if ui.add(secondary_button(t("Browse…"))).clicked() {
                            actions.push(Action::Browse);
                        }
                        if ui.add_enabled(self.workspace.busy.is_none(), primary_button(t("Save and verify"))).clicked() {
                            actions.push(Action::SaveWorkspace);
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.workspace.status).size(14.0).strong().color(palette.accent_text));
                });
                ui.add_space(16.0);

                if self.workspace.folders.is_empty() {
                    card(ui, &palette, egui::Margin::symmetric(18, 14), |ui| {
                        ui.label(RichText::new(t("No workspace verified yet.")).size(13.0).color(palette.muted));
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for folder in &self.workspace.folders {
                            ui.allocate_ui_with_layout(vec2(300.0, 80.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                card(ui, &palette, egui::Margin::same(16), |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(t(folder.name)).size(14.5).strong());
                                        if folder.exists {
                                            chip(ui, &tf("{count} FILES", &[("count", &habbo_core::console::n0(folder.file_count as i64))]), palette.success_back, palette.success_text);
                                        } else {
                                            chip(ui, t("MISSING"), palette.warning_back, palette.warning_text);
                                        }
                                    });
                                    ui.add(egui::Label::new(RichText::new(folder.path.display().to_string()).size(11.5).color(palette.muted)).truncate());
                                });
                            });
                        }
                    });
                }
                ui.add_space(16.0);

                card(ui, &palette, egui::Margin::same(20), |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(ui.available_width() - 220.0);
                            ui.label(RichText::new(t("Safe file interaction")).size(14.5).strong());
                            ui.label(
                                RichText::new(t("Compatible operations use these folders directly. Existing files are backed up under backups/<date-time> before replacement; paths outside this workspace are rejected."))
                                    .size(12.5)
                                    .color(palette.muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.workspace.paths.is_some(), secondary_button(t("Open workspace folder"))).clicked() {
                                actions.push(Action::OpenWorkspaceFolder);
                            }
                        });
                    });
                });
            });
        });
    }

    // ------------------------------------------------------------ operations

    fn operation_page(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let palette = Palette::current(ui.ctx());
        let Page::Category(category) = self.page else { return };
        let operations = catalog::for_category(category);

        page_margin(ui, |ui| {
            let total = ui.available_width();
            let list_width = (total / 3.0).clamp(260.0, 360.0);
            let height = ui.available_height();
            ui.horizontal_top(|ui| {
                // Left: the operations of the module.
                ui.allocate_ui_with_layout(vec2(list_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    card(ui, &palette, egui::Margin::same(8), |ui| {
                        ui.set_min_height(height - 18.0);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(RichText::new(tf("{count} operations", &[("count", &operations.len().to_string())])).size(12.0).color(palette.muted));
                        });
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().id_salt("operations").auto_shrink([false, false]).show(ui, |ui| {
                            let mut group: Option<&str> = None;
                            for operation in &operations {
                                let next = group_of(operation);
                                if let Some(next) = next {
                                    if Some(next) != group {
                                        ui.add_space(if group.is_none() { 4.0 } else { 14.0 });
                                        ui.horizontal(|ui| {
                                            ui.add_space(12.0);
                                            ui.label(RichText::new(t(next).to_uppercase()).size(10.5).strong().color(palette.subtle));
                                        });
                                    }
                                }
                                group = next;
                                let selected = self.selected == Some(*operation);
                                if operation_row(ui, &palette, operation, selected, !self.is_running, false).clicked() {
                                    actions.push(Action::Select(operation));
                                }
                            }
                        });
                    });
                });

                ui.add_space(16.0);

                // Right: the selected operation, its output and its answer box.
                ui.allocate_ui_with_layout(vec2(ui.available_width(), height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    card(ui, &palette, egui::Margin::symmetric(24, 22), |ui| {
                        ui.set_min_height(height - 44.0);
                        self.operation_detail(ui, &palette, actions);
                    });
                });
            });
        });
    }

    fn operation_detail(&mut self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        let operation = self.selected;
        ui.horizontal_wrapped(|ui| {
            ui.label(title_text(operation.map_or(t("Select an operation"), |o| t(o.title)), 20.0));
            ui.add_space(10.0);
            if operation.is_some_and(|o| o.is_destructive) {
                chip(ui, t("CAUTION"), palette.warning_back, palette.warning_text);
            }
            if operation.is_some_and(|o| o.requires_input) {
                chip(ui, t("ASKS QUESTIONS"), palette.info_back, palette.info_text);
            }
        });
        ui.label(RichText::new(operation.map_or(t("Choose an operation on the left to run it."), |o| t(o.description))).size(13.0).color(palette.muted));
        if let (Some(operation), Some(paths)) = (operation, &workspace_runtime::router().paths) {
            if WORKSPACE_OPERATION_IDS.contains(&operation.id) {
                ui.label(RichText::new(tf("Uses the asset workspace: {path}", &[("path", &paths.root.display().to_string())])).size(12.0).color(palette.accent_text));
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can_run = operation.is_some() && !self.is_running;
            if ui.add_enabled(can_run, primary_button(t("▶  Run operation"))).clicked() {
                actions.push(Action::Run);
            }
            if operation.is_some()
                && ui
                    .add(secondary_button(t("+ Add to queue")))
                    .on_hover_text(t("Runs after the current operation (or now, when nothing runs)"))
                    .clicked()
            {
                actions.push(Action::Queue);
            }
            if self.is_running && ui.add(secondary_button(t("⏹  Cancel"))).clicked() {
                actions.push(Action::Cancel);
            }
            if self.has_finished_run && !self.is_running && ui.add(secondary_button(t("Done"))).clicked() {
                actions.push(Action::Done);
            }
        });

        if !self.queue.is_empty() {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tf("QUEUE ({count})", &[("count", &self.queue.len().to_string())])).size(10.5).strong().color(palette.subtle));
                for (index, run) in self.queue.iter().enumerate() {
                    if index > 0 {
                        ui.label(RichText::new(t("›")).color(palette.subtle));
                    }
                    chip(ui, t(run.operation.title), palette.info_back, palette.info_text);
                }
                if ui.add(egui::Button::new(RichText::new(t("Clear")).size(11.5)).corner_radius(6)).clicked() {
                    actions.push(Action::ClearQueue);
                }
            });
        }

        if self.is_running {
            ui.add_space(12.0);
            let progress = self.log.progress();
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.5, palette.hover);
            let bar = match progress {
                // The operation prints its progress ("(25.0%)", "[ 10/40]"): a real bar.
                Some(fraction) => egui::Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height())),
                // Otherwise a sliding indeterminate bar.
                None => {
                    let time = ui.input(|i| i.time) as f32;
                    let width = rect.width() * 0.25;
                    let x = rect.left() + ((time * 0.6).fract() * (rect.width() + width)) - width;
                    egui::Rect::from_min_max(egui::pos2(x.max(rect.left()), rect.top()), egui::pos2((x + width).min(rect.right()), rect.bottom()))
                }
            };
            ui.painter().rect_filled(bar, 1.5, Fixed::brand());
            if let Some(fraction) = progress {
                ui.label(RichText::new(format!("{:.0}%", fraction * 100.0)).size(11.5).color(palette.muted));
            }
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("OUTPUT")).size(10.5).strong().color(palette.subtle));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new(t("Copy output")).size(12.0)).corner_radius(8)).clicked() {
                    actions.push(Action::CopyLog);
                }
                if ui
                    .add(egui::Button::new(RichText::new(t("Open bundle…")).size(12.0)).corner_radius(8))
                    .on_hover_text(t("Look inside a .nitro or .hab (you can also drop one on the window)"))
                    .clicked()
                {
                    actions.push(Action::OpenBundle);
                }
            });
        });
        ui.add_space(6.0);

        // Any running operation may ask something (e.g. "download G-Earth?"), so the box shows while it runs.
        let needs_input = operation.is_some_and(|o| o.requires_input) || self.is_running;
        let reserved = if needs_input { 52.0 } else { 0.0 };
        let log_height = (ui.available_height() - reserved).max(160.0);
        egui::Frame::new()
            .fill(Fixed::console_background())
            .stroke(egui::Stroke::new(1.0_f32, Fixed::console_border()))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_height(log_height - 22.0);
                ui.set_width(ui.available_width());
                if self.log.is_empty() {
                    ui.label(RichText::new(t("Output appears here when the operation runs.")).monospace().size(12.0).color(theme::hex("#6B7890")));
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("log")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(log_height - 22.0)
                    .show(ui, |ui| {
                        // Coloured like the console; selectable, so lines can be copied.
                        let job = self.log.layout_job(&egui::FontId::monospace(12.0), Fixed::console_text());
                        ui.add(egui::Label::new(job).selectable(true).wrap());
                    });
            });

        if needs_input {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let edit = ui.add_enabled(
                    self.is_running,
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text(t("Answer the prompt and press Enter (empty = default)"))
                        .desired_width(ui.available_width() - 90.0),
                );
                if self.focus_input && self.is_running {
                    edit.request_focus();
                    self.focus_input = false;
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    actions.push(Action::Submit);
                    edit.request_focus();
                }
                if ui.add_enabled(self.is_running, secondary_button(t("Send"))).clicked() {
                    actions.push(Action::Submit);
                }
            });
        }
    }

    // ------------------------------------------------------------ dialogs

    fn dialog_ui(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(dialog) = &self.dialog else { return };
        let palette = Palette::current(ctx);
        let workspace_root = workspace_runtime::router().paths.as_ref().map(|p| p.root.display().to_string()).unwrap_or_default();

        if let Dialog::DroppedSwf(files) = dialog {
            let modal = egui::Modal::new(egui::Id::new("dropped")).show(ctx, |ui| {
                ui.set_width(470.0);
                ui.label(RichText::new(tf("Convert {count} SWF file(s)", &[("count", &files.len().to_string())])).size(18.0).strong());
                ui.label(
                    RichText::new(t("They are copied into SWFCompiler/import/<kind> and converted from there. What are they?"))
                        .size(13.0)
                        .color(palette.muted),
                );
                ui.add_space(10.0);
                for (kind, label) in [("furniture", "Furniture"), ("clothes", "Clothes"), ("pets", "Pets"), ("effects", "Effects")] {
                    ui.horizontal(|ui| {
                        ui.add_sized([110.0, 28.0], egui::Label::new(RichText::new(t(label)).strong()));
                        if ui.add(primary_button(t("to .hab"))).clicked() {
                            actions.push(Action::DroppedSwf(kind, ".hab"));
                        }
                        if ui.add(secondary_button(t("to .nitro"))).clicked() {
                            actions.push(Action::DroppedSwf(kind, ".nitro"));
                        }
                    });
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.add(secondary_button(t("Cancel"))).clicked() {
                        actions.push(Action::CancelDialog);
                    }
                });
            });
            if modal.should_close() && actions.is_empty() {
                actions.push(Action::CancelDialog);
            }
            return;
        }

        if let Dialog::SwitchInterface = dialog {
            let modal = egui::Modal::new(egui::Id::new("switch")).show(ctx, |ui| {
                ui.set_width(760.0);
                self.selector_body(ui, &palette, actions);
            });
            if modal.should_close() {
                actions.push(Action::CancelDialog);
            }
            return;
        }

        let (heading, message, note, confirm, icon, fore, back) = match dialog {
            Dialog::Workspace(operation, _) => (
                t(operation.title).to_string(),
                tf("This operation will interact with the configured Nitro workspace:\n{path}", &[("path", &workspace_root)]),
                t("Existing files written by compatible operations are backed up automatically before replacement."),
                t("Continue with workspace"),
                "📁",
                palette.accent_text,
                palette.accent_soft,
            ),
            Dialog::Destructive(operation, _) => (
                t("Database change").to_string(),
                tf("{operation} can modify your database.", &[("operation", t(operation.title))]),
                t("Confirm only if your configuration and backups are ready."),
                t("Confirm and run"),
                "⚠",
                palette.warning_text,
                palette.warning_back,
            ),
            Dialog::LeaveSettings(_) => (
                t("Unsaved changes").to_string(),
                t("Your config.ini changes are not saved yet.").to_string(),
                t("Discard them, or cancel and press Save changes."),
                t("Discard changes"),
                "⚠",
                palette.warning_text,
                palette.warning_back,
            ),
            Dialog::SwitchInterface | Dialog::DroppedSwf(_) => unreachable!(),
        };

        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(470.0);
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 20.0, back);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(20.0), fore);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(heading).size(18.0).strong());
                    ui.label(RichText::new(message).size(13.0));
                    ui.label(RichText::new(note).size(12.5).color(palette.muted));
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui.add(primary_button(confirm)).clicked() {
                            actions.push(Action::ConfirmDialog);
                        }
                        if ui.add(secondary_button(t("Cancel"))).clicked() {
                            actions.push(Action::CancelDialog);
                        }
                    });
                });
            });
        });
        if modal.should_close() && actions.is_empty() {
            actions.push(Action::CancelDialog);
        }
    }

    fn selector_ui(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let palette = Palette::current(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.surface).inner_margin(egui::Margin { left: 40, right: 40, top: 34, bottom: 28 }))
            .show(ctx, |ui| self.selector_body(ui, &palette, actions));
    }

    /// `InterfaceSelectorWindow`: Professional or Classic CLI.
    fn selector_body(&self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::hover());
            logo_tile(ui, rect, 15.0);
            ui.label(RichText::new(t("All-in-1 Converter")).size(14.0).strong().color(palette.muted));
        });
        ui.add_space(22.0);
        ui.label(title_text(t("Choose your workspace"), 28.0));
        ui.label(RichText::new(t("Both interfaces use the same converter functions and configuration.")).size(14.0).color(palette.muted));
        ui.add_space(24.0);

        ui.columns(2, |columns| {
            let choices = [
                ("Professional", "Dashboard with operation pages, live output and the light or dark system theme.", "🏠", RunMode::Professional, true),
                ("Classic CLI", "Keyboard-only numbered menus for console workflows, scripts and SSH.", "↔", RunMode::Cli, false),
            ];
            for (column, (title, description, icon, mode, recommended)) in columns.iter_mut().zip(choices) {
                let stroke = if recommended { egui::Stroke::new(2.0_f32, Fixed::brand()) } else { egui::Stroke::new(1.0_f32, palette.card_border) };
                let response = egui::Frame::new()
                    .fill(palette.card)
                    .stroke(stroke)
                    .corner_radius(14)
                    .inner_margin(egui::Margin::symmetric(22, 20))
                    .show(column, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(42.0, 42.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 11.0, palette.accent_soft);
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(20.0), palette.accent_text);
                            if recommended {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    chip(ui, t("RECOMMENDED"), palette.accent_soft, palette.accent_text);
                                });
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new(t(title)).size(18.0).strong());
                        ui.label(RichText::new(t(description)).size(13.0).color(palette.muted));
                        ui.add_space(8.0);
                        ui.label(RichText::new(t("Open ›")).size(13.0).strong().color(palette.accent_text));
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.hovered() {
                    column.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    actions.push(Action::ChooseMode(mode));
                }
            }
        });

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("You can switch later from the sidebar, or with 's' in the CLI menu.")).size(12.0).color(palette.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.selector { t("Exit") } else { t("Cancel") };
                if ui.add(secondary_button(label)).clicked() {
                    actions.push(Action::ChooseMode(RunMode::Quit));
                }
            });
        });
    }
}

// ---------------------------------------------------------------- pieces

fn sidebar_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(10.5).strong().color(Fixed::sidebar_muted()));
    });
    ui.add_space(6.0);
}

fn nav_button(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let fill = if selected {
        Fixed::sidebar_selected()
    } else if response.hovered() && enabled {
        Fixed::sidebar_hover()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let text_color = if !enabled { theme::with_alpha(Fixed::sidebar_text(), 110) } else if selected { Color32::WHITE } else { Fixed::sidebar_text() };
    let icon_color = if selected { Fixed::sidebar_accent() } else { text_color };
    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(16.0), icon_color);
    let font = egui::FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
    let pos = rect.left_center() + vec2(41.0, -galley.size().y / 2.0);
    ui.painter().galley(pos, galley, text_color);
    if selected {
        // A faux semi-bold: draw the label once more, one pixel to the right.
        let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.5), text_color);
        ui.painter().galley(pos + vec2(0.4, 0.0), galley, text_color);
    }
    if response.hovered() && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(3.0);
    response
}

fn card_heading(ui: &mut egui::Ui, palette: &Palette, title: &str, subtitle: &str) {
    egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 10 }).show(ui, |ui| {
        ui.label(title_text(title, 15.0));
        ui.label(RichText::new(subtitle).size(12.0).color(palette.muted));
    });
}

/// The logo: a yellow tile with a dark edge in the Hotel look, an indigo tile otherwise.
fn logo_tile(ui: &egui::Ui, rect: egui::Rect, icon_size: f32) {
    let corner = rect.width() * 0.25;
    ui.painter().rect_filled(rect, corner, Fixed::logo());
    if theme::hotel() {
        ui.painter().rect_stroke(rect, corner, egui::Stroke::new(2.0_f32, Fixed::outline()), egui::StrokeKind::Inside);
    }
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "📦", egui::FontId::proportional(icon_size), Fixed::logo_text());
}

/// One operation in a list: title (+ CAUTION), two lines of description, optional chevron.
fn operation_row(ui: &mut egui::Ui, palette: &Palette, operation: &OperationDefinition, selected: bool, enabled: bool, chevron: bool) -> egui::Response {
    let fill = if selected { palette.selected } else { Color32::TRANSPARENT };
    let inner = egui::Frame::new().fill(fill).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 9)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - if chevron { 24.0 } else { 0.0 });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t(operation.title)).size(13.5).strong());
                    if operation.is_destructive {
                        chip(ui, t("CAUTION"), palette.warning_back, palette.warning_text);
                    }
                });
                let mut job = egui::text::LayoutJob::single_section(
                    t(operation.description).to_string(),
                    egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: palette.muted, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping { max_rows: 2, break_anywhere: false, overflow_character: Some('…'), max_width: ui.available_width() };
                ui.label(job);
            });
            if chevron {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(t("›")).size(18.0).color(palette.subtle));
                });
            }
        });
    });
    let response = inner.response.interact(if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    if response.hovered() && enabled && !selected {
        ui.painter().rect_filled(response.rect, 8.0, theme::with_alpha(palette.hover, 120));
    }
    if response.hovered() && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Sections of the long Hotel Tools list, in catalogue order.
fn group_of(operation: &OperationDefinition) -> Option<&'static str> {
    if operation.category != OperationCategory::HotelTools {
        return None;
    }
    let id = operation.id;
    if id.contains("merge") || id.contains("generate-sql") {
        return Some("Gamedata");
    }
    if id == "tools.decompile-swf" {
        return Some("SWF");
    }
    if id.contains("compile") {
        return Some("Bundles");
    }
    for (needle, group) in [("furniture", "Furniture"), ("clothes", "Clothes"), ("pets", "Pets"), ("effects", "Effects")] {
        if id.contains(needle) {
            return Some(group);
        }
    }
    None
}

// ---------------------------------------------------------------- bundle preview and dropped files

impl ProApp {
    fn open_preview(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        match preview::BundlePreview::open(ctx, path) {
            Ok(preview) => self.preview = Some(preview),
            Err(error) => self.status_text = format!("{error:#}"),
        }
    }

    /// Files dropped on the window: a .nitro / .hab opens in the preview, .swf files are converted.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if dropped.is_empty() {
            return;
        }
        let bundles: Vec<_> = dropped.iter().filter(|p| habbo_core::paths::has_ext(p, "nitro") || habbo_core::paths::has_ext(p, "hab")).collect();
        let swfs: Vec<_> = dropped.iter().filter(|p| habbo_core::paths::has_ext(p, "swf")).cloned().collect();
        if let Some(bundle) = bundles.first() {
            let bundle = (*bundle).clone();
            self.open_preview(ctx, &bundle);
        } else if !swfs.is_empty() {
            self.dialog = Some(Dialog::DroppedSwf(swfs));
        } else {
            self.status_text = t("Drop .swf files to convert them, or a .nitro / .hab to look inside").into();
        }
    }

    fn preview_ui(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // A hint while files are dragged over the window.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop-hint")));
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                t("Drop .swf files to convert them, or a .nitro / .hab to look inside"),
                egui::FontId::proportional(20.0),
                Color32::WHITE,
            );
        }

        let Some(preview) = &self.preview else { return };
        let palette = Palette::current(ctx);
        let mut open = true;
        egui::Window::new(tf("Bundle: {name}", &[("name", &preview.name())]))
            .id(egui::Id::new("bundle-preview"))
            .open(&mut open)
            .default_size([820.0, 620.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(RichText::new(preview.path.display().to_string()).size(11.5).color(palette.muted));
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (name, size) in &preview.entries {
                        chip(ui, &format!("{name} · {}", human_size(*size)), palette.hover, palette.text);
                    }
                });
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    columns[0].label(RichText::new(t("SHEET")).size(10.5).strong().color(palette.subtle));
                    match &preview.sheet {
                        Some((texture, size, name)) => {
                            columns[0].label(RichText::new(format!("{name}: {} × {} px", size[0], size[1])).size(12.0).color(palette.muted));
                            egui::ScrollArea::both().id_salt("preview-sheet").max_height(480.0).show(&mut columns[0], |ui| {
                                // Checkerboard-free: the window background shows through transparent pixels.
                                let max = ui.available_width().max(200.0);
                                let scale = (max / size[0] as f32).min(2.0);
                                ui.image((texture.id(), vec2(size[0] as f32 * scale, size[1] as f32 * scale)));
                            });
                        }
                        None => {
                            columns[0].label(RichText::new(t("No sheet in this bundle.")).color(palette.muted));
                        }
                    }
                    columns[1].label(RichText::new(t("JSON")).size(10.5).strong().color(palette.subtle));
                    egui::ScrollArea::both().id_salt("preview-json").max_height(500.0).show(&mut columns[1], |ui| {
                        // A read-only editor keeps the json indentation (a label would trim it).
                        let mut json = preview.json.as_str();
                        ui.add(egui::TextEdit::multiline(&mut json).code_editor().desired_width(f32::INFINITY));
                    });
                });
            });
        if !open {
            actions.push(Action::ClosePreview);
        }
    }
}

fn human_size(bytes: usize) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

// ---------------------------------------------------------------- self-update

impl ProApp {
    /// Right side of the header: an available update, its install progress or the restart button.
    /// Drawn right-to-left, before the status pill.
    fn update_banner(&self, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
        if self.update.installed.is_some() {
            if ui.add(primary_button(t("Restart to update"))).clicked() {
                actions.push(Action::RestartUpdated);
            }
            ui.label(RichText::new(t("Update installed.")).size(12.0).color(palette.success_text));
        } else if self.update.installing.is_some() {
            ui.spinner();
            ui.label(RichText::new(t("Downloading the update…")).size(12.0).color(palette.muted));
        } else if let Some(release) = &self.update.available {
            if ui.add_enabled(!self.is_running, primary_button(&tf("Update to {version}", &[("version", &release.version)]))).on_hover_text(&release.notes).clicked() {
                actions.push(Action::InstallUpdate);
            }
            if let Some(error) = &self.update.error {
                ui.label(RichText::new(error).size(12.0).color(palette.error_text));
            }
        }
        ui.add_space(8.0);
    }
}
