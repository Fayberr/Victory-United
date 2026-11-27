use eframe::egui;

pub fn show(_ops: &mut super::OpsState, _shared: &crate::app::Shared, ui: &mut egui::Ui) {
    ui.label(crate::i18n::tr("stub.ops"));
}
