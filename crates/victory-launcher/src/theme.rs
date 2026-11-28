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

/// The VU mark without its app-icon tile: the glyph alone on a transparent background, so it sits on the
/// window like text does instead of showing up as a dark box. Two sizes (small for the top bar, large for
/// the welcome screen) so it is not shrunk too far, and a variant for each theme, because the letters are
/// white on the dark theme and near-black on the light one. See `branding/make-mark.py`.
pub struct Logo {
    /// `[dark theme, light theme]`.
    small: [egui::TextureHandle; 2],
    large: [egui::TextureHandle; 2],
}

const MARK_SMALL: &[u8] = include_bytes!("../../../branding/vu-mark-small.png");
const MARK_LARGE: &[u8] = include_bytes!("../../../branding/vu-mark-large.png");
/// Letters on the light theme.
const LIGHT_LETTERS: [u8; 3] = [22, 24, 29];

impl Logo {
    pub fn load(ctx: &egui::Context) -> Self {
        let make = |png: &[u8], name: &str| {
            let img = image::load_from_memory(png).expect("bundled mark is a valid PNG").into_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let dark = img.into_raw();
            let light = for_light_theme(&dark);
            let tex = |rgba: &[u8], suffix: &str| {
                ctx.load_texture(format!("vu-mark-{name}-{suffix}"), egui::ColorImage::from_rgba_unmultiplied(size, rgba), egui::TextureOptions::LINEAR)
            };
            [tex(&dark, "dark"), tex(&light, "light")]
        };
        Self { small: make(MARK_SMALL, "small"), large: make(MARK_LARGE, "large") }
    }
}

/// Turns the white letters dark and leaves the colored wings alone (straight, not premultiplied, RGBA).
fn for_light_theme(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        let (hi, lo) = (px[0].max(px[1]).max(px[2]), px[0].min(px[1]).min(px[2]));
        if px[3] == 0 || hi == 0 {
            continue;
        }
        let saturation = f32::from(hi - lo) / f32::from(hi);
        let letter = (1.0 - (saturation / 0.3).powi(2)).clamp(0.0, 1.0);
        for (c, dark) in px[..3].iter_mut().zip(LIGHT_LETTERS) {
            *c = (f32::from(*c) + (f32::from(dark) - f32::from(*c)) * letter).round() as u8;
        }
    }
    out
}

/// Draws the mark `height` points tall (about 2.6 times as wide).
pub fn mark(ui: &mut egui::Ui, logo: &Logo, height: f32) {
    let theme = usize::from(!ui.visuals().dark_mode);
    let wanted_pixels = height * ui.ctx().pixels_per_point();
    let tex = if wanted_pixels > 90.0 { &logo.large[theme] } else { &logo.small[theme] };
    let aspect = tex.size()[0] as f32 / tex.size()[1] as f32;
    ui.add(egui::Image::new((tex.id(), egui::vec2(height * aspect, height))));
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
    // A button that cannot be pressed yet keeps the shape but drops the loud fill.
    let (fill, ink, stroke) = if ui.is_enabled() {
        (v.strong_text_color(), v.panel_fill, Stroke::NONE)
    } else {
        (
            v.widgets.noninteractive.weak_bg_fill,
            v.weak_text_color(),
            v.widgets.noninteractive.bg_stroke,
        )
    };
    let b = egui::Button::new(egui::RichText::new(text).color(ink).strong().size(17.0))
        .fill(fill)
        .stroke(stroke)
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

    #[test]
    fn the_light_variant_darkens_the_letters_and_keeps_the_wings() {
        let img = image::load_from_memory(MARK_LARGE).unwrap().into_rgba8();
        let dark = img.into_raw();
        let light = for_light_theme(&dark);
        let (mut white, mut orange) = (0, 0);
        for (d, l) in dark.chunks_exact(4).zip(light.chunks_exact(4)) {
            if d[3] < 250 {
                continue;
            }
            if d[0] > 245 && d[1] > 245 && d[2] > 245 {
                white += 1;
                assert!(l[0] < 40 && l[1] < 40 && l[2] < 40, "a white pixel stayed light: {l:?}");
            }
            if d[0] > 200 && d[2] < 120 {
                orange += 1;
                assert_eq!(d[..3], l[..3], "a wing pixel changed");
            }
        }
        assert!(white > 1000 && orange > 500, "found {white} letter and {orange} wing pixels");
    }

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
