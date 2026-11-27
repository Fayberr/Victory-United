//! Developer tab: engine builds, profiler, log viewer, folders, shader cache, active environment.
use crate::{
    app::Shared,
    i18n::{tr, trf},
    ops::{OpsState, profile},
    paths, theme,
};
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
    pub profiler_tab: bool,
    profile: Option<profile::Profile>,
    profile_path: Option<PathBuf>,
    profile_thread: Option<u32>,
    profile_search: String,
    profile_note: Option<String>,
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
            profile: profile::parse(profile::DEMO.as_bytes()).ok(),
            profile_path: Some("profile-session.txt".into()),
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
    for e in std::fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let p = e.path();
        let r = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        r.map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(size)
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
        let path = shared
            .log_file
            .clone()
            .filter(|p| p.is_file())
            .or_else(|| newest_log(&paths::logs_dir()));
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

pub fn show(ops: &mut OpsState, shared: &Shared, scroll: Option<f32>, ui: &mut egui::Ui) {
    let before = ops.cfg.clone();
    ui.label(theme::strong(tr("dev.title")).size(24.0));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut ops.dev.profiler_tab, false, tr("dev.tools"));
        ui.selectable_value(&mut ops.dev.profiler_tab, true, tr("dev.profiler"));
    });
    let mut area = egui::ScrollArea::vertical()
        .id_salt(ops.dev.profiler_tab)
        .auto_shrink([false, false]);
    if let Some(scroll) = scroll {
        area = area.vertical_scroll_offset(scroll);
    }
    area.show(ui, |ui| {
        if ops.dev.profiler_tab {
            profile_overview(ui, ops);
            ui.add_space(12.0);
            profiler_card(ui, ops);
        } else {
            builds_card(ui, ops, shared);
            ui.add_space(12.0);
            log_card(ui, ops, shared);
            ui.add_space(12.0);
            folders_card(ui, ops, shared);
            ui.add_space(12.0);
            env_card(ui, shared);
        }
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
                let kind = if b.name.ends_with("-diagnostics") {
                    tr("dev.kind_diag")
                } else {
                    tr("dev.kind_normal")
                };
                ui.label(egui::RichText::new(&b.name).monospace());
                ui.label(theme::dim(kind).size(12.5));
                if b.is_current {
                    theme::badge(ui, tr("updates.active"), true);
                } else if ui
                    .add_enabled(!running, egui::Button::new(tr("updates.use")))
                    .clicked()
                {
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
            ui.add(
                egui::TextEdit::singleline(&mut ops.cfg.own_engine)
                    .desired_width((ui.available_width() - 330.0).max(120.0))
                    .hint_text(tr("dev.own_hint")),
            );
            #[cfg(windows)]
            if ui.button(tr("dev.browse")).clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("victory.exe", &["exe"])
                    .pick_file()
                {
                    ops.cfg.own_engine = p.display().to_string();
                }
            }
            if ui
                .add_enabled(
                    !ops.cfg.own_engine.is_empty(),
                    egui::Button::new(tr("dev.use_installed")),
                )
                .clicked()
            {
                ops.cfg.own_engine.clear();
            }
        });
        if !ops.cfg.own_engine.is_empty() && !Path::new(&ops.cfg.own_engine).is_file() {
            ui.label(theme::warn_text(ui, tr("dev.own_missing")));
        }
    });
}

fn profiler_card(ui: &mut egui::Ui, ops: &mut OpsState) {
    use super::profile::{Schedule, Window};
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.profile_schedule")));
        ui.label(theme::dim(tr("dev.profiler_help")).size(12.5));
        let schedule = &mut ops.cfg.profiler;
        ui.checkbox(&mut schedule.enabled, tr("dev.profile_enable"));
        ui.label(theme::dim(tr("dev.profile_windows_help")).size(12.5));
        let mut removal = None;
        for (index, window) in schedule.windows.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("dev.p_from"));
                    ui.add(
                        egui::DragValue::new(&mut window.from)
                            .range(0.0..=86_400.0)
                            .speed(0.25)
                            .suffix(" s"),
                    );
                    ui.label(tr("dev.p_to"));
                    ui.add(
                        egui::DragValue::new(&mut window.to)
                            .range(0.0..=86_400.0)
                            .speed(0.25)
                            .suffix(" s"),
                    );
                    if ui.button(tr("dev.profile_remove")).clicked() {
                        removal = Some(index);
                    }
                });
            });
        }
        if let Some(index) = removal {
            schedule.windows.remove(index);
        }
        if ui
            .add_enabled(
                schedule.windows.len() < 32,
                egui::Button::new(tr("dev.profile_add")),
            )
            .clicked()
        {
            let from = schedule.windows.iter().map(|w| w.to).fold(0.0, f64::max);
            schedule.windows.push(Window {
                from,
                to: from + 20.0,
            });
        }
        egui::Grid::new("profile-config")
            .num_columns(2)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                for (key, value, range, help) in [
                    (
                        "dev.profile_interval",
                        &mut schedule.sample_ms,
                        1..=50,
                        "dev.profile_interval_help",
                    ),
                    (
                        "dev.profile_threads",
                        &mut schedule.threads,
                        4..=28,
                        "dev.profile_threads_help",
                    ),
                    (
                        "dev.profile_depth",
                        &mut schedule.depth,
                        1..=40,
                        "dev.profile_depth_help",
                    ),
                ] {
                    ui.label(tr(key)).on_hover_text(tr(help));
                    ui.add(egui::DragValue::new(value).range(range))
                        .on_hover_text(tr(help));
                    ui.end_row();
                }
                ui.label(tr("dev.profile_filter"))
                    .on_hover_text(tr("dev.profile_filter_help"));
                ui.text_edit_singleline(&mut schedule.filter)
                    .on_hover_text(tr("dev.profile_filter_help"));
                ui.end_row();
                ui.label(tr("dev.p_file"))
                    .on_hover_text(tr("dev.profile_folder_help"));
                ui.add(
                    egui::TextEdit::singleline(&mut schedule.folder)
                        .hint_text(tr("dev.profile_folder_default")),
                )
                .on_hover_text(tr("dev.profile_folder_help"));
                ui.end_row();
            });
        ui.checkbox(&mut schedule.symbolize, tr("dev.profile_symbols"))
            .on_hover_text(tr("dev.profile_symbols_help"));
        if let Err(error) = profile::launch_flags(schedule, &paths::data_dir(), 0) {
            ui.label(theme::warn_text(ui, error.to_string()));
        }
        if ui.button(tr("monitor.reset")).clicked() {
            *schedule = Schedule::default();
        }
    });
}

fn profile_overview(ui: &mut egui::Ui, ops: &mut OpsState) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.profile_overview")));
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("dev.profile_latest")).clicked() {
                let folder = ops.cfg.profiler.folder(&paths::data_dir());
                set_profile(&mut ops.dev, profile::latest(&folder));
            }
            #[cfg(windows)]
            if ui.button(tr("dev.profile_open")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr("dev.profiler"), &["txt", "profile"])
                    .pick_file()
                {
                    set_profile(&mut ops.dev, Ok(path));
                }
            }
            if let Some(path) = &ops.dev.profile_path {
                ui.label(theme::dim(
                    path.file_name().unwrap_or_default().to_string_lossy(),
                ));
            }
        });
        if let Some(note) = &ops.dev.profile_note {
            ui.label(theme::warn_text(ui, note));
        }
        let Some(data) = &ops.dev.profile else { return };
        use super::profile::{Group, View};
        let view = &mut ops.cfg.profile_view;
        ui.horizontal_wrapped(|ui| {
            for (group, text) in [
                (Group::Thread, "dev.profile_groups_threads"),
                (Group::Function, "dev.profile_groups_functions"),
                (Group::Module, "dev.profile_groups_modules"),
            ] {
                ui.selectable_value(&mut view.group, group, tr(text));
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut ops.dev.profile_search)
                    .desired_width(150.0)
                    .hint_text(tr("dev.profile_search")),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("dev.profile_top"));
            ui.add(egui::DragValue::new(&mut view.top).range(1..=250))
                .on_hover_text(tr("dev.profile_top_help"));
            ui.label(tr("dev.profile_min"));
            ui.add(
                egui::DragValue::new(&mut view.minimum_percent)
                    .range(0.0..=100.0)
                    .suffix("%")
                    .speed(0.1),
            )
            .on_hover_text(tr("dev.profile_min_help"));
            ui.checkbox(&mut view.merge_threads, tr("dev.profile_merge"))
                .on_hover_text(tr("dev.profile_merge_help"));
            if ui.button(tr("monitor.reset")).clicked() {
                *view = View::default();
                ops.dev.profile_thread = None;
                ops.dev.profile_search.clear();
            }
        });
        egui::ComboBox::from_id_salt("profile-thread")
            .selected_text(
                ops.dev
                    .profile_thread
                    .and_then(|id| data.threads.get(&id))
                    .map_or_else(
                        || tr("dev.profile_all_threads").to_owned(),
                        |t| format!("{} [#{}]", t.name, t.id),
                    ),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut ops.dev.profile_thread,
                    None,
                    tr("dev.profile_all_threads"),
                );
                for t in data.threads.values().filter(|t| t.attempted > 0) {
                    ui.selectable_value(
                        &mut ops.dev.profile_thread,
                        Some(t.id),
                        format!("{} [#{}]", t.name, t.id),
                    );
                }
            });
        let rows =
            match profile::overview(data, view, ops.dev.profile_thread, &ops.dev.profile_search) {
                Ok(rows) => rows,
                Err(error) => {
                    ui.label(theme::warn_text(ui, error.to_string()));
                    return;
                }
            };
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("dev.profile_copy")).clicked() {
                match profile::as_text(&rows) {
                    Ok(text) => ui.ctx().copy_text(text),
                    Err(error) => ops.dev.profile_note = Some(error.to_string()),
                }
            }
            #[cfg(windows)]
            if ui.button(tr("monitor.export")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("profile-overview.csv")
                    .save_file()
                {
                    ops.dev.profile_note = profile::export_csv(&path, &rows)
                        .err()
                        .map(|e| e.to_string());
                }
            }
            ui.label(theme::dim(trf(
                "dev.profile_rounds",
                &[("n", &data.rounds.to_string())],
            )));
        });
        if rows.is_empty() {
            ui.label(theme::dim(tr("dev.profile_empty")));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("profile-table")
                .max_height(286.0)
                .show(ui, |ui| {
                    egui::Grid::new("profile-samples")
                        .num_columns(3)
                        .striped(true)
                        .spacing([16.0, 5.0])
                        .show(ui, |ui| {
                            for name in [
                                "dev.profile_name",
                                "dev.profile_self",
                                "dev.profile_inclusive",
                            ] {
                                ui.label(theme::strong(tr(name)));
                            }
                            ui.end_row();
                            for row in &rows {
                                ui.add(egui::Label::new(&row.name).truncate())
                                    .on_hover_text(&row.name);
                                ui.monospace(format!("{:.1}%", row.self_percent))
                                    .on_hover_text(row.self_samples.to_string());
                                ui.monospace(format!("{:.1}%", row.inclusive_percent))
                                    .on_hover_text(row.inclusive_samples.to_string());
                                ui.end_row();
                            }
                        });
                });
        }
    });
}

fn set_profile(dev: &mut DevState, path: anyhow::Result<PathBuf>) {
    match path.and_then(|path| Ok((profile::open(&path)?, path))) {
        Ok((profile, path)) => {
            dev.profile = Some(profile);
            dev.profile_path = Some(path);
            dev.profile_thread = None;
            dev.profile_note = None;
        }
        Err(error) => dev.profile_note = Some(error.to_string()),
    }
}

fn log_card(ui: &mut egui::Ui, ops: &mut OpsState, shared: &Shared) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("dev.log")));
        ui.horizontal(|ui| {
            if ui.button(tr("dev.log_load")).clicked() {
                ops.dev.load_log(shared);
            }
            if ui
                .add_enabled(
                    ops.dev.log_path.is_some(),
                    egui::Button::new(tr("dev.log_check")),
                )
                .clicked()
            {
                let msg = match ops.dev.grown() {
                    Some(true) => tr("dev.log_changed"),
                    Some(false) => tr("dev.log_same"),
                    None => tr("dev.no_log"),
                };
                ops.dev.log_note = Some(msg.to_string());
            }
            if ui
                .add_enabled(
                    !ops.dev.log_text.is_empty(),
                    egui::Button::new(tr("dev.copy")),
                )
                .clicked()
            {
                ui.ctx().copy_text(ops.dev.log_text.clone());
            }
            ui.add(
                egui::TextEdit::singleline(&mut ops.dev.search)
                    .hint_text(tr("dev.search"))
                    .desired_width(200.0),
            );
        });
        if let Some(p) = &ops.dev.log_path {
            ui.label(theme::dim(p.display().to_string()).size(12.5));
        }
        if let Some(n) = &ops.dev.log_note {
            ui.label(theme::dim(n.clone()).size(12.5));
        }
        let needle = ops.dev.search.to_lowercase();
        let lines: Vec<&str> = ops
            .dev
            .log_text
            .lines()
            .filter(|l| needle.is_empty() || l.to_lowercase().contains(&needle))
            .collect();
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(egui::Margin::same(8))
            .corner_radius(6)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .auto_shrink([false, true])
                    .stick_to_bottom(true)
                    .show_rows(ui, 15.0, lines.len(), |ui, range| {
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
            } else if ui
                .add_enabled(!running, egui::Button::new(tr("dev.cache_clear")))
                .clicked()
            {
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
            ui.label(
                egui::RichText::new(format!("{k}={v}"))
                    .monospace()
                    .size(12.5),
            );
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
        std::fs::write(
            &p,
            (0..5000).map(|i| format!("line {i}\n")).collect::<String>(),
        )
        .unwrap();
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
        for n in [
            "run-20260101-120000.log",
            "run-20260102-080000.log",
            "other.txt",
        ] {
            std::fs::write(dir.path().join(n), "x").unwrap();
        }
        assert_eq!(
            newest_log(dir.path()).unwrap().file_name().unwrap(),
            "run-20260102-080000.log"
        );
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
}
