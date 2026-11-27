//! Neutral dark gray theme. There is no accent hue: layers are told apart by lightness, selection and
//! focus by a lighter fill and a thin outline. Muted amber and red exist for warnings and errors only.
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals};

/// The gray ramp, darkest to lightest.
pub const BG: Color32 = Color32::from_gray(20);
pub const PANEL: Color32 = Color32::from_gray(27);
pub const CARD: Color32 = Color32::from_gray(34);
pub const FIELD: Color32 = Color32::from_gray(16);
pub const HOVER: Color32 = Color32::from_gray(48);
pub const ACTIVE: Color32 = Color32::from_gray(60);
pub const LINE: Color32 = Color32::from_gray(52);
pub const LINE_STRONG: Color32 = Color32::from_gray(96);
pub const TEXT_DIM: Color32 = Color32::from_gray(132);
pub const TEXT: Color32 = Color32::from_gray(208);
pub const TEXT_STRONG: Color32 = Color32::from_gray(240);

/// Warnings and errors only, muted.
pub const WARN: Color32 = Color32::from_rgb(196, 160, 92);
pub const ERROR: Color32 = Color32::from_rgb(194, 108, 100);

pub const RADIUS: u8 = 6;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    let r = CornerRadius::same(RADIUS);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = Color32::from_gray(24);
    v.code_bg_color = FIELD;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(8);
    v.override_text_color = None;
    v.hyperlink_color = TEXT_STRONG;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.selection.bg_fill = Color32::from_gray(70);
    v.selection.stroke = Stroke::new(1.0, TEXT_STRONG);
    v.popup_shadow = egui::Shadow::NONE;
    v.window_shadow = egui::Shadow::NONE;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = Color32::from_gray(40);
    w.inactive.weak_bg_fill = Color32::from_gray(38);
    w.inactive.bg_stroke = Stroke::new(1.0, LINE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    w.hovered.corner_radius = r;
    w.active.bg_fill = ACTIVE;
    w.active.weak_bg_fill = ACTIVE;
    w.active.bg_stroke = Stroke::new(1.0, TEXT);
    w.active.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    w.active.corner_radius = r;
    w.open.bg_fill = ACTIVE;
    w.open.weak_bg_fill = ACTIVE;
    w.open.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    w.open.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    w.open.corner_radius = r;
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.window_margin = Margin::same(14);
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating = true;
        s.animation_time = 0.0; // no animation: draw on input only
        s.text_styles.insert(TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(14.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(14.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
    });
}

/// A raised group: one lightness step above the page, thin outline.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(16))
}

/// A quiet strip (header, status bar).
pub fn bar() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(18, 10))
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_DIM)
}

pub fn strong(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_STRONG).strong()
}

/// The one prominent button on a screen: light fill, dark text.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let b = egui::Button::new(egui::RichText::new(text).color(Color32::from_gray(18)).strong().size(17.0))
        .fill(Color32::from_gray(222))
        .stroke(Stroke::NONE)
        .min_size(egui::vec2(180.0, 46.0))
        .corner_radius(CornerRadius::same(8));
    ui.add(b)
}

/// Small outlined text label used for risk and status badges (text carries the meaning, not color).
pub fn badge(ui: &mut egui::Ui, text: &str, strong_look: bool) {
    let (fg, line) = if strong_look { (TEXT_STRONG, LINE_STRONG) } else { (TEXT_DIM, LINE) };
    egui::Frame::new()
        .stroke(Stroke::new(1.0, line))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.5).color(fg));
        });
}

pub fn warn_text(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(WARN)
}

pub fn error_text(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(ERROR)
}
