//! Neutral dark gray theme, no color accent. Owned by the "core" worker (placeholder until then).
use eframe::egui;

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
}
