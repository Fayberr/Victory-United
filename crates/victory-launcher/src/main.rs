//! Victory Launcher: settings, presets, flags, monitor, updates and bug report for Victory United.
//!
//! Hard rule: this program sits next to an emulator that is CPU bound, so it must use almost no CPU.
//! No repaint loops, no polling threads, no animation. See docs/launcher-contract.md ("Idle CPU rules").
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod core;
mod i18n;
mod ops;
mod paths;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Victory Launcher")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([780.0, 520.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Victory Launcher",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc)))
        }),
    )
}
