use super::{
    widgets::{kv, open_folder, open_url, page_title},
    CoreState,
};
use crate::{app::Shared, i18n::tr, paths, theme};
use eframe::egui;

const REPO: &str = "https://github.com/Fayberr/Victory-United";

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    page_title(ui, tr("about.title"), tr("about.sub"));
    ui.set_max_width(820.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        kv(ui, tr("about.version"), &format!("{} ({})", env!("CARGO_PKG_VERSION"), if cfg!(debug_assertions) { "debug" } else { "release" }));
        let engine = core.engine_exe().and_then(|p| p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| tr("status.no_engine").to_string());
        kv(ui, tr("about.engine"), &engine);
        kv(ui, tr("about.licence"), "GPL-3.0-or-later");
        ui.add_space(8.0);
        ui.label(theme::dim(tr("about.text")).size(13.0));
    });
    ui.add_space(12.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("about.links")).size(16.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button(tr("about.project")).clicked() {
                open_url(REPO);
            }
            if ui.button(tr("about.issues")).clicked() {
                open_url(&format!("{REPO}/issues"));
            }
        });
        ui.label(theme::dim(REPO).size(12.5));
    });
    ui.add_space(12.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("about.folders")).size(16.0));
        ui.add_space(4.0);
        let cache = core.plan().map(|p| p.cache_dir).unwrap_or_else(paths::cache_root);
        for (label, path) in [
            (tr("about.data"), paths::data_dir()),
            (tr("about.logs"), paths::logs_dir()),
            (tr("about.cache"), cache),
            (tr("about.engine_folder"), paths::engine_root()),
        ] {
            ui.horizontal(|ui| {
                if ui.add_sized([150.0, 28.0], egui::Button::new(label)).clicked() {
                    open_folder(&path);
                }
                ui.label(theme::dim(path.to_string_lossy()).size(12.5));
            });
        }
    });
}
