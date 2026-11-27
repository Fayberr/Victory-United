//! "Core" half of the launcher: play, settings, presets, flags, profiles, bug report. Owned by the core worker.
pub mod about;
pub mod engine;
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

use crate::{app::Shared, i18n::trf, paths};
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
        let game = settings.game_folder.as_deref().and_then(game::from_pick).or_else(|| settings.game_exe.as_deref().and_then(game::from_pick));
        let profiles = launch::list_profiles(&paths::profiles_dir());
        let profile = profiles.iter().find(|p| p.name == settings.active_profile).cloned().unwrap_or_default();
        let extra = std::fs::read_to_string(paths::launch_flags_file()).map(|t| launch::parse_extra(&t)).unwrap_or_default();
        let mut s = Self::blank(ctx, settings, game, profile, profiles, extra);
        s.drop_unusable_engine();
        s.start_hardware_probe();
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
        let game = game::Game { folder: "D:/Games/Demo Game/Content".into(), exe: "D:/Games/Demo Game/Content/game.exe".into(), title: "Demo Game".into() };
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
            return Some("engine/a1b2c3d4-release/victory.exe".into());
        }
        self.own_engine().or_else(paths::active_engine_exe)
    }

    /// The developer's own engine from Settings, when it is set and still a usable `victory.exe`.
    pub fn own_engine(&self) -> Option<PathBuf> {
        self.settings.engine_override.clone().filter(|p| engine::check(p).is_ok())
    }

    /// An earlier version let any file be chosen as the engine (even the launcher itself, which made Play open
    /// another launcher window). Forget a stored choice that is not a real engine, so the built-in one is used.
    fn drop_unusable_engine(&mut self) {
        if let Some(bad) = self.settings.engine_override.clone().filter(|p| engine::check(p).is_err()) {
            self.settings.engine_override = None;
            self.save_settings();
            self.notice = Some(trf("engine.dropped", &[("path", &bad.to_string_lossy())]));
        }
    }

    /// Takes a different engine from the user, or says why it cannot be used.
    pub fn use_engine(&mut self, path: PathBuf) -> Result<(), engine::Problem> {
        engine::check(&path)?;
        self.settings.engine_override = Some(path);
        self.save_settings();
        Ok(())
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
        Some(launch::Plan::new(engine, &game.folder, game.exe.clone(), self.profile.options.console, &resolved, &dirs, &paths::cache_root(), &paths::game_data_root()))
    }

    /// The Play button.
    pub fn play(&mut self, acknowledged: bool) {
        if !self.persist {
            return;
        }
        self.notice = None;
        if let Err(error) = play::hardware_allows_play(self.hw.as_ref(), acknowledged) {
            self.notice = Some(error.to_string());
            return;
        }
        if self.runner.busy() || settings::storage_busy(self) {
            return;
        }
        let settings_file = paths::settings_file();
        let engine = paths::engine_root();
        let game = self.game.as_ref().map(|g| g.folder.as_path());
        let ctx = storage::MoveContext { settings: &settings_file, engine: &engine, game };
        let guarded = storage::lock(&paths::activity_lock()).and_then(|guard| {
            let current = store::Settings::load_from(&settings_file);
            storage::validate_data_work(&ctx, &current)?;
            saves::recover_restore(&storage::Location::Data.path(&paths::data_dir(), &current))?;
            Ok((guard, current))
        });
        let (guard, current) = match guarded {
            Ok(result) => result,
            Err(error) => {
                self.notice = Some(format!("{error:#}"));
                return;
            }
        };
        self.settings.storage = current.storage;
        paths::configure_storage(&self.settings);
        let Some(mut plan) = self.plan() else {
            self.notice = Some(if self.game.is_none() { "Pick the game folder first.".into() } else { "No engine is installed yet.".into() });
            return;
        };
        let ops = crate::ops::OpsSettings::load();
        match crate::ops::settings::engine_options(&ops, &paths::data_dir()) {
            Ok(flags) => {
                let resolved = self.resolved().0;
                for (key, value) in flags {
                    let override_allowed = !matches!(resolved.source.get(&key), Some(launch::Source::Edit | launch::Source::Extra))
                        && !self.profile.removed.contains(&key);
                    if override_allowed {
                        plan.env.insert(key, value);
                    }
                }
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                return;
            }
        }
        let cold = runner::shader_entries(&plan.cache_dir, runner::COLD_CACHE_ENTRIES) < runner::COLD_CACHE_ENTRIES;
        self.last_env = plan.env.iter().filter(|(k, _)| !launch::PATH_VARS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        let saves = runner::RunStorage {
            _guard: guard,
            backups: paths::backups_root(&self.settings),
            before: self.settings.backups.before_launch,
            after: self.settings.backups.after_exit,
            retention: self.settings.backups.retention,
        };
        self.runner.start(plan, paths::logs_dir(), cold, !self.active_extra().is_empty(), Some(saves));
    }

    /// Starts again after a quick exit, without the extra flags.
    pub fn play_without_flags(&mut self, acknowledged: bool) {
        self.ui.start_without_flags = true;
        self.play(acknowledged);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state(engine_override: Option<PathBuf>) -> CoreState {
        let settings = store::Settings { engine_override, ..Default::default() };
        let mut s = CoreState::blank(egui::Context::default(), settings, None, launch::Profile::default(), Vec::new(), Vec::new());
        s.persist = false;
        s
    }

    #[test]
    fn a_stored_launcher_as_engine_is_forgotten_and_never_started() {
        let t = testutil::tempdir("engine-heal");
        let launcher = t.join("victory-launcher.exe");
        std::fs::write(&launcher, b"MZ").unwrap();
        let mut s = state(Some(launcher.clone()));
        assert!(s.own_engine().is_none(), "even before it is dropped it is not used");
        s.drop_unusable_engine();
        assert_eq!(s.settings.engine_override, None);
        assert!(s.notice.as_deref().is_some_and(|n| n.contains("victory-launcher.exe")));
    }

    #[test]
    fn a_real_engine_choice_is_kept_and_other_files_are_refused() {
        let t = testutil::tempdir("engine-keep");
        let engine = t.join("victory.exe");
        std::fs::write(&engine, b"MZ").unwrap();
        let mut s = state(Some(engine.clone()));
        s.drop_unusable_engine();
        assert_eq!(s.engine_exe(), Some(engine.clone()));
        assert!(s.notice.is_none());
        let other = t.join("tool.exe");
        std::fs::write(&other, b"MZ").unwrap();
        assert_eq!(s.use_engine(other), Err(engine::Problem::WrongName));
        assert_eq!(s.settings.engine_override, Some(engine));
    }
}
