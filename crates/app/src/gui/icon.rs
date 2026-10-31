//! The app icon (assets/icon.png, drawn by scripts/make-icon.py; the exe carries the same
//! picture as resources/app.ico): the window icon and the logo in the sidebar and on About.

use eframe::egui;

const PNG: &[u8] = include_bytes!("../../assets/icon.png");

/// The icon as RGBA pixels for the window.
pub fn window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(PNG).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData { rgba: image.into_raw(), width, height })
}

/// Paints the icon into `rect`; the texture is made once and kept in egui's memory.
pub fn paint(painter: &egui::Painter, rect: egui::Rect) {
    let ctx = painter.ctx();
    let id = egui::Id::new("app-icon");
    let texture = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let image = image::load_from_memory(PNG).map(|i| i.into_rgba8()).unwrap_or_default();
        let size = [image.width() as usize, image.height() as usize];
        let texture = ctx.load_texture("app-icon", egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()), egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
        texture
    });
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}
