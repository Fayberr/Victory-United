//! "Core" half of the launcher: play, settings, presets, flags, profiles, bug report. Owned by the core worker.
pub mod about;
pub mod actions;
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
pub mod saves;
pub mod settings;
pub mod store;
pub mod storage;
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
    Unsupported,
    Idle,
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
    pub rendering_adapter: Option<u64>,
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
    demo_game: Option<game::Game>,
    pub last_env: Vec<(String, String)>,

}

impl CoreState {
    pub fn new(ctx: egui::Context, settings: store::Settings) -> Self {
        paths::configure_storage(&settings);
        let game = actions::chosen_game(&settings);
        let profiles = launch::list_profiles(&paths::profiles_dir());
        let profile = actions::active_profile(&actions::Dirs::real(), &settings);
        let extra = launch::read_extra(&paths::launch_flags_file());
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
            rendering_adapter: None,
            hw_rx: None,
            detected: Vec::new(),
            detecting: false,
            detect_rx: None,
            notice: None,
            ui: Default::default(),
            shown_demo: None,
            demo_game: None,
            last_env: Vec::new(),

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
        s.demo_game = Some(game.clone());
        s.detected = vec![game];
        s.ui.demo = true;
        s
    }

    pub fn screen(&self) -> Screen {
        if let Some(s) = self.force_screen {
            return if s == Screen::FirstRun { s } else { Screen::Main };
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
            shared.game_pid = self.runner.running_pid();
        } else {
            self.runner.drain();
            settings::tick_storage(self);
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
        shared.launch_env = if self.ui.demo { self.last_env.clone() } else { self.runner.env.clone() };
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
        let mut hardware = hw::Hardware::demo();
        if want == Screen::Unsupported {
            hardware.gpu_name = "NVIDIA GeForce GTX 1060".into();
            hardware.mesh_shaders = Some(false);
            hardware.vram_mb = 6144;
        }
        self.hw = Some(hardware);
        // The first-run screenshot shows the walk-through before anything is chosen.
        self.game = if want == Screen::FirstRun { None } else { self.demo_game.clone() };
        let log: PathBuf = "logs/run-20260101-120000.log".into();
        self.runner.state = match want {
            Screen::Unsupported | Screen::Idle => RunState::Idle,
            Screen::WarmUp => RunState::Warming { done: 38_000_000, total: 120_000_000, since: Instant::now() - Duration::from_secs(95) },
            _ => RunState::Running { pid: 4242, since: Instant::now() - Duration::from_secs(312), log },
        };
        self.last_env = self.resolved().0.sorted();
    }

    pub fn engine_exe(&self) -> Option<PathBuf> {
        if self.ui.demo {
            return (self.force_screen != Some(Screen::FirstRun)).then(|| "engine/a1b2c3d4-release/victory.exe".into());
        }
        actions::Dirs::real().engine_exe(&self.settings)
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

    #[cfg(test)]
    pub fn is_changed_for_test(&self, name: &str) -> bool {
        self.profile.edits.contains_key(name) || self.profile.removed.iter().any(|r| r == name)
    }

    pub fn gpu(&self) -> (String, u32) {
        self.hw.as_ref().map(|h| (h.gpu_name.clone(), h.vendor_id)).unwrap_or_default()
    }

    /// The preset in use and the resolved environment.
    pub fn resolved(&self) -> (launch::Resolved, String) {
        actions::resolve(&self.profile, self.hw.as_ref(), &self.active_extra())
    }

    pub fn active_extra(&self) -> Vec<launch::ExtraFlag> {
        if self.ui.start_without_flags { Vec::new() } else { self.extra.clone() }
    }

    pub fn plan(&self) -> Option<launch::Plan> {
        let extra = self.active_extra();
        self.launch_input(&extra).plan(&paths::cache_root(), &paths::game_data_root())
    }

    fn launch_input<'a>(&'a self, extra: &'a [launch::ExtraFlag]) -> actions::LaunchInput<'a> {
        actions::LaunchInput {
            settings: &self.settings,
            profile: &self.profile,
            extra,
            hardware: self.hw.as_ref(),
            game: self.game.as_ref(),
            engine: self.engine_exe(),
        }
    }

    /// The Play button.
    pub fn play(&mut self, acknowledged: bool) {
        if !self.persist {
            return;
        }
        self.notice = None;
        if self.runner.busy() || settings::storage_busy(self) {
            return;
        }
        let extra = self.active_extra();
        let ops = crate::ops::OpsSettings::load();
        let mut runner = self.runner.handle();
        let started = actions::start_game(&mut runner, &actions::Dirs::real(), &self.launch_input(&extra), &ops, acknowledged);
        self.runner.drain();
        match started {
            Ok(storage) => {
                self.settings.storage = storage;
                paths::configure_storage(&self.settings);
            }
            Err(error) => self.notice = Some(format!("{error:#}")),
        }
    }

    /// Starts again after a quick exit, without the extra flags.
    pub fn play_without_flags(&mut self, acknowledged: bool) {
        self.ui.start_without_flags = true;
        self.play(acknowledged);
    }

    pub fn save_extra(&mut self) {
        if self.persist {
            let _ = launch::write_extra(&paths::launch_flags_file(), &self.extra);
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

    /// "New" and "Duplicate": a profile under `name`, empty or with the current values.
    pub fn add_profile(&mut self, name: &str, copy: bool) {
        if !copy {
            self.profile = launch::Profile::default();
        }
        self.profile.name = name.to_string();
        self.ui.settings.new_name.clear();
        self.save_profile();
    }

    pub fn rename_profile(&mut self, name: &str) {
        let old = std::mem::replace(&mut self.profile.name, name.to_string());
        self.ui.settings.new_name.clear();
        if self.persist {
            let _ = launch::save_renamed(&paths::profiles_dir(), &self.profile, &old);
            self.save_profile();
        }
    }

    pub fn delete_active_profile(&mut self) {
        if self.persist {
            match launch::remove_profile(&paths::profiles_dir(), &self.profile.name, &self.profile.name) {
                Ok(next) => self.profile = next,
                Err(error) => {
                    self.notice = Some(error.to_string());
                    return;
                }
            }
            self.profiles = launch::list_profiles(&paths::profiles_dir());
        } else {
            self.profile = self.profiles.first().cloned().unwrap_or_default();
        }
        self.settings.active_profile = self.profile.name.clone();
        self.save_settings();
    }

    /// Reads everything again after something else changed the files (an agent, for example).
    pub fn reload_from_disk(&mut self) {
        let dirs = actions::Dirs::real();
        self.settings = dirs.settings();
        paths::configure_storage(&self.settings);
        self.game = actions::chosen_game(&self.settings).or_else(|| self.game.take().filter(|_| self.settings.game_folder.is_none()));
        self.profiles = launch::list_profiles(&paths::profiles_dir());
        self.profile = actions::active_profile(&dirs, &self.settings);
        self.extra = launch::read_extra(&paths::launch_flags_file());
        self.ui.report.built = false;
        settings::reload(self);
        crate::theme::set_appearance(&self.ctx, &self.settings.appearance);
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
