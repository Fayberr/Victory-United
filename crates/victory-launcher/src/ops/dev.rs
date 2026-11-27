//! Developer tab: engine builds, profiler, log viewer, folders, shader cache, active environment.
use crate::{app::Shared, i18n::{tr, trf}, ops::OpsState, paths, theme};
use eframe::egui;
use std::path::{Path, PathBuf};

/// Lines of the log the viewer keeps.
const TAIL_LINES: usize = 2000;
/// How much of the end of a log file is read at most.
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Default)]
pub struct DevState {
    pub log_path: Option<PathBuf>,
    pub log_text: String,
    log_len: u64,
    pub log_note: Option<String>,
    pub search: String,
    pub profile_from: String,
    pub profile_to: String,
    pub profile_ms: String,
    pub profile_file: String,
    pub profile_note: Option<String>,
    pub cache_bytes: Option<u64>,
    pub confirm_clear: bool,
    pub note: Option<String>,
}

impl DevState {
    pub fn demo() -> Self {
        let mut log = String::new();
        for i in 0..40 {
            log.push_str(&format!("[00:{:02}:{:02}] INFO victory_gpu: presenting to a window {{ width: 3840, height: 2160 }}\n", i / 4, (i * 7) % 60));
        }
        log.push_str("[00:11:03] INFO victory_gpu: 91.4 fps, 10.9 ms/frame\n");
        Self {
            log_path: Some("logs/run-20260101-120000.log".into()),
            log_text: log,
            profile_from: "20".into(),
            profile_to: "40".into(),
            profile_ms: "5".into(),
            profile_file: "profile.txt".into(),
            cache_bytes: Some(3_400 << 20),
            ..Default::default()
        }
    }
}

/// The last `max_lines` lines of the end of a file, reading at most `max_bytes`. Returns the text and
/// the file length at the time of reading.
pub fn read_tail(path: &Path, max_lines: usize, max_bytes: u64) -> std::io::Result<(String, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    f.take(max_bytes).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably cut in the middle
    }
    let skip = lines.len().saturating_sub(max_lines);
    Ok((lines[skip..].join("\n"), len))
}

/// The newest `run-*.log` in `dir`.
pub fn newest_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("run-") && n.ends_with(".log")
        })
        .max_by_key(|e| e.file_name())
        .map(|e| e.path())
}

pub fn dir_bytes(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Deletes what is inside the shader cache folder. Refuses any folder that is not named `cache`, so a
/// wrong path can never wipe something else. Returns the bytes freed.
pub fn clear_cache(root: &Path) -> Result<u64, String> {
    if root.file_name().and_then(|n| n.to_str()) != Some("cache") {
        return Err(format!("{} is not a cache folder", root.display()));
    }
    if !root.is_dir() {
        return Ok(0);
    }
    let size = dir_bytes(root);
    for e in std::fs::read_dir(root).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let r = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        r.map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(size)
}

/// The value of `VICTORY_PROFILE`, or why it is not valid. Matches what the engine's parser accepts:
/// from and to in seconds, the sampling step in milliseconds, then the file.
pub fn profile_value(from: &str, to: &str, ms: &str, file: &str) -> Result<String, &'static str> {
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|v| *v >= 0.0);
    let (f, t, m) = (num(from).ok_or("from")?, num(to).ok_or("to")?, num(ms).ok_or("ms")?);
    if f >= t {
        return Err("order");
    }
    if m <= 0.0 {
        return Err("ms");
    }
    if file.trim().is_empty() || file.contains(',') {
        return Err("file");
    }
    Ok(format!("{},{},{},{}", from.trim(), to.trim(), ms.trim(), file.trim()))
}

fn open_folder(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(not(windows))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(p).spawn();
}

fn mb(b: u64) -> String {
    format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
}

impl DevState {
    fn load_log(&mut self, shared: &Shared) {
        let path = shared.log_file.clone().filter(|p| p.is_file()).or_else(|| newest_log(&paths::logs_dir()));
        self.log_path = path.clone();
        match path.map(|p| read_tail(&p, TAIL_LINES, TAIL_BYTES)) {
            Some(Ok((text, len))) => {
                self.log_text = text;
                self.log_len = len;
                self.log_note = None;
            }
            Some(Err(e)) => self.log_note = Some(e.to_string()),
            None => self.log_note = Some(tr("dev.no_log").to_string()),
        }
    }

    /// Whether the file is longer than what was loaded (one metadata call, only on a button).
    fn grown(&self) -> Option<bool> {
        let len = std::fs::metadata(self.log_path.as_ref()?).ok()?.len();
        Some(len != self.log_len)
    }
}

pub fn show(ops: &mut OpsState, shared: &Shared, ui: &mut egui::Ui) {
    let before = ops.cfg.clone();
    ui.label(theme::strong(tr("dev.title")).size(24.0));
    ui.add_space(10.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        builds_card(ui, ops, shared);
        ui.add_space(12.0);
        profiler_card(ui, ops);
        ui.add_space(12.0);
        log_card(ui, ops, shared);
        ui.add_space(12.0);
        folders_card(ui, ops, shared);
        ui.add_space(12.0);
        env_card(ui, shared);
    });
    ops.save_if_changed(&before);
}

fn builds_card(ui: &mut egui::Ui, ops: &mut OpsState, shared: &Shared) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.engine")));
        let running = shared.game_pid.is_some();
        let mut switch = None;
        for b in ops.updates.installed.clone() {
            ui.horizontal(|ui| {
                let kind = if b.name.ends_with("-diagnostics") { tr("dev.kind_diag") } else { tr("dev.kind_normal") };
                ui.label(egui::RichText::new(&b.name).monospace());
                ui.label(theme::dim(kind).size(12.5));
                if b.is_current {
                    theme::badge(ui, tr("updates.active"), true);
                } else if ui.add_enabled(!running, egui::Button::new(tr("updates.use"))).clicked() {
                    switch = Some(b.name.clone());
                }
            });
        }
        if let Some(n) = switch {
            ops.updates.rollback(Some(&n));
        }
        if ops.updates.installed.is_empty() {
            ui.label(theme::dim(tr("updates.no_builds")));
        }
        ui.label(theme::dim(tr("dev.diag_help")).size(12.5));
        ui.add_space(8.0);
        ui.label(tr("dev.own_engine"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut ops.cfg.own_engine).desired_width((ui.available_width() - 330.0).max(120.0)).hint_text(tr("dev.own_hint")));
            #[cfg(windows)]
            if ui.button(tr("dev.browse")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("victory.exe", &["exe"]).pick_file() {
                    ops.cfg.own_engine = p.display().to_string();
                }
            }
            if ui.add_enabled(!ops.cfg.own_engine.is_empty(), egui::Button::new(tr("dev.use_installed"))).clicked() {
                ops.cfg.own_engine.clear();
            }
        });
        if !ops.cfg.own_engine.is_empty() && !Path::new(&ops.cfg.own_engine).is_file() {
            ui.label(theme::warn_text(ui, tr("dev.own_missing")));
        }
    });
}

fn profiler_card(ui: &mut egui::Ui, ops: &mut OpsState) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.profiler")));
        ui.add(egui::Label::new(theme::dim(tr("dev.profiler_help")).size(12.5)).wrap());
        ui.add_space(6.0);
        let d = &mut ops.dev;
        ui.horizontal(|ui| {
            for (label, field, w) in [(tr("dev.p_from"), &mut d.profile_from, 50.0), (tr("dev.p_to"), &mut d.profile_to, 50.0), (tr("dev.p_ms"), &mut d.profile_ms, 40.0)] {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(field).desired_width(w));
            }
            ui.label(tr("dev.p_file"));
            ui.add(egui::TextEdit::singleline(&mut d.profile_file).desired_width(160.0));
        });
        let value = profile_value(&d.profile_from, &d.profile_to, &d.profile_ms, &d.profile_file);
        ui.horizontal(|ui| match &value {
            Ok(v) => {
                ui.label(egui::RichText::new(format!("VICTORY_PROFILE={v}")).monospace());
                if ui.button(tr("dev.copy")).clicked() {
                    ui.ctx().copy_text(format!("VICTORY_PROFILE={v}"));
                }
                if ui.button(tr("dev.add_flag")).clicked() {
                    d.profile_note = Some(match add_launch_flag("VICTORY_PROFILE", v) {
                        Ok(()) => tr("dev.flag_added").to_string(),
                        Err(e) => e,
                    });
                }
            }
            Err(_) => {
                ui.label(theme::warn_text(ui, tr("dev.profile_invalid")));
            }
        });
        if let Some(n) = &d.profile_note {
            ui.label(theme::dim(n.clone()).size(12.5));
        }
    });
}

/// Adds `NAME=value` to `launch-flags.txt` (the core half's extra flags file), replacing an older line
/// for the same name.
pub fn add_launch_flag(name: &str, value: &str) -> Result<(), String> {
    let path = paths::data_dir().join("launch-flags.txt");
    add_flag_to(&path, name, value)
}

pub fn add_flag_to(path: &Path, name: &str, value: &str) -> Result<(), String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().filter(|l| l.split('=').next().map(str::trim) != Some(name)).map(str::to_string).collect();
    lines.push(format!("{name}={value}"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn log_card(ui: &mut egui::Ui, ops: &mut OpsState, shared: &Shared) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.log")));
        ui.horizontal(|ui| {
            if ui.button(tr("dev.log_load")).clicked() {
                ops.dev.load_log(shared);
            }
            if ui.add_enabled(ops.dev.log_path.is_some(), egui::Button::new(tr("dev.log_check"))).clicked() {
                let msg = match ops.dev.grown() {
                    Some(true) => tr("dev.log_changed"),
                    Some(false) => tr("dev.log_same"),
                    None => tr("dev.no_log"),
                };
                ops.dev.log_note = Some(msg.to_string());
            }
            if ui.add_enabled(!ops.dev.log_text.is_empty(), egui::Button::new(tr("dev.copy"))).clicked() {
                ui.ctx().copy_text(ops.dev.log_text.clone());
            }
            ui.add(egui::TextEdit::singleline(&mut ops.dev.search).hint_text(tr("dev.search")).desired_width(200.0));
        });
        if let Some(p) = &ops.dev.log_path {
            ui.label(theme::dim(p.display().to_string()).size(12.5));
        }
        if let Some(n) = &ops.dev.log_note {
            ui.label(theme::dim(n.clone()).size(12.5));
        }
        let needle = ops.dev.search.to_lowercase();
        let lines: Vec<&str> = ops.dev.log_text.lines().filter(|l| needle.is_empty() || l.to_lowercase().contains(&needle)).collect();
        egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(egui::Margin::same(8)).corner_radius(6).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).stick_to_bottom(true).show_rows(ui, 15.0, lines.len(), |ui, range| {
                for l in &lines[range] {
                    ui.label(egui::RichText::new(*l).monospace().size(12.0));
                }
            });
        });
        if !ops.dev.search.is_empty() {
            ui.label(theme::dim(trf("dev.matches", &[("n", &lines.len().to_string())])).size(12.5));
        }
    });
}

fn folders_card(ui: &mut egui::Ui, ops: &mut OpsState, shared: &Shared) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.folders")));
        ui.horizontal_wrapped(|ui| {
            for (label, path) in [
                (tr("dev.open_cache"), paths::cache_root()),
                (tr("dev.open_logs"), paths::logs_dir()),
                (tr("dev.open_reports"), paths::reports_dir()),
                (tr("dev.open_engine"), paths::engine_root()),
            ] {
                if ui.button(label).clicked() {
                    open_folder(&path);
                }
            }
        });
        ui.add_space(8.0);
        ui.label(theme::strong(tr("dev.cache")));
        ui.label(theme::dim(tr("dev.cache_help")).size(12.5));
        let running = shared.game_pid.is_some();
        ui.horizontal(|ui| {
            if ui.button(tr("dev.cache_measure")).clicked() {
                ops.dev.cache_bytes = Some(dir_bytes(&paths::cache_root()));
            }
            if let Some(b) = ops.dev.cache_bytes {
                ui.label(trf("dev.cache_size", &[("size", &mb(b))]));
            }
            if ops.dev.confirm_clear {
                ui.label(theme::warn_text(ui, tr("dev.cache_ask")));
                if ui.button(tr("dev.cache_yes")).clicked() {
                    ops.dev.note = Some(match clear_cache(&paths::cache_root()) {
                        Ok(b) => trf("dev.cache_cleared", &[("size", &mb(b))]),
                        Err(e) => e,
                    });
                    ops.dev.cache_bytes = Some(dir_bytes(&paths::cache_root()));
                    ops.dev.confirm_clear = false;
                }
                if ui.button(tr("updates.no")).clicked() {
                    ops.dev.confirm_clear = false;
                }
            } else if ui.add_enabled(!running, egui::Button::new(tr("dev.cache_clear"))).clicked() {
                ops.dev.confirm_clear = true;
            }
        });
        if let Some(n) = &ops.dev.note {
            ui.label(theme::dim(n.clone()).size(12.5));
        }
    });
}

fn env_card(ui: &mut egui::Ui, shared: &Shared) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.env")));
        ui.label(theme::dim(tr("dev.env_help")).size(12.5));
        if shared.launch_env.is_empty() {
            ui.label(theme::dim(tr("dev.env_none")));
        }
        for (k, v) in &shared.launch_env {
            ui.label(egui::RichText::new(format!("{k}={v}")).monospace().size(12.5));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("run-1.log");
        std::fs::write(&p, (0..5000).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
        let (t, len) = read_tail(&p, 2000, 1 << 20).unwrap();
        assert_eq!(t.lines().count(), 2000);
        assert!(t.ends_with("line 4999"));
        assert_eq!(len, std::fs::metadata(&p).unwrap().len());
        // A small byte window never returns a cut first line.
        let (t, _) = read_tail(&p, 2000, 100).unwrap();
        assert!(t.lines().all(|l| l.starts_with("line ") && l.len() >= 6));
    }

    #[test]
    fn newest_log_by_name() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["run-20260101-120000.log", "run-20260102-080000.log", "other.txt"] {
            std::fs::write(dir.path().join(n), "x").unwrap();
        }
        assert_eq!(newest_log(dir.path()).unwrap().file_name().unwrap(), "run-20260102-080000.log");
        assert!(newest_log(&dir.path().join("none")).is_none());
    }

    #[test]
    fn cache_clear_is_fenced() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(cache.join("game1")).unwrap();
        std::fs::write(cache.join("game1/a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(cache.join("b.bin"), vec![0u8; 500]).unwrap();
        // Something that is not named cache is refused and untouched.
        let other = dir.path().join("settings");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("keep.toml"), "x").unwrap();
        assert!(clear_cache(&other).is_err());
        assert!(other.join("keep.toml").exists());
        assert_eq!(clear_cache(&cache).unwrap(), 1500);
        assert!(cache.is_dir() && std::fs::read_dir(&cache).unwrap().count() == 0);
        assert_eq!(clear_cache(&dir.path().join("x/cache")).unwrap(), 0);
    }

    #[test]
    fn profile_value_matches_the_engine_parser() {
        assert_eq!(profile_value("20", "40", "5", "p.txt").unwrap(), "20,40,5,p.txt");
        assert!(profile_value("40", "20", "5", "p.txt").is_err());
        assert!(profile_value("a", "20", "5", "p.txt").is_err());
        assert!(profile_value("1", "2", "0", "p.txt").is_err());
        assert!(profile_value("1", "2", "5", "").is_err());
        assert!(profile_value("1", "2", "5", "a,b").is_err());
    }

    #[test]
    fn launch_flag_replaces_an_older_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("launch-flags.txt");
        std::fs::write(&p, "VICTORY_X=1\nVICTORY_PROFILE=old\n").unwrap();
        add_flag_to(&p, "VICTORY_PROFILE", "1,2,3,f").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "VICTORY_X=1\nVICTORY_PROFILE=1,2,3,f\n");
    }
}
