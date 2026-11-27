//! The desktop overlay is a separate click-through viewport. Its preview uses the same content.
use super::{
    OpsSettings, OpsState,
    monitor::{Row, Summary, rate_picker},
    settings::{Corner, Field, OverlayLayout, OverlayMode, OverlaySettings, TextTone},
};
use crate::{i18n::tr, theme};
use eframe::egui::{self, Color32, ViewportBuilder, ViewportId};
use std::collections::VecDeque;
use victory_stats::PresentState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Fps,
    Full,
}

impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Fps,
            Self::Fps => Self::Full,
            Self::Full => Self::Off,
        }
    }
}

pub fn visible(cfg: &OpsSettings, mode: Mode, running: bool, focused: bool) -> bool {
    mode != Mode::Off
        && cfg.overlay_options.mode == OverlayMode::Window
        && (running || !cfg.overlay_options.only_game)
        && (!focused || !cfg.overlay_options.hide_focused)
}

/// Top-left of the popup inside the monitor rectangle.
pub fn position(
    corner: Corner,
    monitor: egui::Rect,
    window: egui::Vec2,
    margin: f32,
) -> egui::Pos2 {
    let within = |span: f32, size: f32| (span - size).max(0.0);
    let x = within(monitor.width(), window.x);
    let y = within(monitor.height(), window.y);
    let margin_x = margin.clamp(0.0, x);
    let margin_y = margin.clamp(0.0, y);
    let horizontal = match corner {
        Corner::TopLeft | Corner::BottomLeft => margin_x,
        _ => (x - margin).max(0.0),
    };
    let vertical = match corner {
        Corner::TopLeft | Corner::TopRight => margin_y,
        _ => (y - margin).max(0.0),
    };
    monitor.min + egui::vec2(horizontal, vertical)
}

pub struct Content {
    lines: Vec<String>,
    spark: Vec<f32>,
    layout: OverlayLayout,
}

impl Content {
    pub fn new(
        cfg: &OpsSettings,
        summary: Option<&Summary>,
        history: &VecDeque<Row>,
        running: bool,
        mode: Mode,
    ) -> Self {
        let o = &cfg.overlay_options;
        let layout = if mode == Mode::Fps {
            OverlayLayout::Fps
        } else {
            o.layout
        };
        let mut content = Self {
            lines: Vec::new(),
            spark: Vec::new(),
            layout,
        };
        let status = match (running, summary) {
            (false, _) => Some(tr("overlay.idle")),
            (_, None) => Some(tr("overlay.waiting")),
            (_, Some(s)) => match s.state {
                Some(PresentState::Occluded) => Some(tr("monitor.occluded")),
                Some(PresentState::Waiting) => Some(tr("overlay.waiting")),
                Some(PresentState::Other(_)) => Some(tr("monitor.rejected")),
                _ => None,
            },
        };
        if let Some(status) = status {
            content.lines.push(status.to_owned());
            return content;
        }
        let Some(summary) = summary else {
            return content;
        };
        if layout == OverlayLayout::Fps {
            let value = summary
                .value(Field::Fps, cfg.monitor.target_ms, 0)
                .unwrap_or_else(|| tr("monitor.unavailable").into());
            content.lines.push(format!("{value} FPS"));
            return content;
        }
        for &field in o.fields.iter().filter(|f| Field::OVERLAY.contains(f)) {
            let value = if field == Field::FrameMs {
                summary
                    .row
                    .and_then(|r| r.frame_ms)
                    .map(|ms| format!("{ms:.1} ms"))
            } else {
                summary.value(field, cfg.monitor.target_ms, 0)
            };
            content.lines.push(format!(
                "{}  {}",
                field.label(),
                value.as_deref().unwrap_or(tr("monitor.unavailable"))
            ));
        }
        if o.sparkline {
            content.spark = history
                .iter()
                .rev()
                .take(64)
                .filter_map(|r| r.frame_ms)
                .collect();
            content.spark.reverse();
        }
        if content.lines.is_empty() && content.spark.is_empty() {
            content.lines.push(tr("overlay.no_values").to_owned());
        }
        content
    }

    fn lines(&self) -> Vec<String> {
        if self.layout == OverlayLayout::Line {
            vec![self.lines.join("   ·   ")]
        } else {
            self.lines.clone()
        }
    }
}

pub fn show(ctx: &egui::Context, cfg: &OpsSettings, content: &Content) {
    let o = &cfg.overlay_options;
    let scale = o.scale_percent.clamp(60, 200) as f32 / 100.0;
    let font = egui::FontId::proportional(14.0 * scale);
    let lines = content.lines();
    let dimensions = ctx.fonts_mut(|fonts| {
        lines
            .iter()
            .map(|s| {
                fonts
                    .layout_no_wrap(s.clone(), font.clone(), Color32::PLACEHOLDER)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
    });
    let monitor = ctx
        .input(|i| i.viewport().monitor_size)
        .unwrap_or(egui::vec2(1920.0, 1080.0));
    let width = (dimensions + 24.0)
        .max(if content.spark.is_empty() {
            92.0
        } else {
            144.0
        })
        .min(monitor.x);
    let height = (lines.len().max(1) as f32 * (18.0 * scale + 3.0)
        + 15.0
        + if content.spark.is_empty() {
            0.0
        } else {
            24.0 * scale
        })
    .min(monitor.y);
    let size = egui::vec2(width, height);
    let id = ViewportId::from_hash_of(("victory-overlay", cfg.overlay_corner));
    let builder = ViewportBuilder::default()
        .with_title(tr("overlay.window_title"))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_inner_size(size)
        .with_position(position(
            cfg.overlay_corner,
            egui::Rect::from_min_size(egui::Pos2::ZERO, monitor),
            size,
            o.margin.min(160) as f32,
        ));
    ctx.show_viewport_immediate(id, builder, |ctx, _class| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                render(ui, o, &lines, &content.spark, true);
            });
    });
}

fn render(
    ui: &mut egui::Ui,
    o: &OverlaySettings,
    lines: &[String],
    spark: &[f32],
    full_width: bool,
) {
    let dark = o.tone == TextTone::Light;
    let palette = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let opacity = (o.opacity_percent.min(100) * 255 / 100) as u8;
    let bg = Color32::from_rgba_unmultiplied(
        palette.panel_fill.r(),
        palette.panel_fill.g(),
        palette.panel_fill.b(),
        opacity,
    );
    let text = palette.strong_text_color();
    let scale = o.scale_percent.clamp(60, 200) as f32 / 100.0;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            if full_width {
                ui.set_width(ui.available_width());
            }
            for line in lines {
                ui.add(
                    egui::Label::new(egui::RichText::new(line).size(14.0 * scale).color(text))
                        .truncate(),
                );
            }
            if spark.len() > 1 {
                let low = spark.iter().copied().fold(f32::INFINITY, f32::min);
                let span = (spark.iter().copied().fold(0.0, f32::max) - low).max(1.0);
                let width = ui.available_width().clamp(110.0, 460.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 22.0 * scale), egui::Sense::hover());
                let points = spark
                    .iter()
                    .enumerate()
                    .map(|(i, &ms)| {
                        let x = rect.left() + rect.width() * i as f32 / (spark.len() - 1) as f32;
                        let y = rect.bottom() - 1.0 - (rect.height() - 2.0) * (ms - low) / span;
                        egui::pos2(x, y)
                    })
                    .collect::<Vec<_>>();
                ui.painter()
                    .add(egui::Shape::line(points, egui::Stroke::new(1.25, text)));
            }
        });
}

pub fn settings(ui: &mut egui::Ui, ops: &mut OpsState) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("overlay.title")).size(16.0));
            if ui.button(tr("monitor.reset")).clicked() {
                ops.cfg.overlay = false;
                ops.cfg.overlay_corner = Corner::default();
                ops.cfg.overlay_options = OverlaySettings::default();
                ops.hotkey_draft.clear();
                ops.overlay_mode = None;
            }
        });
        if ui
            .checkbox(&mut ops.cfg.overlay, tr("overlay.enable"))
            .on_hover_text(tr("overlay.enable_help"))
            .changed()
        {
            ops.overlay_mode = None;
        }
        let o = &mut ops.cfg.overlay_options;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut o.mode, OverlayMode::Window, tr("overlay.desktop"));
            ui.selectable_value(&mut o.mode, OverlayMode::Engine, tr("overlay.engine"));
        });
        ui.label(theme::dim(tr("overlay.cost_help")).size(12.5));
        if o.mode == OverlayMode::Engine {
            ui.label(theme::dim(tr("overlay.engine_help")).size(12.5));
            return;
        }
        ui.label(theme::dim(tr("overlay.desktop_help")).size(12.5));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("overlay.layout"))
                .on_hover_text(tr("overlay.layout_help"));
            for (value, label) in [
                (OverlayLayout::Line, "overlay.line"),
                (OverlayLayout::Vertical, "overlay.vertical"),
                (OverlayLayout::Fps, "overlay.fps"),
            ] {
                ui.selectable_value(&mut o.layout, value, tr(label));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("overlay.corner"))
                .on_hover_text(tr("overlay.corner_help"));
            egui::ComboBox::from_id_salt("overlay-corner")
                .selected_text(ops.cfg.overlay_corner.label())
                .show_ui(ui, |ui| {
                    for c in Corner::ALL {
                        ui.selectable_value(&mut ops.cfg.overlay_corner, c, c.label());
                    }
                });
            ui.label(tr("overlay.margin"))
                .on_hover_text(tr("overlay.margin_help"));
            ui.add(egui::DragValue::new(&mut o.margin).range(0..=160));
        });
        ui.horizontal_wrapped(|ui| {
            for (label, value, range, help) in [
                (
                    "overlay.scale",
                    &mut o.scale_percent,
                    60..=200,
                    "overlay.scale_help",
                ),
                (
                    "overlay.opacity",
                    &mut o.opacity_percent,
                    0..=100,
                    "overlay.opacity_help",
                ),
            ] {
                ui.label(tr(label)).on_hover_text(tr(help));
                ui.add(egui::DragValue::new(value).range(range).suffix("%"))
                    .on_hover_text(tr(help));
            }
            ui.selectable_value(&mut o.tone, TextTone::Light, tr("overlay.light_text"))
                .on_hover_text(tr("overlay.text_help"));
            ui.selectable_value(&mut o.tone, TextTone::Dark, tr("overlay.dark_text"))
                .on_hover_text(tr("overlay.text_help"));
        });
        rate_picker(ui, "overlay-rate", &mut o.rate);
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut o.sparkline, tr("overlay.spark"))
                .on_hover_text(tr("overlay.spark_help"));
            ui.checkbox(&mut o.only_game, tr("overlay.only_game"))
                .on_hover_text(tr("overlay.only_game_help"));
            ui.checkbox(&mut o.hide_focused, tr("overlay.hide_focus"))
                .on_hover_text(tr("overlay.hide_focus_help"));
        });
        ui.separator();
        ui.label(theme::strong(tr("overlay.values")))
            .on_hover_text(tr("overlay.values_help"));
        ui.horizontal_wrapped(|ui| {
            for &field in Field::OVERLAY {
                let mut enabled = o.fields.contains(&field);
                if ui.checkbox(&mut enabled, field.label()).changed() {
                    if enabled {
                        o.fields.push(field);
                    } else {
                        o.fields.retain(|f| *f != field);
                    }
                }
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("overlay.hotkey"))
                .on_hover_text(tr("overlay.hotkey_help"));
            ui.add(
                egui::TextEdit::singleline(&mut ops.hotkey_draft)
                    .desired_width(142.0)
                    .hint_text("Ctrl+Alt+F10"),
            )
            .on_hover_text(tr("overlay.hotkey_help"));
            if ui.button(tr("overlay.apply")).clicked() {
                match super::hotkey::parse(&ops.hotkey_draft) {
                    Ok(_) => {
                        o.hotkey = ops.hotkey_draft.trim().to_owned();
                        ops.hotkey_error = None;
                    }
                    Err(error) => ops.hotkey_error = Some(error.to_string()),
                }
            }
        });
        if let Some(error) = &ops.hotkey_error {
            ui.label(theme::warn_text(ui, error));
        }
        ui.separator();
        ui.label(theme::dim(tr("overlay.preview")));
        let content = Content::new(
            &ops.cfg,
            ops.monitor.summary.as_ref(),
            &ops.monitor.history,
            ops.monitor.summary.is_some(),
            Mode::Full,
        );
        render(
            ui,
            &ops.cfg.overlay_options,
            &content.lines(),
            &content.spark,
            false,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_respect_margin_and_the_monitor_origin() {
        let rect = egui::Rect::from_min_size(egui::pos2(-1000.0, 0.0), egui::vec2(1000.0, 600.0));
        let window = egui::vec2(120.0, 40.0);
        assert_eq!(
            position(Corner::TopLeft, rect, window, 16.0),
            egui::pos2(-984.0, 16.0)
        );
        assert_eq!(
            position(Corner::BottomRight, rect, window, 16.0),
            egui::pos2(-136.0, 544.0)
        );
        assert_eq!(
            position(Corner::BottomRight, rect, egui::vec2(1000.0, 700.0), 16.0),
            rect.min
        );
    }

    #[test]
    fn visibility_and_cycle_obey_settings() {
        let mut cfg = OpsSettings::default();
        assert!(!visible(&cfg, Mode::Fps, false, false));
        assert!(visible(&cfg, Mode::Fps, true, false));
        cfg.overlay_options.hide_focused = true;
        assert!(!visible(&cfg, Mode::Full, true, true));
        cfg.overlay_options.mode = OverlayMode::Engine;
        assert!(!visible(&cfg, Mode::Full, true, false));
        assert_eq!(Mode::Off.next().next().next(), Mode::Off);
    }

    #[test]
    fn occluded_overlay_uses_the_hidden_message() {
        let mut summary = Summary::default();
        summary.state = Some(PresentState::Occluded);
        let content = Content::new(
            &OpsSettings::default(),
            Some(&summary),
            &VecDeque::new(),
            true,
            Mode::Fps,
        );
        assert_eq!(
            content.lines,
            ["Game window is hidden, the game is not drawing"]
        );
    }
}
