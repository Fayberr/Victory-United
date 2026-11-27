use super::CoreState;
use crate::{app::Shared, theme};
use eframe::egui;

pub fn show(_core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    ui.label(theme::strong("About").size(24.0));
}
