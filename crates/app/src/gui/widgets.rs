//! Small building blocks shared by the pages: cards, chips, headings and buttons.

use eframe::egui::{self, Color32, RichText};

use super::theme::{Fixed, Palette};

pub fn card<R>(ui: &mut egui::Ui, palette: &Palette, margin: egui::Margin, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.card_border))
        .corner_radius(12)
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn chip(ui: &mut egui::Ui, text: &str, back: Color32, fore: Color32) {
    egui::Frame::new().fill(back).corner_radius(6).inner_margin(egui::Margin::symmetric(7, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).size(10.5).strong().color(fore));
    });
}

pub fn heading(ui: &mut egui::Ui, palette: &Palette, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(15.0).strong());
        ui.label(RichText::new(subtitle).size(12.5).color(palette.muted));
    });
}

pub fn label_block(ui: &mut egui::Ui, palette: &Palette, label: &str, hint: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).size(13.0).strong());
        ui.label(RichText::new(hint).size(11.5).color(palette.muted));
    });
}

pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(Color32::WHITE))
        .fill(Fixed::brand())
        .corner_radius(8)
        .min_size(egui::vec2(0.0, 32.0))
}

pub fn secondary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string())).corner_radius(8).min_size(egui::vec2(0.0, 32.0))
}

/// A status dot.
pub fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}
