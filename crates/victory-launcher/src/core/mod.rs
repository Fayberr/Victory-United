//! "Core" half of the launcher: play, settings, presets, flags, profiles, bug report. Owned by the core worker.
pub mod about;
pub mod firstrun;
pub mod flagreg;
pub mod flags;
pub mod game;
pub mod hw;
pub mod launch;
pub mod play;
pub mod presets;
pub mod report;
pub mod runner;
pub mod settings;
pub mod store;
#[cfg(test)]
pub mod testutil;
pub mod widgets;

use crate::{app::Shared, paths};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    time::{Duration, Instant},
};

/// Which full page the window shows. `Main` is the tabbed shell; `WarmUp` only exists for screenshots
/// (the real warm-up screen is part of the Play tab, driven by the runner state).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Main,
    FirstRun,
    WarmUp,
}

pub struct CoreState {
    /// Set by the capture mode to show a particular page regardless of the real state.
    pub force_screen: Option<Screen>,
    /// Vertical scroll offset requested by the capture mode.
    pub page_scroll: Option<f32>,
    /// False in demo mode: nothing is written to disk and nothing is started.
    pub persist: bool,
    pub ctx: egui::Context,
    pub settings: store::Settings,
    pub game: Option<game::Game>,
    pub profile: launch::Profile,
    pub profiles: Vec<launch::Profile>,
    pub extra: Vec<launch::ExtraFlag>,
    pub hw: Option<hw::Hardware>,
    hw_rx: Option<Receiver<hw::Hardware>>,
    pub detected: Vec<game::Game>,
    pub detecting: bool,
    detect_rx: Option<Receiver<Vec<game::Game>>>,
    pub runner: runner::Runner,
    /// A message for the user after something failed (shown on the Play tab).
    pub notice: Option<String>,
    /// Settings tab, Flags tab and bug report keep their own widgets state here.
    pub ui: widgets::UiState,
    shown_demo: Option<Screen>,
    pub last_env: Vec<(String, String)>,
    pub last_engine: Option<PathBuf>,
}

impl CoreState {
    pub fn new(ctx: egui::Context) -> Self {
        let settings = store::Settings::load();
        let game = settings.game_folder.as_deref().and_then(game::from_pick).or_else(|| settings.game_exe.as_deref().and_then(game::from_pick));
        let profiles = launch::list_profiles(&paths::profiles_dir());
        let profile = profiles.iter().find(|p| p.name == settings.active_profile).cloned().unwrap_or_default();
        let extra = std::fs::read_to_string(paths::launch_flags_file()).map(|t| launch::parse_extra(&t)).unwrap_or_default();
        let mut s = Self::blank(ctx, settings, game, profile, profiles, extra);
        s.start_hardware_probe();
        if s.game.is_none() {
            s.start_detection();
        }
        s
    }

    fn blank(ctx: egui::Context, settings: store::Settings, game: Option<game::Game>, profile: launch::Profile, profiles: Vec<launch::Profile>, extra: Vec<launch::ExtraFlag>) -> Self {
        Self {
            force_screen: None,
            page_scroll: None,
            persist: true,
            runner: runner::Runner::new(ctx.clone()),
            ctx,
            settings,
            game,
            profile,
            profiles,
            extra,
            hw: None,
            hw_rx: None,
            detected: Vec::new(),
            detecting: false,
            detect_rx: None,
            notice: None,
            ui: Default::default(),
            shown_demo: None,
            last_env: Vec::new(),
            last_engine: None,
        }
    }

    /// Synthetic state for `--ui-demo` and screenshots.
    pub fn demo() -> Self {
        let ctx = egui::Context::default();
        let game = game::Game { folder: "D:/XboxGames/Demo Game/Content".into(), exe: "D:/XboxGames/Demo Game/Content/game.exe".into(), title: "Demo Game".into() };
        let settings = store::Settings { game_folder: Some(game.folder.clone()), game_exe: Some(game.exe.clone()), setup_done: true, ..Default::default() };
        let mut profile = launch::Profile::default();
        profile.edits.insert("VICTORY_THIN_THREADS".into(), "8".into());
        profile.edits.insert("VICTORY_TEXTURE_ROOM_PERCENT".into(), "80".into());
        let profiles = vec![launch::Profile { name: "Default".into(), ..Default::default() }, launch::Profile { name: "Low memory".into(), ..Default::default() }];
        let extra = vec![
            launch::ExtraFlag { name: "VICTORY_LAND_ECHO".into(), value: Some("0".into()), enabled: true },
            launch::ExtraFlag { name: "VICTORY_SYNC_FINE".into(), value: None, enabled: false },
        ];
        let mut s = Self::blank(ctx, settings, Some(game.clone()), profile, profiles, extra);
        s.persist = false;
        s.hw = Some(hw::Hardware::demo());
        s.detected = vec![game];
        s.ui.demo = true;
        s
    }

    pub fn screen(&self) -> Screen {
        if let Some(s) = self.force_screen {
            return if s == Screen::WarmUp { Screen::Main } else { s };
        }
        if !self.settings.setup_done && self.game.is_none() { Screen::FirstRun } else { Screen::Main }
    }

    fn start_hardware_probe(&mut self) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        self.hw_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(hw::probe());
            ctx.request_repaint();
        });
    }

    pub fn start_detection(&mut self) {
        if self.detecting {
            return;
        }
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        self.detect_rx = Some(rx);
        self.detecting = true;
        std::thread::spawn(move || {
            let _ = tx.send(game::auto_detect());
            ctx.request_repaint();
        });
    }

    /// Once per frame: results of background work, dropped files, runner events, and `Shared`.
    pub fn tick(&mut self, ctx: &egui::Context, shared: &mut Shared) {
        if self.ui.demo {
            self.apply_demo_screen();
        } else {
            self.runner.drain();
            if let Some(rx) = &self.hw_rx {
                if let Ok(hw) = rx.try_recv() {
                    self.hw = Some(hw);
                    self.hw_rx = None;
                }
            }
            if let Some(rx) = &self.detect_rx {
                if let Ok(found) = rx.try_recv() {
                    self.detected = found;
                    self.detecting = false;
                    self.detect_rx = None;
                }
            }
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            if let Some(g) = dropped.iter().find_map(|p| game::from_pick(p)) {
                self.set_game(g);
            } else if !dropped.is_empty() {
                self.notice = Some("That is not a game folder: no MicrosoftGame.Config was found there.".into());
            }
            shared.game_pid = self.runner.running_pid();
            if let Some(log) = self.runner.log_file() {
                shared.log_file = Some(log);
            }
        }
        shared.launch_env = self.last_env.clone();
        shared.engine_exe = self.engine_exe();
    }

    /// Demo and capture: show the state each screenshot name stands for.
    fn apply_demo_screen(&mut self) {
        use runner::RunState;
        let want = self.force_screen.unwrap_or(Screen::Main);
        if self.shown_demo == Some(want) {
            return;
        }
        self.shown_demo = Some(want);
        let log: PathBuf = "logs/run-20260101-120000.log".into();
        self.runner.state = match want {
            Screen::WarmUp => RunState::Warming { done: 38_000_000, total: 120_000_000, since: Instant::now() - Duration::from_secs(95) },
            _ => RunState::Running { pid: 4242, since: Instant::now() - Duration::from_secs(312), log },
        };
        self.last_env = self.resolved().0.sorted();
    }

    pub fn engine_exe(&self) -> Option<PathBuf> {
        if self.ui.demo {
            return Some("engine/a1b2c3d4-release/victory.exe".into());
        }
        self.settings.engine_override.clone().filter(|p| p.is_file()).or_else(paths::active_engine_exe)
    }

    pub fn set_game(&mut self, g: game::Game) {
        self.settings.game_folder = Some(g.folder.clone());
        self.settings.game_exe = Some(g.exe.clone());
        self.game = Some(g);
        self.notice = None;
        self.save_settings();
    }

    pub fn save_settings(&mut self) {
        if self.persist {
            if let Err(e) = self.settings.save() {
                self.notice = Some(format!("Could not save settings: {e}"));
            }
        }
    }

    pub fn gpu(&self) -> (String, u32) {
        self.hw.as_ref().map(|h| (h.gpu_name.clone(), h.vendor_id)).unwrap_or_default()
    }

    /// The preset in use and the resolved environment.
    pub fn resolved(&self) -> (launch::Resolved, String) {
        let file = presets::builtin();
        let (gpu, vendor) = self.gpu();
        let id = launch::preset_id(file, &self.profile, &gpu, vendor).to_string();
        (launch::resolve(file, &id, &self.profile, &self.active_extra()), id)
    }

    pub fn active_extra(&self) -> Vec<launch::ExtraFlag> {
        if self.ui.start_without_flags { Vec::new() } else { self.extra.clone() }
    }

    pub fn plan(&self) -> Option<launch::Plan> {
        let game = self.game.as_ref()?;
        let engine = self.engine_exe()?;
        let (resolved, _) = self.resolved();
        let dirs = self.settings.dirs.clone();
        Some(launch::Plan::new(engine, &game.folder, game.exe.clone(), self.profile.options.console, &resolved, &dirs, &paths::cache_root(), &paths::data_dir()))
    }

    /// The Play button.
    pub fn play(&mut self) {
        self.notice = None;
        let Some(plan) = self.plan() else {
            self.notice = Some(if self.game.is_none() { "Pick the game folder first.".into() } else { "No engine is installed yet.".into() });
            return;
        };
        let cold = runner::shader_entries(&plan.cache_dir, runner::COLD_CACHE_ENTRIES) < runner::COLD_CACHE_ENTRIES;
        self.last_env = plan.env.iter().filter(|(k, _)| !launch::PATH_VARS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        self.runner.start(plan, paths::logs_dir(), cold, !self.active_extra().is_empty());
    }

    /// Starts again after a quick exit, without the extra flags.
    pub fn play_without_flags(&mut self) {
        self.ui.start_without_flags = true;
        self.play();
    }

    pub fn save_extra(&mut self) {
        if self.persist {
            let _ = store::write_atomic(&paths::launch_flags_file(), launch::format_extra(&self.extra).as_bytes());
        }
    }

    pub fn save_profile(&mut self) {
        if self.persist {
            let _ = launch::save_profile(&paths::profiles_dir(), &self.profile);
            self.settings.active_profile = self.profile.name.clone();
            self.save_settings();
            self.profiles = launch::list_profiles(&paths::profiles_dir());
        }
    }
}

/// `Shared` as it looks while the game runs, for demo and screenshots.
pub fn demo_shared() -> Shared {
    Shared {
        game_pid: Some(4242),
        launch_env: Vec::new(),
        engine_exe: Some("engine/a1b2c3d4-release/victory.exe".into()),
        log_file: Some("logs/run-20260101-120000.log".into()),
    }
}
