//! The small always-on-top, click-through fps overlay: a second egui viewport with no decorations,
//! transparent, ignoring the mouse. It is drawn from `OpsState::tick`, so it only redraws when the
//! main window gets its once-a-second update. Exclusive fullscreen games hide it (the window sits
//! under the game's own swap chain); borderless and windowed games show it.
use super::{monitor::Summary, settings::Corner, OpsSettings};
use crate::i18n::tr;
use eframe::egui::{self, Color32, ViewportBuilder, ViewportId};

const SIZE: egui::Vec2 = egui::vec2(132.0, 52.0);
const MARGIN: f32 = 16.0;

/// Top-left position of the overlay in a monitor of `monitor` points.
pub fn position(corner: Corner, monitor: egui::Vec2) -> egui::Pos2 {
    let x = match corner {
        Corner::TopLeft | Corner::BottomLeft => MARGIN,
        _ => monitor.x - SIZE.x - MARGIN,
    };
    let y = match corner {
        Corner::TopLeft | Corner::TopRight => MARGIN,
        _ => monitor.y - SIZE.y - MARGIN,
    };
    egui::pos2(x.max(0.0), y.max(0.0))
}

pub fn show(ctx: &egui::Context, cfg: &OpsSettings, summary: Option<&Summary>, running: bool) {
    let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
    // One viewport id per corner: changing the corner makes a new window at the new place.
    let id = ViewportId::from_hash_of(("victory-overlay", cfg.overlay_corner));
    let builder = ViewportBuilder::default()
        .with_title("Victory overlay")
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_inner_size(SIZE)
        .with_position(position(cfg.overlay_corner, monitor));
    let (line1, line2) = match (summary, running) {
        (Some(s), _) => {
            let fps = s.fps_counter.unwrap_or(s.fps_avg);
            (format!("{fps:.0} fps"), format!("{:.1} ms", 1000.0 / fps.max(0.1)))
        }
        (None, true) => (tr("overlay.waiting").to_string(), String::new()),
        (None, false) => (tr("overlay.idle").to_string(), String::new()),
    };
    ctx.show_viewport_immediate(id, builder, |ctx, _class| {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_black_alpha(150)).corner_radius(6).inner_margin(egui::Margin::symmetric(10, 6))).show(ctx, |ui| {
            ui.label(egui::RichText::new(line1.as_str()).size(20.0).color(Color32::from_gray(235)).strong());
            ui.label(egui::RichText::new(line2.as_str()).size(13.0).color(Color32::from_gray(170)));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corners_stay_on_the_monitor() {
        let m = egui::vec2(1920.0, 1080.0);
        assert_eq!(position(Corner::TopLeft, m), egui::pos2(16.0, 16.0));
        assert_eq!(position(Corner::BottomRight, m), egui::pos2(1920.0 - 132.0 - 16.0, 1080.0 - 52.0 - 16.0));
        assert_eq!(position(Corner::TopRight, m).y, 16.0);
        assert_eq!(position(Corner::BottomLeft, egui::vec2(10.0, 10.0)), egui::pos2(16.0, 0.0));
    }
}
