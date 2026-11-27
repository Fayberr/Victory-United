use super::CoreState;
use crate::{app::Shared, theme};
use eframe::egui;

#[derive(Default)]
pub struct ReportUi {}

pub fn show(_core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    ui.label(theme::strong("Bug report").size(24.0));
}
