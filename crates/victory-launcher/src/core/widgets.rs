//! Small UI helpers shared by the core screens, and the state they keep between frames.
use crate::theme;
use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct UiState {
    /// `--ui-demo`: no disk writes, no process starts.
    pub demo: bool,
    /// "Start without my flags" for the next start.
    pub start_without_flags: bool,
    pub confirm_stop: bool,
    pub settings: super::settings::SettingsUi,
    pub flags: super::flags::FlagsUi,
    pub report: super::report::ReportUi,
    pub agent: crate::agent::ui::View,
}

/// Opens a folder in Explorer (or the file manager elsewhere).
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Opens a web address in the default browser.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn pick_folder() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().pick_folder()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `filter` is (label, extensions).
pub fn pick_file(filter: (&str, &[&str])) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().add_filter(filter.0, filter.1).pick_file()
    }
    #[cfg(not(windows))]
    {
        let _ = filter;
        None
    }
}

pub fn save_file(name: &str, filter: (&str, &[&str])) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().set_file_name(name).add_filter(filter.0, filter.1).save_file()
    }
    #[cfg(not(windows))]
    {
        let _ = (name, filter);
        None
    }
}

pub fn page_title(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(theme::strong(title).size(24.0));
    if !sub.is_empty() {
        ui.add_space(2.0);
        ui.label(theme::dim(sub));
    }
    ui.add_space(14.0);
}

/// Label on the left in dim text, value on the right.
pub fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.add_sized([96.0, 18.0], egui::Label::new(theme::dim(key)));
        // Wraps inside the card instead of pushing it past the window edge (long graphics card names).
        ui.add(egui::Label::new(value).wrap());
    });
}

/// A calm notice box with muted warning or error text.
pub fn notice(ui: &mut egui::Ui, text: &str, error: bool) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0_f32, ui.visuals().widgets.hovered.bg_stroke.color))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(if error { theme::error_text(ui, text) } else { theme::warn_text(ui, text) });
        });
}

/// A scroll area that obeys the capture mode's requested offset.
pub fn page_scroll(ui: &mut egui::Ui, core_scroll: Option<f32>, add: impl FnOnce(&mut egui::Ui)) {
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if let Some(y) = core_scroll {
        area = area.vertical_scroll_offset(y);
    }
    area.show(ui, add);
}
