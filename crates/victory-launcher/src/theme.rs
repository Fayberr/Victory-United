//! Neutral gray themes. Layers, selection and focus use lightness and outlines.
use crate::core::store::Appearance;
use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals,
};

pub fn apply(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.visuals = visuals(theme);
            s.spacing.item_spacing = egui::vec2(10.0, 8.0);
            s.spacing.button_padding = egui::vec2(14.0, 6.0);
            s.spacing.interact_size.y = 28.0;
            s.spacing.window_margin = Margin::same(14);
            s.spacing.scroll.bar_width = 8.0;
            s.spacing.scroll.floating = true;
            s.animation_time = 0.0; // no animation: draw on input only
            s.text_styles.insert(
                TextStyle::Heading,
                FontId::new(22.0, FontFamily::Proportional),
            );
            s.text_styles
                .insert(TextStyle::Body, FontId::new(14.5, FontFamily::Proportional));
            s.text_styles.insert(
                TextStyle::Button,
                FontId::new(14.5, FontFamily::Proportional),
            );
            s.text_styles.insert(
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            );
            s.text_styles.insert(
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            );
        });
    }
}

pub fn set_appearance(ctx: &egui::Context, appearance: &Appearance) {
    use crate::core::store::ThemeChoice;
    ctx.set_theme(match appearance.theme {
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::System => egui::ThemePreference::System,
    });
    ctx.set_zoom_factor(f32::from(appearance.scale_percent.clamp(80, 150)) / 100.0);
}

fn visuals(theme: egui::Theme) -> Visuals {
    let dark = theme == egui::Theme::Dark;
    let gray = |d, l| Color32::from_gray(if dark { d } else { l });
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let panel = gray(27, 245);
    let line = gray(52, 198);
    let outline = gray(113, 105);
    let text = gray(208, 39);
    let strong = gray(240, 20);
    let active = gray(60, 201);
    let field = gray(16, 252);
    v.panel_fill = gray(20, 234);
    v.window_fill = panel;
    v.extreme_bg_color = field;
    v.faint_bg_color = gray(24, 239);
    v.code_bg_color = field;
    v.window_stroke = Stroke::new(1.0_f32, line);
    v.window_corner_radius = CornerRadius::same(8);
    v.weak_text_color = Some(gray(186, 70));
    v.hyperlink_color = strong;
    v.warn_fg_color = if dark {
        Color32::from_rgb(215, 178, 107)
    } else {
        Color32::from_rgb(112, 71, 19)
    };
    v.error_fg_color = if dark {
        Color32::from_rgb(231, 145, 138)
    } else {
        Color32::from_rgb(143, 47, 44)
    };
    v.selection.bg_fill = gray(70, 188);
    v.selection.stroke = Stroke::new(1.0_f32, strong);
    v.popup_shadow = egui::Shadow::NONE;
    v.window_shadow = egui::Shadow::NONE;

    let w = &mut v.widgets;
    for (style, fill, border, foreground) in [
        (&mut w.noninteractive, panel, line, text),
        (&mut w.inactive, gray(40, 223), line, text),
        (&mut w.hovered, gray(48, 212), outline, strong),
        (&mut w.active, active, outline, strong),
        (&mut w.open, active, outline, strong),
    ] {
        style.bg_fill = fill;
        style.weak_bg_fill = fill;
        style.bg_stroke = Stroke::new(1.0_f32, border);
        style.fg_stroke = Stroke::new(1.0_f32, foreground);
        style.corner_radius = CornerRadius::same(6);
    }
    v
}

pub struct Palette {
    pub chart: [Color32; 6],
    pub target: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub badge: Stroke,
    pub badge_selected: Stroke,
}

pub fn palette(v: &Visuals) -> Palette {
    let (chart, target, good, warn, bad) = if v.dark_mode {
        (
            [
                Color32::from_rgb(96, 165, 250),  // sky blue
                Color32::from_rgb(245, 166, 91),  // warm amber-orange
                Color32::from_rgb(76, 195, 138),  // teal-emerald
                Color32::from_rgb(232, 106, 125), // coral-rose
                Color32::from_rgb(167, 139, 250), // soft violet
                Color32::from_rgb(56, 189, 248),  // cyan
            ],
            Color32::from_rgb(110, 200, 155),
            Color32::from_rgb(76, 195, 138),
            Color32::from_rgb(215, 178, 107),
            Color32::from_rgb(231, 145, 138),
        )
    } else {
        (
            [
                Color32::from_rgb(37, 99, 235),   // cobalt blue
                Color32::from_rgb(217, 119, 6),   // burnt amber-orange
                Color32::from_rgb(5, 150, 105),   // emerald green
                Color32::from_rgb(220, 38, 38),   // crimson rose
                Color32::from_rgb(124, 58, 237),  // violet
                Color32::from_rgb(2, 132, 199),   // deep cyan
            ],
            Color32::from_rgb(16, 145, 100),
            Color32::from_rgb(5, 150, 105),
            Color32::from_rgb(180, 110, 20),
            Color32::from_rgb(190, 50, 45),
        )
    };
    Palette {
        chart,
        target,
        good,
        warn,
        bad,
        badge: Stroke::new(1.0_f32, v.weak_text_color()),
        badge_selected: Stroke::new(1.0_f32, v.strong_text_color()),
    }
}

fn card_fill(v: &Visuals) -> Color32 {
    Color32::from_gray(if v.dark_mode { 34 } else { 248 })
}

pub fn card(ui: &egui::Ui) -> egui::Frame {
    let v = ui.visuals();
    egui::Frame::new()
        .fill(card_fill(v))
        .stroke(v.window_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(16))
}

pub fn mark(ui: &mut egui::Ui, logo: &egui::TextureHandle, size: f32) {
    ui.image((logo.id(), egui::Vec2::splat(size)));
}

pub fn bar(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(ctx.style().visuals.window_fill)
        .inner_margin(Margin::symmetric(18, 10))
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).weak()
}

pub fn strong(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).strong()
}

pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let v = ui.visuals();
    let b = egui::Button::new(
        egui::RichText::new(text)
            .color(v.panel_fill)
            .strong()
            .size(17.0),
    )
    .fill(v.strong_text_color())
    .stroke(Stroke::NONE)
    .min_size(egui::vec2(180.0, 46.0))
    .corner_radius(CornerRadius::same(8));
    ui.add(b)
}

pub fn badge(ui: &mut egui::Ui, text: &str, strong_look: bool) {
    let colors = palette(ui.visuals());
    let stroke = if strong_look {
        colors.badge_selected
    } else {
        colors.badge
    };
    egui::Frame::new()
        .stroke(stroke)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.5).color(stroke.color));
        });
}

pub fn warn_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(ui.visuals().warn_fg_color)
}

pub fn error_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(ui.visuals().error_fg_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f64 {
        let linear = |s: u8| {
            let s = f64::from(s) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn both_themes_meet_text_contrast() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let v = visuals(theme);
            for bg in [
                v.panel_fill,
                v.window_fill,
                card_fill(&v),
                v.extreme_bg_color,
                v.selection.bg_fill,
            ] {
                for fg in [v.text_color(), v.weak_text_color()] {
                    assert!(contrast(fg, bg) >= 4.5, "{theme:?}: {fg:?} on {bg:?}");
                }
                assert!(contrast(v.strong_text_color(), bg) >= 3.0);
            }
            assert!(contrast(v.panel_fill, v.strong_text_color()) >= 4.5);
        }
    }

    #[test]
    fn system_preference_follows_platform_input() {
        let ctx = egui::Context::default();
        apply(&ctx);
        set_appearance(&ctx, &Appearance::default());
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let _ = ctx.run(
                egui::RawInput {
                    system_theme: Some(theme),
                    ..Default::default()
                },
                |ctx| {
                    assert_eq!(ctx.theme(), theme);
                    assert_eq!(ctx.style().visuals.panel_fill, visuals(theme).panel_fill);
                },
            );
        }
    }
}
