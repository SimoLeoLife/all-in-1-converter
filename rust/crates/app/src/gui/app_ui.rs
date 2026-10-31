//! Drawing of the Professional window. Clicks are collected as [`Action`]s and applied after
//! the frame is laid out, so drawing only reads the state.

use super::*;

enum Action {
    Navigate(Page),
    OpenOperation(&'static OperationDefinition),
    Select(&'static OperationDefinition),
    Run,
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
}

impl eframe::App for ProApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply(ctx);
        super::screenshot::tick(self, ctx);
        self.take_pending_start(ctx);
        self.drain_events();
        self.poll_workspace();
        if self.is_running || self.workspace.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        if ctx.input(|i| i.viewport().close_requested()) && self.is_running {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.status_text = "Wait for the active operation to finish before closing".into();
        }

        let mut actions = Vec::new();
        if self.selector {
            self.selector_ui(ctx, &mut actions);
        } else {
            self.sidebar(ctx, &mut actions);
            self.footer(ctx);
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Palette::current(ctx).surface)).show(ctx, |ui| {
                self.header(ui);
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
            Action::Run => self.request_run(),
            Action::Done => self.clear_log(),
            Action::Submit => self.submit_input(),
            Action::CopyLog => {
                let count = self.log.line_count();
                ctx.copy_text(self.log.text().to_string());
                self.status_text = if count == 0 { "Nothing to copy".into() } else { format!("Copied {} lines", habbo_core::console::n0(count as i64)) };
            }
            Action::Browse => {
                if let Some(folder) = rfd::FileDialog::new().set_title("Select your nitro-assets folder").pick_folder() {
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
                Some(Dialog::Workspace(operation)) => {
                    if operation.is_destructive {
                        self.dialog = Some(Dialog::Destructive(operation));
                    } else {
                        self.start(operation);
                    }
                }
                Some(Dialog::Destructive(operation)) => self.start(operation),
                Some(Dialog::LeaveSettings(then)) => then(self),
                Some(Dialog::SwitchInterface) | None => {}
            },
            Action::CancelDialog => self.dialog = None,
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
                    ui.painter().rect_filled(rect, 10.0, theme::hex("#5A68EE"));
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "📦", egui::FontId::proportional(18.0), Color32::WHITE);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("All-in-1 Converter").size(15.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("Habbo asset workstation").size(11.5).color(Fixed::sidebar_muted()));
                    });
                });
                ui.add_space(22.0);

                let nav = |ui: &mut egui::Ui, actions: &mut Vec<Action>, icon: &str, label: &str, page: Page| {
                    if nav_button(ui, icon, label, self.page == page, !self.is_running).clicked() {
                        actions.push(Action::Navigate(page));
                    }
                };
                nav(ui, actions, "🏠", "Dashboard", Page::Dashboard);
                nav(ui, actions, "📁", "Asset Workspace", Page::Workspace);
                sidebar_label(ui, "MODULES");
                for (category, label, _, icon, _) in MODULES {
                    nav(ui, actions, icon, label, Page::Category(*category));
                }
                sidebar_label(ui, "GENERAL");
                nav(ui, actions, "⚙", "Settings", Page::Settings);
                nav(ui, actions, "ℹ", "About", Page::Category(OperationCategory::General));

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if nav_button(ui, "↔", "Switch to classic CLI", false, !self.is_running).clicked() {
                        actions.push(Action::OpenSwitch);
                    }
                    ui.add_space(4.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 0.0, Fixed::sidebar_hover());
                });
            });
    }

    fn header(&self, ui: &mut egui::Ui) {
        let palette = Palette::current(ui.ctx());
        let (title, subtitle) = self.title();
        egui::Frame::new().inner_margin(egui::Margin { left: 32, right: 32, top: 26, bottom: 20 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(26.0).strong());
                    ui.label(RichText::new(subtitle).size(13.5).color(palette.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, back, fore) = if self.is_running {
                        ("Running", palette.info_back, palette.info_text)
                    } else if !self.has_finished_run {
                        ("Ready", palette.hover, palette.muted)
                    } else if self.last_run_succeeded {
                        ("Completed", palette.success_back, palette.success_text)
                    } else {
                        ("Failed", palette.error_back, palette.error_text)
                    };
                    egui::Frame::new().fill(back).corner_radius(14).inner_margin(egui::Margin::symmetric(11, 5)).show(ui, |ui| {
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
                            None => "No asset workspace".to_string(),
                            Some(paths) => format!("Workspace: {}", paths.root.display()),
                        };
                        let hotel = match &self.hotel {
                            Some(hotel) => format!("{} ({})", hotel.name, hotel.language),
                            None => "Custom hotel URLs".into(),
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
                // Module tiles
                ui.columns(4, |columns| {
                    for (column, (category, label, caption, icon, color)) in columns.iter_mut().zip(MODULES) {
                        let color = theme::hex(color);
                        let response = egui::Frame::new()
                            .fill(palette.card)
                            .stroke(egui::Stroke::new(1.0_f32, palette.card_border))
                            .corner_radius(12)
                            .inner_margin(egui::Margin::symmetric(20, 18))
                            .show(column, |ui| {
                                ui.set_width(ui.available_width());
                                let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, 10.0, theme::with_alpha(color, 0x26));
                                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *icon, egui::FontId::proportional(19.0), color);
                                ui.add_space(12.0);
                                ui.label(RichText::new(catalog::for_category(*category).len().to_string()).size(28.0).strong());
                                ui.label(RichText::new(*label).size(14.0).strong());
                                ui.label(RichText::new(*caption).size(12.0).color(palette.muted));
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
                        card_heading(ui, &palette, "Quick actions", "Frequently used operations");
                        for id in ["habbo.all", "tools.swf-furniture-hab", "tools.generate-sql", "database.info"] {
                            let operation = catalog::get(id);
                            if operation_row(ui, &palette, operation, false, true, true).clicked() {
                                actions.push(Action::OpenOperation(operation));
                            }
                        }
                    });
                    card(&mut columns[1], &palette, egui::Margin::same(12), |ui| {
                        card_heading(ui, &palette, "Recent activity", "Runs in this session");
                        if self.recent_runs.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(18.0);
                                ui.label(RichText::new("🕓").size(26.0).color(palette.subtle));
                                ui.label(RichText::new("No operations run yet.").size(12.5).color(palette.muted));
                                ui.add_space(18.0);
                            });
                        }
                        for run in &self.recent_runs {
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                dot(ui, if run.succeeded { palette.success_text } else { palette.error_text });
                                ui.label(RichText::new(run.title).size(13.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(12.0);
                                    ui.label(RichText::new(run.finished_at.format("%H:%M").to_string()).size(12.0).color(palette.muted));
                                });
                            });
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
                                    ui.label(RichText::new(self.hotel.map_or("Custom hotel URLs", |h| h.name)).size(14.5).strong());
                                    if let Some(hotel) = &self.hotel {
                                        chip(ui, &hotel.language.to_uppercase(), palette.accent_soft, palette.accent_text);
                                    }
                                });
                                let text = match &self.hotel {
                                    Some(hotel) => format!("{}. Habbo Original downloads gamedata from this hotel.", hotel.region),
                                    None if self.hotel_known => "The gamedata URLs in config.ini point at no single Habbo hotel.".into(),
                                    None => "config.ini could not be read.".into(),
                                };
                                ui.label(RichText::new(text).size(12.5).color(palette.muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(secondary_button("Change")).clicked() {
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
                                ui.label(RichText::new("Asset workspace").size(14.5).strong());
                                let text = match &router.paths {
                                    None => "Not connected. Operations write to their own output folders next to the converter.".to_string(),
                                    Some(paths) => format!("Connected to {}. Compatible operations read and write there, with backups.", paths.root.display()),
                                };
                                ui.label(RichText::new(text).size(12.5).color(palette.muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(secondary_button(if router.paths.is_none() { "Connect folder" } else { "Manage" })).clicked() {
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
                    ui.label(RichText::new("Nitro assets folder").size(15.0).strong());
                    ui.label(RichText::new("Choose the nitro-assets root. Known folders are detected automatically.").size(12.5).color(palette.muted));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let buttons_width = 260.0;
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workspace.root_path)
                                .hint_text(r"E:\Users\you\Nitro-Files\nitro-assets")
                                .desired_width(ui.available_width() - buttons_width),
                        );
                        if ui.add(secondary_button("Browse…")).clicked() {
                            actions.push(Action::Browse);
                        }
                        if ui.add_enabled(self.workspace.busy.is_none(), primary_button("Save and verify")).clicked() {
                            actions.push(Action::SaveWorkspace);
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.workspace.status).size(14.0).strong().color(palette.accent_text));
                });
                ui.add_space(16.0);

                if self.workspace.folders.is_empty() {
                    card(ui, &palette, egui::Margin::symmetric(18, 14), |ui| {
                        ui.label(RichText::new("No workspace verified yet.").size(13.0).color(palette.muted));
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for folder in &self.workspace.folders {
                            ui.allocate_ui_with_layout(vec2(300.0, 80.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                card(ui, &palette, egui::Margin::same(16), |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(folder.name).size(14.5).strong());
                                        if folder.exists {
                                            chip(ui, &format!("{} FILES", habbo_core::console::n0(folder.file_count as i64)), palette.success_back, palette.success_text);
                                        } else {
                                            chip(ui, "MISSING", palette.warning_back, palette.warning_text);
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
                            ui.label(RichText::new("Safe file interaction").size(14.5).strong());
                            ui.label(
                                RichText::new("Compatible operations use these folders directly. Existing files are backed up under backups/<date-time> before replacement; paths outside this workspace are rejected.")
                                    .size(12.5)
                                    .color(palette.muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.workspace.paths.is_some(), secondary_button("Open workspace folder")).clicked() {
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
                            ui.label(RichText::new(format!("{} operations", operations.len())).size(12.0).color(palette.muted));
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
                                            ui.label(RichText::new(next.to_uppercase()).size(10.5).strong().color(palette.subtle));
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
            ui.label(RichText::new(operation.map_or("Select an operation", |o| o.title)).size(20.0).strong());
            ui.add_space(10.0);
            if operation.is_some_and(|o| o.is_destructive) {
                chip(ui, "CAUTION", palette.warning_back, palette.warning_text);
            }
            if operation.is_some_and(|o| o.requires_input) {
                chip(ui, "ASKS QUESTIONS", palette.info_back, palette.info_text);
            }
        });
        ui.label(RichText::new(operation.map_or("Choose an operation on the left to run it.", |o| o.description)).size(13.0).color(palette.muted));
        if let (Some(operation), Some(paths)) = (operation, &workspace_runtime::router().paths) {
            if WORKSPACE_OPERATION_IDS.contains(&operation.id) {
                ui.label(RichText::new(format!("Uses the asset workspace: {}", paths.root.display())).size(12.0).color(palette.accent_text));
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can_run = operation.is_some() && !self.is_running;
            if ui.add_enabled(can_run, primary_button("▶  Run operation")).clicked() {
                actions.push(Action::Run);
            }
            if self.has_finished_run && !self.is_running && ui.add(secondary_button("Done")).clicked() {
                actions.push(Action::Done);
            }
        });

        if self.is_running {
            ui.add_space(12.0);
            let time = ui.input(|i| i.time) as f32;
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 1.5, palette.hover);
            let width = rect.width() * 0.25;
            let x = rect.left() + ((time * 0.6).fract() * (rect.width() + width)) - width;
            let bar = egui::Rect::from_min_max(egui::pos2(x.max(rect.left()), rect.top()), egui::pos2((x + width).min(rect.right()), rect.bottom()));
            ui.painter().rect_filled(bar, 1.5, Fixed::brand());
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("OUTPUT").size(10.5).strong().color(palette.subtle));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Copy output").size(12.0)).corner_radius(8)).clicked() {
                    actions.push(Action::CopyLog);
                }
            });
        });
        ui.add_space(6.0);

        let needs_input = operation.is_some_and(|o| o.requires_input);
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
                    ui.label(RichText::new("Output appears here when the operation runs.").monospace().size(12.0).color(theme::hex("#6B7890")));
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("log")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(log_height - 22.0)
                    .show(ui, |ui| {
                        let mut text = self.log.text();
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::FontId::monospace(12.0))
                                .text_color(Fixed::console_text())
                                .frame(false)
                                .desired_width(f32::INFINITY),
                        );
                    });
            });

        if needs_input {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let edit = ui.add_enabled(
                    self.is_running,
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text("Answer the prompt and press Enter (empty = default)")
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
                if ui.add_enabled(self.is_running, secondary_button("Send")).clicked() {
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
            Dialog::Workspace(operation) => (
                operation.title.to_string(),
                format!("This operation will interact with the configured Nitro workspace:\n{workspace_root}"),
                "Existing files written by compatible operations are backed up automatically before replacement.",
                "Continue with workspace",
                "📁",
                palette.accent_text,
                palette.accent_soft,
            ),
            Dialog::Destructive(operation) => (
                "Database change".to_string(),
                format!("{} can modify your database.", operation.title),
                "Confirm only if your configuration and backups are ready.",
                "Confirm and run",
                "⚠",
                palette.warning_text,
                palette.warning_back,
            ),
            Dialog::LeaveSettings(_) => (
                "Unsaved changes".to_string(),
                "Your config.ini changes are not saved yet.".to_string(),
                "Discard them, or cancel and press Save changes.",
                "Discard changes",
                "⚠",
                palette.warning_text,
                palette.warning_back,
            ),
            Dialog::SwitchInterface => unreachable!(),
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
                        if ui.add(secondary_button("Cancel")).clicked() {
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
            ui.painter().rect_filled(rect, 8.0, Fixed::brand());
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "📦", egui::FontId::proportional(15.0), Color32::WHITE);
            ui.label(RichText::new("All-in-1 Converter").size(14.0).strong().color(palette.muted));
        });
        ui.add_space(22.0);
        ui.label(RichText::new("Choose your workspace").size(28.0).strong());
        ui.label(RichText::new("Both interfaces use the same converter functions and configuration.").size(14.0).color(palette.muted));
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
                                    chip(ui, "RECOMMENDED", palette.accent_soft, palette.accent_text);
                                });
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new(title).size(18.0).strong());
                        ui.label(RichText::new(description).size(13.0).color(palette.muted));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Open ›").size(13.0).strong().color(palette.accent_text));
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
            ui.label(RichText::new("You can switch later from the sidebar, or with 's' in the CLI menu.").size(12.0).color(palette.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.selector { "Exit" } else { "Cancel" };
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
        ui.label(RichText::new(title).size(15.0).strong());
        ui.label(RichText::new(subtitle).size(12.0).color(palette.muted));
    });
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
                    ui.label(RichText::new(operation.title).size(13.5).strong());
                    if operation.is_destructive {
                        chip(ui, "CAUTION", palette.warning_back, palette.warning_text);
                    }
                });
                let mut job = egui::text::LayoutJob::single_section(
                    operation.description.to_string(),
                    egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: palette.muted, ..Default::default() },
                );
                job.wrap = egui::text::TextWrapping { max_rows: 2, break_anywhere: false, overflow_character: Some('…'), max_width: ui.available_width() };
                ui.label(job);
            });
            if chevron {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("›").size(18.0).color(palette.subtle));
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
