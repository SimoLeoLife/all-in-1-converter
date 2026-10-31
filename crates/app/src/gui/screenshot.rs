//! Debug aid: `HABBO_SCREENSHOT=<file.png>` saves one screenshot of the window and closes it.
//! `HABBO_SCREENSHOT_PAGE` picks the page (dashboard, workspace, settings, habbo, nitro, tools,
//! database, about, selector, confirm); `HABBO_SCREENSHOT_THEME=dark|light` forces the theme and
//! `HABBO_SCREENSHOT_LOOK=hotel|professional` the look.

use eframe::egui;

use super::{Page, ProApp};
use crate::catalog::{self, OperationCategory};

pub fn tick(app: &mut ProApp, ctx: &egui::Context) {
    let Ok(path) = std::env::var("HABBO_SCREENSHOT") else { return };
    let frame = ctx.cumulative_pass_nr();

    if frame == 0 {
        if let Ok(lang) = std::env::var("HABBO_SCREENSHOT_LANG") {
            super::i18n::set(&lang);
            app.status_text = super::i18n::t("Ready").into();
        }
        if let Ok(look) = std::env::var("HABBO_SCREENSHOT_LOOK") {
            super::theme::set_hotel(!look.eq_ignore_ascii_case("professional"));
            super::theme::install_fonts(ctx);
        }
        match std::env::var("HABBO_SCREENSHOT_THEME").as_deref() {
            Ok("dark") => ctx.set_theme(egui::Theme::Dark),
            Ok("light") => ctx.set_theme(egui::Theme::Light),
            _ => {}
        }
        let page = std::env::var("HABBO_SCREENSHOT_PAGE").unwrap_or_default();
        app.selector = page == "selector";
        let category = |c| Page::Category(c);
        app.page = match page.as_str() {
            "workspace" => Page::Workspace,
            "settings" => Page::Settings,
            "habbo" => category(OperationCategory::HabboOriginal),
            "nitro" => category(OperationCategory::NitroCustom),
            "tools" | "confirm" => category(OperationCategory::HotelTools),
            "database" => category(OperationCategory::Database),
            "about" => category(OperationCategory::General),
            _ => Page::Dashboard,
        };
        if let Page::Category(c) = app.page {
            app.selected = catalog::for_category(c).first().copied();
        }
        if page == "tools" {
            app.selected = catalog::find("tools.swf-furniture-hab");
            app.log.append("🔄 Converting 40 SWF files...\n✅ Extracted: chair.json\n");
            app.log.set_color(Some(habbo_core::Color::Red));
            app.log.append("❌ Failed to convert broken_chair.swf\n");
            app.log.set_color(Some(habbo_core::Color::Green));
            app.log.append("📦 Generated chair.hab -> SWFCompiler/furniture/hab\n");
            app.log.set_color(None);
            app.log.append("⚡ [   10/40] (25.0%) |  10.8 items/sec | Converted: 10\n");
            // Shown as running (no real task behind it).
            app.is_running = true;
            for id in ["tools.swf-clothes-hab", "tools.generate-sql"] {
                app.queue.push_back(super::QueuedRun { operation: catalog::get(id), answers: vec![] });
            }
        }
        if let Ok(bundle) = std::env::var("HABBO_SCREENSHOT_BUNDLE") {
            match super::preview::BundlePreview::open(ctx, std::path::Path::new(&bundle)) {
                Ok(preview) => app.preview = Some(preview),
                Err(error) => eprintln!("preview: {error:#}"),
            }
        }
        if page == "drop" {
            app.dialog = Some(super::Dialog::DroppedSwf(vec!["a.swf".into(), "b.swf".into()]));
        }
        if page == "confirm" {
            app.dialog = Some(super::Dialog::Destructive(catalog::get("database.optimize"), super::After::Run));
        }
    }

    if frame == 8 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    let image = ctx.input(|i| {
        i.raw.events.iter().find_map(|event| match event {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(image) = image {
        let [w, h] = image.size;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let _ = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else {
        ctx.request_repaint();
    }
}
