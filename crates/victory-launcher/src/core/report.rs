use crate::{i18n::tr, theme};
use eframe::egui;

pub fn show(_core: &mut super::CoreState, _shared: &crate::app::Shared, ui: &mut egui::Ui) {
    ui.label(theme::strong(tr("stub.core")).size(24.0));
}
