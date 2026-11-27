//! "Ops" half of the launcher: monitor, overlay, updates, developer tools. Owned by the ops worker.
//!
//! Idle rules (docs/launcher-contract.md): nothing here polls. `tick` runs once per frame; the only
//! timer is `request_repaint_after(1 s)`, asked for while the game runs and the Monitor tab or the
//! overlay is visible. Worker threads send their result through a channel and call
//! `request_repaint` once.
pub mod dev;
mod hotkey;
pub mod monitor;
pub mod overlay;
pub mod profile;
mod sensors;
pub mod settings;
pub mod updates;

use crate::app::Shared;
use eframe::egui;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering::Relaxed},
};
use std::time::{Duration, Instant};

pub use settings::OpsSettings;

#[derive(Default)]
pub struct OpsState {
    pub cfg: OpsSettings,
    /// False in demo mode: nothing is written to disk.
    pub persist: bool,
    pub monitor: monitor::MonitorState,
    pub updates: updates::UpdatesState,
    pub dev: dev::DevState,
    pub monitor_page: monitor::Page,
    pub live: monitor::Live,
    pub overlay_mode: Option<overlay::Mode>,
    pub hotkey_draft: String,
    pub hotkey_error: Option<String>,
    pub monitor_demo: bool,
    sampler: Option<sensors::Sampler>,
    last_sample: Option<Instant>,
    sensor_target: Option<sensors::Target>,
    hotkey: Option<hotkey::Hotkey>,
    hotkey_config: String,
    hotkey_presses: Arc<AtomicU32>,
    overlay_cache: Option<OverlayCache>,
}

struct OverlayCache {
    at: Instant,
    content: overlay::Content,
    options: settings::OverlaySettings,
    mode: overlay::Mode,
    running: bool,
}

impl OpsState {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let cfg = OpsSettings::load();
        let mut s = Self {
            persist: true,
            ..Default::default()
        };
        s.updates = updates::UpdatesState::new(ctx);
        s.hotkey_draft = cfg.overlay_options.hotkey.clone();
        s.cfg = cfg;
        s
    }

    /// Synthetic state for `--ui-demo` and screenshots.
    pub fn demo() -> Self {
        Self {
            cfg: OpsSettings {
                monitor: settings::MonitorSettings {
                    cards: settings::MonitorSettings::preset(settings::Preset::Everything),
                    ..Default::default()
                },
                overlay_options: settings::OverlaySettings {
                    sparkline: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            live: monitor::Live {
                gpu_load: Some(77.0),
                nvml_load: true,
                gpu_temp: Some(63),
                cpu_load: Some(46.0),
                system_ram: Some((22 << 30, 64 << 30)),
            },
            monitor: monitor::MonitorState::demo(),
            updates: updates::UpdatesState::demo(),
            dev: dev::DevState::demo(),
            ..Default::default()
        }
    }

    /// Once per frame, before the screens. `monitor_visible` is true while the Monitor tab shows.
    pub fn tick(&mut self, ctx: &egui::Context, shared: &Shared, monitor_visible: bool) {
        self.updates.tick();
        if !self.persist && !self.monitor_demo {
            return;
        }
        let (minimized, focused) = ctx.input(|i| {
            (
                i.viewport().minimized.unwrap_or(false),
                i.viewport().focused.unwrap_or(false),
            )
        });
        let now = Instant::now();
        let monitor_visible =
            monitor_visible && !minimized && self.monitor_page == monitor::Page::Live;
        self.update_hotkey(ctx);
        let configured = if self.cfg.overlay {
            overlay::Mode::Full
        } else {
            overlay::Mode::Off
        };
        let presses = self.hotkey_presses.swap(0, Relaxed) % 3;
        for _ in 0..presses {
            self.overlay_mode = Some(self.overlay_mode.unwrap_or(configured).next());
        }
        let mode = self.overlay_mode.unwrap_or(configured);
        let running = shared.game_pid.is_some();
        let overlay_visible = overlay::visible(&self.cfg, mode, running, focused);
        let rate = match (monitor_visible, overlay_visible) {
            (true, true) => Some(
                self.cfg
                    .monitor
                    .rate
                    .duration()
                    .min(self.cfg.overlay_options.rate.duration()),
            ),
            (true, false) => Some(self.cfg.monitor.rate.duration()),
            (false, true) => Some(self.cfg.overlay_options.rate.duration()),
            _ => None,
        };
        if let (Some(pid), Some(rate)) = (shared.game_pid, rate) {
            self.monitor.refresh(pid, now, rate);
            if self.persist && self.cfg.monitor.live_sensors {
                self.sample(ctx, now, pid);
            } else if self.persist {
                self.clear_sensors();
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        } else {
            if !running {
                self.monitor.forget();
            }
            if self.persist {
                self.clear_sensors();
            }
        }
        if overlay_visible {
            self.update_overlay(ctx, now, running, mode);
        } else {
            self.overlay_cache = None;
        }
    }

    fn clear_sensors(&mut self) {
        self.sampler = None;
        self.last_sample = None;
        self.sensor_target = None;
        self.live = monitor::Live::default();
    }

    fn sample(&mut self, ctx: &egui::Context, now: Instant, pid: u32) {
        let target = sensors::Target {
            pid,
            luid: self.monitor.summary.as_ref().and_then(|s| s.adapter_luid),
        };
        if self.sensor_target != Some(target) {
            self.sensor_target = Some(target);
            self.last_sample = None;
            self.live = monitor::Live::default();
        }
        if self.sampler.is_none() {
            let wake = ctx.clone();
            self.sampler = sensors::Sampler::start(move || wake.request_repaint()).ok();
        }
        let Some(sampler) = &self.sampler else { return };
        if let Some(result) = sampler.take(target) {
            self.live = result;
        }
        if self
            .last_sample
            .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_millis(995))
        {
            self.last_sample = Some(now);
            sampler.request(target);
        }
    }

    fn update_overlay(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        running: bool,
        mode: overlay::Mode,
    ) {
        let options = &self.cfg.overlay_options;
        let due = self.overlay_cache.as_ref().is_none_or(|cache| {
            cache.options != *options
                || cache.mode != mode
                || cache.running != running
                || now
                    .saturating_duration_since(cache.at)
                    .saturating_add(Duration::from_millis(12))
                    >= options.rate.duration()
        });
        if due {
            let summary = self
                .monitor
                .summary
                .as_ref()
                .map(|s| s.with_live(self.live));
            self.overlay_cache = Some(OverlayCache {
                at: now,
                options: options.clone(),
                mode,
                running,
                content: overlay::Content::new(
                    &self.cfg,
                    summary.as_ref(),
                    &self.monitor.history,
                    running,
                    mode,
                ),
            });
        }
        if let Some(cache) = &self.overlay_cache {
            overlay::show(ctx, &self.cfg, &cache.content);
        }
    }

    fn update_hotkey(&mut self, ctx: &egui::Context) {
        if !self.persist {
            return;
        }
        let configured = if self.cfg.overlay_options.mode == settings::OverlayMode::Window {
            self.cfg.overlay_options.hotkey.as_str()
        } else {
            ""
        };
        if configured == self.hotkey_config {
            return;
        }
        self.hotkey = None;
        self.hotkey_config = configured.to_owned();
        self.hotkey_error = None;
        let key = hotkey::parse(configured);
        match key {
            Ok(Some(key)) => {
                let presses = Arc::clone(&self.hotkey_presses);
                let wake = ctx.clone();
                match hotkey::Hotkey::register(key, move || {
                    presses.fetch_add(1, Relaxed);
                    wake.request_repaint();
                }) {
                    Ok(hotkey) => self.hotkey = Some(hotkey),
                    Err(error) => self.hotkey_error = Some(error.to_string()),
                }
            }
            Ok(None) => {}
            Err(error) => self.hotkey_error = Some(error.to_string()),
        }
    }

    /// Writes the settings when they changed.
    pub fn save_if_changed(&mut self, before: &OpsSettings) {
        if self.persist && *before != self.cfg {
            if let Err(error) = self.cfg.save() {
                self.dev.note = Some(error.to_string());
            }
        }
    }
}

/// `YYYYMMDD-HHMMSS` in UTC, for file names.
pub fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
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
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
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
