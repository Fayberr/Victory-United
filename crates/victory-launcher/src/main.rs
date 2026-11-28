//! Victory United Launcher: settings, presets, flags, monitor, updates and bug report for Victory United.
//!
//! Hard rule: this program sits next to an emulator that is CPU bound, so it must use almost no CPU.
//! No repaint loops, no polling threads, no animation. See docs/launcher-contract.md ("Idle CPU rules").
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent;
mod app;
mod capture;
mod core;
mod i18n;
mod ops;
mod paths;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    if let Some(code) = agent::run_from_args(std::env::args().skip(1)) {
        std::process::exit(code);
    }
    let args = app::Args::parse();
    if let Some(file) = &args.hw_dump {
        // Verification aid: write the hardware check to a file and exit, no window.
        let hw = core::hw::probe();
        let _ = std::fs::write(file, format!("{}{:?}\n{:?}\n", hw.summary(), hw.note, core::hw::warnings(&hw)));
        return Ok(());
    }
    let logo = image::load_from_memory(include_bytes!("../../../branding/vu-icon.png"))
        .expect("bundled logo is a valid PNG")
        .into_rgba8();
    let (width, height) = logo.dimensions();
    let icon = std::sync::Arc::new(egui::IconData { rgba: logo.into_raw(), width, height });
    let settings = if args.demo { Default::default() } else { core::store::Settings::load() };
    let mut viewport = egui::ViewportBuilder::default()
            .with_title("Victory United Launcher")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([780.0, 520.0])
            .with_icon(icon.clone());
    if settings.behavior.remember_window
        && let Some(saved) = settings.window.filter(|saved| saved.valid()) {
            viewport = viewport.with_inner_size(saved.size).with_position(saved.position);
        }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Victory United Launcher",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, args, settings)))
        }),
    )
}
