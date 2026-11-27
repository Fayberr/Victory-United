//! Window shell: navigation and dispatch. The two halves keep their own state (`core`, `ops`) and only
//! see each other through `Shared`, which the core half fills in once per frame.
use crate::{capture::Capture, core, i18n::tr, ops, theme};
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Play,
    Settings,
    Saves,
    Flags,
    Monitor,
    Updates,
    Dev,
    Report,
    About,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::Play,
        Tab::Settings,
        Tab::Saves,
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
            Tab::Saves => tr("tab.saves"),
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
    capture: Option<Capture>,
    logo: egui::TextureHandle,
}

/// Command line switches: `--ui-demo` and `--capture <dir>` (the latter implies the former).
#[derive(Default, Clone)]
pub struct Args {
    pub demo: bool,
    pub capture: Option<std::path::PathBuf>,
    pub hw_dump: Option<std::path::PathBuf>,
}

impl Args {
    pub fn parse() -> Self {
        let mut args = Self::default();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--ui-demo" => args.demo = true,
                "--hw-dump" => args.hw_dump = it.next().map(Into::into),
                "--capture" => {
                    args.demo = true;
                    args.capture = it.next().map(Into::into);
                }
                _ => {}
            }
        }
        args
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args, icon: &egui::IconData) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (core, ops, shared) = if args.demo {
            (core::CoreState::demo(), ops::OpsState::demo(), core::demo_shared())
        } else {
            (core::CoreState::new(ctx.clone()), ops::OpsState::new(ctx), Default::default())
        };
        theme::set_appearance(&cc.egui_ctx, &core.settings.appearance);
        let capture = args.capture.map(Capture::new);
        let logo = cc.egui_ctx.load_texture(
            "victory-logo",
            egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba),
            egui::TextureOptions::LINEAR,
        );
        Self { tab: Tab::Play, core, ops, shared, capture, logo }
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").frame(theme::bar(ctx).inner_margin(egui::Margin::symmetric(18, 12))).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.spacing_mut().button_padding.x = 8.0;
                ui.horizontal(|ui| {
                    theme::mark(ui, &self.logo, 24.0);
                    ui.label(theme::strong(tr("app.name")).size(18.0));
                });
                ui.add_space(8.0);
                for (n, tab) in Tab::ALL.into_iter().enumerate() {
                    if n == 3 {
                        // Players use the first three; everything after is one click further for developers.
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                    }
                    let on = self.tab == tab;
                    let text = if on { theme::strong(tab.label()) } else { egui::RichText::new(tab.label()) };
                    let mut b = egui::Button::new(text).min_size(egui::vec2(0.0, 30.0));
                    b = if on {
                        b.fill(ui.visuals().widgets.active.bg_fill).stroke(egui::Stroke::new(1.0_f32, ui.visuals().widgets.hovered.bg_stroke.color))
                    } else {
                        b.fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)
                    };
                    if ui.add(b).clicked() {
                        self.tab = tab;
                    }
                }
            });
        });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").frame(theme::bar(ctx).inner_margin(egui::Margin::symmetric(18, 7))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let state = match self.shared.game_pid {
                    Some(pid) => format!("{} (pid {pid})", tr("status.running")),
                    None => tr("status.idle").to_string(),
                };
                ui.label(theme::dim(state).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let engine = match &self.shared.engine_exe {
                        Some(p) => p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        None => tr("status.no_engine").to_string(),
                    };
                    ui.label(theme::dim(format!("{} {}   |   {} {}", tr("status.engine"), engine, tr("status.launcher"), env!("CARGO_PKG_VERSION"))).size(12.5));
                });
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(cap) = &mut self.capture {
            if cap.drive(ctx, &mut self.tab, &mut self.core) {
                self.capture = None;
            }
        }
        self.core.tick(ctx, &mut self.shared);
        self.ops.tick(ctx, &self.shared, self.tab == Tab::Monitor);
        if self.core.screen() == core::Screen::FirstRun {
            egui::CentralPanel::default().show(ctx, |ui| core::firstrun::show(&mut self.core, &self.shared, &self.logo, ui));
            return;
        }
        self.header(ctx);
        self.status_bar(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill).inner_margin(egui::Margin::symmetric(28, 22))).show(ctx, |ui| {
            match self.tab {
                Tab::Play => core::play::show(&mut self.core, &self.shared, ui),
                Tab::Settings => core::settings::show(&mut self.core, &self.shared, ui),
                Tab::Saves => core::settings::show_saves(&mut self.core, ui),
                Tab::Flags => core::flags::show(&mut self.core, &self.shared, ui),
                Tab::Report => core::report::show(&mut self.core, &self.shared, ui),
                Tab::About => core::about::show(&mut self.core, &self.shared, &self.logo, ui),
                Tab::Monitor => ops::monitor::show(&mut self.ops, &self.shared, ui),
                Tab::Updates => ops::updates::show(&mut self.ops, &self.shared, ui),
                Tab::Dev => ops::dev::show(&mut self.ops, &self.shared, ui),
            }
        });
    }
}
