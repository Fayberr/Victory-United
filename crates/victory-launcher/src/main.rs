//! Victory Launcher: settings, presets, flags, monitor, updates and bug report for Victory United.
//!
//! Hard rule: this program sits next to an emulator that is CPU bound, so it must use almost no CPU.
//! No repaint loops, no polling threads, no animation. See docs/launcher-contract.md ("Idle CPU rules").
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod core;
mod i18n;
mod ops;
mod paths;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Victory Launcher")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([780.0, 520.0])
            .with_icon(icon.clone()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Victory Launcher",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, args, &icon)))
        }),
    )
}
