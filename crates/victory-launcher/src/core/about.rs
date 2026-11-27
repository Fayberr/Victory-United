use eframe::egui;

pub fn show(_core: &mut super::CoreState, _shared: &crate::app::Shared, ui: &mut egui::Ui) {
    ui.label(crate::i18n::tr("stub.core"));
}
