use super::CoreState;
use crate::{app::Shared, theme};
use eframe::egui;

#[derive(Default)]
pub struct SettingsUi {}

pub fn show(_core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    ui.label(theme::strong("Settings").size(24.0));
}
