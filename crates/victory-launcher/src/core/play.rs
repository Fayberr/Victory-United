use crate::{i18n::tr, theme};
use eframe::egui;

pub fn show(_core: &mut super::CoreState, _shared: &crate::app::Shared, ui: &mut egui::Ui) {
    ui.label(theme::strong(tr("play.title")).size(24.0));
    ui.add_space(14.0);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let _ = theme::primary_button(ui, tr("play.play"));
    });
}
