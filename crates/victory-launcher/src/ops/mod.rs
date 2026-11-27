//! "Ops" half of the launcher: monitor, overlay, updates, developer tools. Owned by the ops worker.
//!
//! Idle rules (docs/launcher-contract.md): nothing here polls. `tick` runs once per frame; the only
//! timer is `request_repaint_after(1 s)`, asked for while the game runs and the Monitor tab or the
//! overlay is visible. Worker threads send their result through a channel and call
//! `request_repaint` once.
pub mod dev;
pub mod monitor;
pub mod overlay;
pub mod settings;
pub mod updates;

use crate::app::Shared;
use eframe::egui;
use std::time::Duration;

pub use settings::OpsSettings;

#[derive(Default)]
pub struct OpsState {
    pub cfg: OpsSettings,
    /// False in demo mode: nothing is written to disk.
    pub persist: bool,
    pub monitor: monitor::MonitorState,
    pub updates: updates::UpdatesState,
    pub dev: dev::DevState,
    /// Whether the window is minimised, as of the last frame.
    minimized: bool,
}

impl OpsState {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let cfg = OpsSettings::load();
        let mut s = Self { persist: true, ..Default::default() };
        s.updates = updates::UpdatesState::new(ctx);
        if cfg.check_on_launch {
            s.updates.check(&cfg);
        }
        s.cfg = cfg;
        s
    }

    /// Synthetic state for `--ui-demo` and screenshots.
    pub fn demo() -> Self {
        Self {
            cfg: OpsSettings::default(),
            persist: false,
            monitor: monitor::MonitorState::demo(),
            updates: updates::UpdatesState::demo(),
            dev: dev::DevState::demo(),
            minimized: false,
        }
    }

    /// Once per frame, before the screens. `monitor_visible` is true while the Monitor tab shows.
    pub fn tick(&mut self, ctx: &egui::Context, shared: &Shared, monitor_visible: bool) {
        self.minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.updates.tick();
        let wants = (monitor_visible && !self.minimized) || self.cfg.overlay;
        if shared.game_pid.is_none() {
            self.monitor.forget();
        } else if wants {
            self.monitor.poll(shared.game_pid, std::time::Instant::now());
        }
        if wants && shared.game_pid.is_some() && self.persist {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.cfg.overlay {
            overlay::show(ctx, &self.cfg, self.monitor.summary.as_ref(), shared.game_pid.is_some());
        }
    }

    /// Writes the settings when they changed.
    pub fn save_if_changed(&mut self, before: &OpsSettings) {
        if self.persist && *before != self.cfg {
            self.cfg.save();
        }
    }
}

/// `YYYYMMDD-HHMMSS` in UTC, for file names.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    stamp_of(secs)
}

pub fn stamp_of(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamps() {
        assert_eq!(super::stamp_of(0), "19700101-000000");
        assert_eq!(super::stamp_of(1_800_000_000), "20270115-080000");
        assert_eq!(super::stamp_of(951_782_400 + 86399), "20000229-235959");
    }
}
