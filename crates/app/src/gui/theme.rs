//! Colours of the Professional window: fixed brand/sidebar/console colours and the
//! light/dark tokens (`Pro.*` in the C# theme).

use eframe::egui::{self, Color32};

pub fn hex(value: &str) -> Color32 {
    let v = value.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).unwrap_or(0);
    match v.len() {
        8 => Color32::from_rgba_unmultiplied(byte(2), byte(4), byte(6), byte(0)),
        _ => Color32::from_rgb(byte(0), byte(2), byte(4)),
    }
}

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub struct Fixed;

impl Fixed {
    pub fn brand() -> Color32 { hex("#4F5FE6") }
    pub fn sidebar() -> Color32 { hex("#0E1424") }
    pub fn sidebar_hover() -> Color32 { hex("#1A2338") }
    pub fn sidebar_selected() -> Color32 { hex("#232F52") }
    pub fn sidebar_text() -> Color32 { hex("#C9D2E3") }
    pub fn sidebar_muted() -> Color32 { hex("#7283A6") }
    pub fn sidebar_accent() -> Color32 { hex("#8B9BFF") }
    pub fn console_background() -> Color32 { hex("#0B101C") }
    pub fn console_text() -> Color32 { hex("#D3DAE6") }
    pub fn console_border() -> Color32 { hex("#1F293D") }
}

/// The light/dark tokens.
#[derive(Clone, Copy)]
pub struct Palette {
    pub surface: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub card_border_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub subtle: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub warning_back: Color32,
    pub warning_text: Color32,
    pub success_back: Color32,
    pub success_text: Color32,
    pub error_back: Color32,
    pub error_text: Color32,
    pub info_back: Color32,
    pub info_text: Color32,
}

impl Palette {
    pub fn for_dark(dark: bool) -> Self {
        let p = |light: &str, dark_value: &str| hex(if dark { dark_value } else { light });
        Self {
            surface: p("#F4F6FA", "#0D121C"),
            card: p("#FFFFFF", "#151B28"),
            card_border: p("#E2E7EF", "#232C3E"),
            card_border_strong: p("#C9D1DE", "#34405A"),
            text: p("#0F172A", "#E7EBF3"),
            muted: p("#5E6B80", "#97A3B8"),
            subtle: p("#8C97AA", "#66738C"),
            hover: p("#EFF2F7", "#1C2434"),
            selected: p("#ECEFFF", "#1E2550"),
            accent_text: p("#4352D4", "#A9B4FF"),
            accent_soft: p("#ECEFFF", "#1E2550"),
            warning_back: p("#FEF3C7", "#3A2C0C"),
            warning_text: p("#8A4B0B", "#F5C451"),
            success_back: p("#DCFCE7", "#11301F"),
            success_text: p("#15803D", "#5BD68A"),
            error_back: p("#FEE2E2", "#3A1717"),
            error_text: p("#B42318", "#F78A80"),
            info_back: p("#E6F0FF", "#152340"),
            info_text: p("#1D4ED8", "#8AB4FF"),
        }
    }

    pub fn current(ctx: &egui::Context) -> Self {
        Self::for_dark(ctx.style().visuals.dark_mode)
    }
}

/// Text, widget and selection colours for the current light/dark mode.
pub fn apply(ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    let palette = Palette::for_dark(dark);
    ctx.style_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = palette.surface;
        visuals.window_fill = palette.card;
        visuals.extreme_bg_color = if dark { hex("#0F1522") } else { hex("#FFFFFF") };
        visuals.override_text_color = Some(palette.text);
        visuals.selection.bg_fill = with_alpha(Fixed::brand(), 90);
        visuals.hyperlink_color = palette.accent_text;
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, palette.card_border_strong);
        visuals.widgets.inactive.weak_bg_fill = palette.card;
        visuals.widgets.hovered.weak_bg_fill = palette.hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, palette.card_border_strong);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}
