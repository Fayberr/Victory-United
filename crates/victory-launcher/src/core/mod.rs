//! "Core" half of the launcher: play, settings, presets, flags, profiles, bug report. Owned by the core worker.
pub mod about;
pub mod firstrun;
pub mod flags;
pub mod play;
pub mod report;
pub mod settings;

use crate::app::Shared;

/// Which full page the window shows. `Main` is the tabbed shell.
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
    /// False in demo mode: nothing is written to disk.
    pub persist: bool,
}

impl Default for CoreState {
    fn default() -> Self {
        Self { force_screen: None, page_scroll: None, persist: true }
    }
}

impl CoreState {
    pub fn new(_ctx: eframe::egui::Context) -> Self {
        Self::default()
    }

    /// Synthetic state for `--ui-demo` and screenshots.
    pub fn demo() -> Self {
        Self { persist: false, ..Self::default() }
    }

    pub fn screen(&self) -> Screen {
        self.force_screen.unwrap_or(Screen::Main)
    }

    /// Once per frame: keep `Shared` current.
    pub fn tick(&mut self, _ctx: &eframe::egui::Context, _shared: &mut Shared) {}
}

/// `Shared` as it looks while the game runs, for demo and screenshots.
pub fn demo_shared() -> Shared {
    Shared {
        game_pid: Some(4242),
        launch_env: vec![
            ("VICTORY_FULLSCREEN".into(), "1".into()),
            ("VICTORY_PANEL".into(), "1".into()),
            ("VICTORY_THIN_THREADS".into(), "6".into()),
        ],
        engine_exe: Some("engine/a1b2c3d4-release/victory.exe".into()),
        log_file: Some("logs/run-20260101-120000.log".into()),
    }
}
