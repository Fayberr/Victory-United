use crate::{i18n::tr, theme};
use eframe::egui;

pub fn show(_core: &mut super::CoreState, _shared: &crate::app::Shared, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(70.0);
        ui.label(theme::strong(tr("firstrun.title")).size(26.0));
        ui.add_space(6.0);
        ui.label(theme::dim(tr("firstrun.sub")));
    });
}
