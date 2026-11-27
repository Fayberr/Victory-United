//! Window shell: navigation and dispatch. The two halves keep their own state (`core`, `ops`) and only
//! see each other through `Shared`, which the core half fills in once per frame.
use crate::{core, i18n::tr, ops};
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Play,
    Settings,
    Flags,
    Monitor,
    Updates,
    Dev,
    Report,
    About,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Play,
        Tab::Settings,
        Tab::Flags,
        Tab::Monitor,
        Tab::Updates,
        Tab::Dev,
        Tab::Report,
        Tab::About,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Play => tr("tab.play"),
            Tab::Settings => tr("tab.settings"),
            Tab::Flags => tr("tab.flags"),
            Tab::Monitor => tr("tab.monitor"),
            Tab::Updates => tr("tab.updates"),
            Tab::Dev => tr("tab.dev"),
            Tab::Report => tr("tab.report"),
            Tab::About => tr("tab.about"),
        }
    }
}

/// Facts the two halves share. Written by the core half, read by the ops half.
#[derive(Default, Clone)]
pub struct Shared {
    /// Process id of the engine while the game runs.
    pub game_pid: Option<u32>,
    /// Final environment (`VICTORY_*`) of the current or last launch, sorted by name.
    pub launch_env: Vec<(String, String)>,
    /// The engine executable in use (installed build or a developer's own).
    pub engine_exe: Option<std::path::PathBuf>,
    /// The log file of the current or last run.
    pub log_file: Option<std::path::PathBuf>,
}

pub struct App {
    pub tab: Tab,
    pub core: core::CoreState,
    pub ops: ops::OpsState,
    pub shared: Shared,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self { tab: Tab::Play, core: Default::default(), ops: Default::default(), shared: Default::default() }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("nav").exact_width(168.0).show(ctx, |ui| {
            for tab in Tab::ALL {
                if ui.selectable_label(self.tab == tab, tab.label()).clicked() {
                    self.tab = tab;
                }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Play => core::play::show(&mut self.core, &self.shared, ui),
            Tab::Settings => core::settings::show(&mut self.core, &self.shared, ui),
            Tab::Flags => core::flags::show(&mut self.core, &self.shared, ui),
            Tab::Report => core::report::show(&mut self.core, &self.shared, ui),
            Tab::About => core::about::show(&mut self.core, &self.shared, ui),
            Tab::Monitor => ops::monitor::show(&mut self.ops, &self.shared, ui),
            Tab::Updates => ops::updates::show(&mut self.ops, &self.shared, ui),
            Tab::Dev => ops::dev::show(&mut self.ops, &self.shared, ui),
        });
    }
}
