//! Updates tab: engine channel, check, download and install with progress, roll back, remove old builds.
//! Network and disk work runs on a thread that reports through a channel; the UI never blocks.
use crate::{app::Shared, core::store::UpdateSchedule, i18n::{tr, trf}, ops::{OpsSettings, OpsState}, paths, theme};
use eframe::egui;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc};
use victory_update::{Engine, Error, Installed, Update, http::System};

/// How many builds `cleanup` keeps (the active one and the rollback target always stay).
pub const KEEP_BUILDS: usize = 3;

pub enum Msg {
    Checked(Result<Update, String>),
    Installed(Result<String, String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    /// Downloading or installing.
    Working,
}

pub struct UpdatesState {
    ctx: Option<egui::Context>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    pub phase: Phase,
    pub update: Option<Update>,
    pub note: Option<(bool, String)>, // (is error, text)
    pub installed: Vec<Installed>,
    /// Bytes done and total of the running download.
    pub progress: Arc<(AtomicU64, AtomicU64)>,
    cancel: Arc<AtomicBool>,
    pub confirm_cleanup: bool,
    pub last_check_started: Option<u64>,
    banner_dismissed: bool,
}

impl Default for UpdatesState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            ctx: None,
            tx,
            rx,
            phase: Phase::Idle,
            update: None,
            note: None,
            installed: Vec::new(),
            progress: Default::default(),
            cancel: Default::default(),
            confirm_cleanup: false,
            last_check_started: None,
            banner_dismissed: false,
        }
    }
}

fn say(e: &Error) -> String {
    match e {
        Error::Offline(_) => tr("updates.offline").to_string(),
        other => other.to_string(),
    }
}

impl UpdatesState {
    pub fn new(ctx: egui::Context) -> Self {
        let mut s = Self { ctx: Some(ctx), ..Default::default() };
        s.refresh_installed();
        s
    }

    pub fn refresh_installed(&mut self) {
        self.installed = victory_update::installed(&paths::engine_root());
    }

    /// Drains finished work. Called once per frame; never blocks.
    pub fn tick(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.phase = Phase::Idle;
            match m {
                Msg::Checked(Ok(u)) => {
                    self.note = Some((false, if u.newer { tr("updates.found").into() } else { tr("updates.current").into() }));
                    self.update = Some(u);
                }
                Msg::Checked(Err(e)) => self.note = Some((true, e)),
                Msg::Installed(Ok(name)) => {
                    self.note = Some((false, trf("updates.installed", &[("name", &name)])));
                    self.update = None;
                    self.refresh_installed();
                }
                Msg::Installed(Err(e)) => self.note = Some((true, e)),
            }
        }
    }

    fn wake(ctx: &Option<egui::Context>) {
        if let Some(c) = ctx {
            c.request_repaint();
        }
    }

    pub fn check(&mut self, cfg: &OpsSettings) {
        if self.phase != Phase::Idle {
            return;
        }
        self.phase = Phase::Checking;
        self.last_check_started = Some(crate::core::storage::now().as_secs());
        self.banner_dismissed = false;
        self.note = None;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let (channel, kind) = (cfg.channel.clone(), cfg.engine_kind.clone());
        let have: Vec<String> = self.installed.iter().map(|b| b.name.clone()).collect();
        std::thread::spawn(move || {
            let r = check_now(&channel, &kind, &have).map_err(|e| say(&e));
            let _ = tx.send(Msg::Checked(r));
            Self::wake(&ctx);
        });
    }

    pub fn install(&mut self, update: Update) {
        if self.phase != Phase::Idle {
            return;
        }
        self.phase = Phase::Working;
        self.note = None;
        self.cancel.store(false, Ordering::Relaxed);
        self.progress.0.store(0, Ordering::Relaxed);
        self.progress.1.store(update.engine.size, Ordering::Relaxed);
        let (tx, ctx, progress, cancel) = (self.tx.clone(), self.ctx.clone(), self.progress.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let r = run_install(&update.engine, &update.folder, &progress, &cancel, &ctx);
            let _ = tx.send(Msg::Installed(r.map(|_| update.folder.clone()).map_err(|e| say(&e))));
            Self::wake(&ctx);
        });
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn rollback(&mut self, to: Option<&str>) {
        self.note = Some(match rollback_now(&paths::engine_root(), to) {
            Ok(n) => (false, trf("updates.switched", &[("name", &n)])),
            Err(e) => (true, e.to_string()),
        });
        self.refresh_installed();
    }

    pub fn cleanup(&mut self) {
        self.note = Some(match cleanup_now(&paths::engine_root()) {
            Ok(removed) => (false, trf("updates.removed", &[("n", &removed.len().to_string())])),
            Err(error) => (true, error.to_string()),
        });
        self.refresh_installed();
    }

    pub fn demo() -> Self {
        let mut s = Self::default();
        let engine = Engine {
            build: "e5f60718aa".into(),
            translator: "0.9.4".into(),
            kind: "release".into(),
            url: String::new(),
            sha256: "0".repeat(64),
            size: 96 << 20,
            notes: "Faster shader translation start.\nFewer stalls when a new area loads.\nThe launcher can now show frame times.".into(),
        };
        let manifest = victory_update::Manifest {
            schema: 1,
            launcher_version: "0.2.0".into(),
            launcher_url: "https://github.com/Fayberr/Victory-United/releases".into(),
            channels: Default::default(),
        };
        s.update = Some(Update { manifest, folder: victory_update::folder_name(&engine), engine, newer: true });
        let t = std::time::SystemTime::now();
        let b = |name: &str, cur, prev, mb: u64| Installed { name: name.into(), is_current: cur, is_previous: prev, modified: t, bytes: mb << 20 };
        s.installed = vec![b("a1b2c3d4-release", true, false, 212), b("9f8e7d6c-release", false, true, 209), b("a1b2c3d4-diagnostics", false, false, 241)];
        s.note = Some((false, tr("updates.found").into()));
        s
    }
}

/// Asks GitHub for the newest engine of `channel` and `kind`. `have` lists the installed folder names.
pub fn check_now(channel: &str, kind: &str, have: &[String]) -> Result<Update, Error> {
    victory_update::check(&System, channel, kind, have)
}

/// Downloads, verifies and installs an engine, makes it the active build and removes old builds.
/// `progress(done, total)` is called for every chunk; return false to cancel.
pub fn install_now(
    engine: &Engine,
    folder: &str,
    downloads: &Path,
    engine_root: &Path,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<(), Error> {
    let _guard = build_lock(engine_root)?;
    let zip = downloads.join(format!("{folder}.zip"));
    victory_update::download(&System, engine, &zip, progress)?;
    let r = victory_update::install(&zip, engine_root, folder);
    let _ = std::fs::remove_file(&zip);
    r?;
    victory_update::cleanup(engine_root, KEEP_BUILDS);
    Ok(())
}

fn build_lock(engine_root: &Path) -> Result<std::fs::File, Error> {
    crate::core::storage::lock(&engine_root.join("updates.lock"))
        .map_err(|_| Error::Io(std::io::Error::other("Engine builds are being updated or the engine folder cannot be written.")))
}

pub fn rollback_now(engine_root: &Path, to: Option<&str>) -> Result<String, Error> {
    let _guard = build_lock(engine_root)?;
    let available = victory_update::installed(engine_root);
    let target = available.iter().find(|build| match to {
        Some(name) => build.name == name,
        None => build.is_previous,
    });
    let build = target.ok_or_else(|| Error::BadArchive("There is no matching installed build.".into()))?;
    victory_update::rollback(engine_root, Some(&build.name))
}

pub fn cleanup_now(engine_root: &Path) -> Result<Vec<String>, Error> {
    let _guard = build_lock(engine_root)?;
    Ok(victory_update::cleanup(engine_root, KEEP_BUILDS))
}

/// Download and install on the worker thread. Progress is published at most about 10 times a second.
fn run_install(
    engine: &Engine,
    folder: &str,
    progress: &(AtomicU64, AtomicU64),
    cancel: &AtomicBool,
    ctx: &Option<egui::Context>,
) -> Result<(), Error> {
    let mut last = std::time::Instant::now();
    install_now(engine, folder, &paths::data_dir().join("downloads"), &paths::engine_root(), &mut |done, total| {
        progress.0.store(done, Ordering::Relaxed);
        progress.1.store(total, Ordering::Relaxed);
        if last.elapsed() >= std::time::Duration::from_millis(100) {
            last = std::time::Instant::now();
            UpdatesState::wake(ctx);
        }
        !cancel.load(Ordering::Relaxed)
    })
}

fn size_text(b: u64) -> String {
    format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
}

pub fn check_due(schedule: UpdateSchedule, last: Option<u64>, now: u64, startup: bool) -> bool {
    match schedule {
        UpdateSchedule::Off => false,
        UpdateSchedule::AtStartup => startup,
        UpdateSchedule::Daily => last.is_none_or(|last| last > now || now - last >= 86_400),
    }
}

pub fn play_banner(state: &mut UpdatesState, ui: &mut egui::Ui) -> bool {
    let Some(update) = &state.update else { return false };
    let launcher = newer_launcher(&update.manifest.launcher_version);
    if state.banner_dismissed || (!update.newer && !launcher) {
        return false;
    }
    let mut open = false;
    theme::card(ui).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            if update.newer {
                ui.label(trf("startup_updates.engine", &[("build", &update.folder)]));
                if ui.small_button(tr("startup_updates.open")).clicked() {
                    open = true;
                }
            }
            if launcher {
                ui.label(trf("startup_updates.launcher", &[("version", &update.manifest.launcher_version)]));
                if update.manifest.launcher_url.starts_with("https://") {
                    ui.hyperlink_to(tr("updates.open_release"), &update.manifest.launcher_url);
                }
            }
            if ui.small_button(tr("startup_updates.dismiss")).clicked() {
                state.banner_dismissed = true;
            }
        });
    });
    ui.add_space(12.0);
    open
}

pub fn show(ops: &mut OpsState, shared: &Shared, ui: &mut egui::Ui) {
    let before = ops.cfg.clone();
    ui.label(theme::strong(tr("updates.title")).size(24.0));
    ui.add_space(10.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let running = shared.game_pid.is_some();
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let current = victory_update::installed(&paths::engine_root()).into_iter().find(|b| b.is_current).map(|b| b.name);
            let current = ops.updates.installed.iter().find(|b| b.is_current).map(|b| b.name.clone()).or(current).unwrap_or_else(|| tr("updates.none").to_string());
            ui.label(trf("updates.engine_now", &[("name", &current)]));
            ui.label(trf("updates.launcher_now", &[("version", env!("CARGO_PKG_VERSION"))]));
            if let Some(u) = &ops.updates.update {
                if newer_launcher(&u.manifest.launcher_version) {
                    ui.horizontal(|ui| {
                        ui.label(trf("updates.launcher_new", &[("version", &u.manifest.launcher_version)]));
                        if !u.manifest.launcher_url.is_empty() {
                            ui.hyperlink_to(tr("updates.open_release"), &u.manifest.launcher_url);
                        }
                    });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(tr("updates.channel"));
                for (id, label) in [("stable", tr("updates.stable")), ("dev", tr("updates.dev"))] {
                    ui.selectable_value(&mut ops.cfg.channel, id.to_string(), label);
                }
                ui.add_space(12.0);
                ui.label(tr("updates.kind"));
                for (id, label) in [("release", tr("updates.kind_release")), ("diagnostics", tr("updates.kind_diag"))] {
                    ui.selectable_value(&mut ops.cfg.engine_kind, id.to_string(), label);
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let idle = ops.updates.phase == Phase::Idle;
                if ui.add_enabled(idle, egui::Button::new(tr("updates.check"))).clicked() {
                    let cfg = ops.cfg.clone();
                    ops.updates.check(&cfg);
                }
                if ops.updates.phase == Phase::Checking {
                    ui.label(theme::dim(tr("updates.checking")));
                }
            });
            if let Some((is_err, text)) = &ops.updates.note {
                ui.add_space(4.0);
                ui.label(if *is_err { theme::error_text(ui, text.clone()) } else { theme::dim(text.clone()) });
            }
        });
        if let Some(u) = ops.updates.update.clone() {
            ui.add_space(12.0);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::strong(trf("updates.release_title", &[("name", &u.folder)])));
                ui.label(theme::dim(trf("updates.release_facts", &[("translator", &u.engine.translator), ("size", &size_text(u.engine.size))])).size(12.5));
                ui.add_space(6.0);
                for line in u.engine.notes.lines() {
                    ui.label(format!("{}  {}", "-", line));
                }
                ui.add_space(8.0);
                match ops.updates.phase {
                    Phase::Working => {
                        let (d, t) = (ops.updates.progress.0.load(Ordering::Relaxed), ops.updates.progress.1.load(Ordering::Relaxed));
                        let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).text(format!("{} / {}", size_text(d), size_text(t))));
                        if ui.button(tr("updates.cancel")).clicked() {
                            ops.updates.cancel();
                        }
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            let label = if u.newer { tr("updates.install") } else { tr("updates.reinstall") };
                            if ui.add_enabled(!running && ops.updates.phase == Phase::Idle, egui::Button::new(label)).clicked() {
                                ops.updates.install(u.clone());
                            }
                            if running {
                                ui.label(theme::warn_text(ui, tr("updates.close_game")));
                            }
                        });
                    }
                }
            });
        }
        ui.add_space(12.0);
        installed_card(ui, ops, running);
    });
    ops.save_if_changed(&before);
}

fn installed_card(ui: &mut egui::Ui, ops: &mut OpsState, running: bool) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("updates.installed_title")));
        ui.add_space(6.0);
        if ops.updates.installed.is_empty() {
            ui.label(theme::dim(tr("updates.no_builds")));
        }
        let mut switch_to = None;
        for b in ops.updates.installed.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&b.name).monospace());
                ui.label(theme::dim(size_text(b.bytes)).size(12.5));
                if b.is_current {
                    theme::badge(ui, tr("updates.active"), true);
                } else if b.is_previous {
                    theme::badge(ui, tr("updates.previous"), false);
                }
                if !b.is_current && ui.add_enabled(!running, egui::Button::new(tr("updates.use"))).clicked() {
                    switch_to = Some(b.name.clone());
                }
            });
        }
        if let Some(n) = switch_to {
            ops.updates.rollback(Some(&n));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let has_prev = ops.updates.installed.iter().any(|b| b.is_previous);
            if ui.add_enabled(has_prev && !running, egui::Button::new(tr("updates.rollback"))).clicked() {
                ops.updates.rollback(None);
            }
            if ops.updates.confirm_cleanup {
                ui.label(trf("updates.cleanup_ask", &[("n", &KEEP_BUILDS.to_string())]));
                if ui.button(tr("updates.yes")).clicked() {
                    ops.updates.cleanup();
                    ops.updates.confirm_cleanup = false;
                }
                if ui.button(tr("updates.no")).clicked() {
                    ops.updates.confirm_cleanup = false;
                }
            } else if ui.add_enabled(ops.updates.installed.len() > 2, egui::Button::new(tr("updates.cleanup"))).clicked() {
                ops.updates.confirm_cleanup = true;
            }
        });
    });
}

/// True when `theirs` is a newer version than this launcher (numeric dotted compare).
pub fn newer_launcher(theirs: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(theirs) > parse(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_schedule_respects_the_day_and_opt_out() {
        use super::{check_due, UpdateSchedule::*};
        assert!(check_due(Daily, None, 1, true));
        assert!(!check_due(Daily, Some(0), 86_399, true));
        assert!(check_due(Daily, Some(0), 86_400, false));
        assert!(check_due(Daily, Some(60), 40, true));
        assert!(!check_due(Off, None, 86_400, true));
        assert!(check_due(AtStartup, Some(0), 1, true));
        assert!(!check_due(AtStartup, None, 86_400, false));
    }

    use super::*;

    #[test]
    fn launcher_versions_compare_numerically() {
        assert!(newer_launcher("99.0.0"));
        assert!(newer_launcher("v0.10.0") || env!("CARGO_PKG_VERSION") != "0.1.0");
        assert!(!newer_launcher("0.0.1"));
        assert!(!newer_launcher(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn a_failed_check_is_reported_not_raised() {
        let mut s = UpdatesState::default();
        s.phase = Phase::Checking;
        s.tx.send(Msg::Checked(Err("offline".into()))).unwrap();
        s.tick();
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(s.note, Some((true, "offline".into())));
    }
}
