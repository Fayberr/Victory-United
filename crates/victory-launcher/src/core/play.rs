use super::{
    runner::{format_duration, remaining, RunState},
    widgets::{kv, notice, open_folder, page_title, pick_folder},
    CoreState,
};
use crate::{app::Shared, i18n::{tr, trf}, theme};
use eframe::egui;

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    let title = core.game.as_ref().map(|g| g.title.clone()).unwrap_or_else(|| tr("play.no_game").to_string());
    page_title(ui, &title, core.game.as_ref().map(|g| g.folder.to_string_lossy().into_owned()).as_deref().unwrap_or(""));
    let wide = ui.available_width() > 720.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let right = if wide { 290.0 } else { 0.0 };
        let left = ui.available_width() - right - if wide { 18.0 } else { 0.0 };
        ui.vertical(|ui| {
            ui.set_width(left);
            play_card(core, ui);
            if let Some(warning) = &core.runner.backup_warning {
                ui.add_space(10.0);
                notice(ui, &trf("saves.warning", &[("reason", warning)]), false);
            }
            if let Some(n) = core.notice.clone() {
                ui.add_space(10.0);
                notice(ui, &n, true);
            }
        });
        if wide {
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.set_width(right);
                machine_card(core, ui);
            });
        }
    });
}

fn play_card(core: &mut CoreState, ui: &mut egui::Ui) {
    let state = core.runner.state.clone();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match state {
            RunState::Warming { done, total, since } => warmup(core, ui, done, total, since.elapsed()),
            RunState::Starting => {
                ui.label(theme::strong(tr("play.starting")).size(18.0));
                ui.add_space(8.0);
                stop_button(core, ui);
            }
            RunState::Running { pid, since, .. } => {
                ui.label(theme::strong(tr("play.running")).size(18.0));
                ui.label(theme::dim(trf("play.running_detail", &[("pid", &pid.to_string()), ("time", &format_duration(since.elapsed()))])));
                ui.add_space(10.0);
                stop_button(core, ui);
            }
            RunState::Ended(s) => {
                ended(core, ui, &s);
            }
            RunState::Idle => idle(core, ui),
        }
    });
    if matches!(core.runner.state, RunState::Idle | RunState::Ended(_)) {
        ui.add_space(12.0);
        preset_card(core, ui);
    }
}

fn idle(core: &mut CoreState, ui: &mut egui::Ui) {
    let engine = core.engine_exe();
    let ready = core.game.is_some() && engine.is_some();
    ui.add_enabled_ui(ready, |ui| {
        if theme::primary_button(ui, tr("play.play")).clicked() {
            core.play();
        }
    });
    ui.add_space(8.0);
    if core.game.is_none() {
        ui.label(theme::dim(tr("play.need_game")));
        if ui.button(tr("play.pick_game")).clicked() {
            if let Some(g) = pick_folder().and_then(|p| super::game::from_pick(&p)) {
                core.set_game(g);
            } else {
                core.notice = Some(tr("play.not_a_game").to_string());
            }
        }
    } else if engine.is_none() {
        ui.label(theme::warn_text(ui, tr("play.need_engine")));
    } else {
        ui.label(theme::dim(shader_status(core)));
    }
}

fn shader_status(core: &CoreState) -> String {
    if core.ui.demo {
        return tr("play.shaders_ready").to_string();
    }
    let Some(plan) = core.plan() else { return String::new() };
    if super::runner::shader_entries(&plan.cache_dir, super::runner::COLD_CACHE_ENTRIES) >= super::runner::COLD_CACHE_ENTRIES {
        tr("play.shaders_ready").to_string()
    } else {
        tr("play.shaders_cold").to_string()
    }
}

fn warmup(core: &mut CoreState, ui: &mut egui::Ui, done: u64, total: u64, elapsed: std::time::Duration) {
    ui.label(theme::strong(tr("play.warm_title")).size(18.0));
    ui.add_space(4.0);
    ui.label(tr("play.warm_text"));
    ui.add_space(14.0);
    let frac = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
    ui.add(egui::ProgressBar::new(frac).desired_height(14.0).fill(theme::palette(ui.visuals()).chart[2]).corner_radius(egui::CornerRadius::same(7)));
    ui.add_space(4.0);
    let eta = remaining(done, total, elapsed).map(|d| trf("play.warm_left", &[("time", &format_duration(d))])).unwrap_or_else(|| tr("play.warm_estimating").to_string());
    let count = if total > 0 { trf("play.warm_count", &[("done", &mib(done)), ("total", &mib(total))]) } else { tr("play.warm_counting").to_string() };
    ui.horizontal(|ui| {
        ui.label(theme::dim(count));
        ui.label(theme::dim("|"));
        ui.label(theme::dim(eta));
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui.button(tr("play.skip")).clicked() {
            core.runner.skip_warmup();
        }
        if ui.button(tr("play.cancel")).clicked() {
            core.runner.stop();
        }
    });
}

fn mib(bytes: u64) -> String {
    format!("{} MB", bytes >> 20)
}

fn stop_button(core: &mut CoreState, ui: &mut egui::Ui) {
    if ui.add(egui::Button::new(tr("play.stop")).min_size(egui::vec2(120.0, 34.0))).clicked() {
        core.runner.stop();
    }
}

fn ended(core: &mut CoreState, ui: &mut egui::Ui, s: &super::runner::Summary) {
    if s.cancelled && s.log.is_none() {
        ui.label(tr("play.cancelled"));
    } else if let Some(e) = &s.error {
        ui.label(theme::error_text(ui, e));
    } else {
        let how = match s.code {
            Some(0) => tr("play.exit_ok").to_string(),
            Some(c) => trf("play.exit_code", &[("code", &c.to_string())]),
            None => tr("play.exit_unknown").to_string(),
        };
        ui.label(theme::strong(how).size(16.0));
        ui.label(theme::dim(trf("play.ran_for", &[("time", &format_duration(std::time::Duration::from_secs(s.secs)))])));
    }
    if let Some(log) = &s.log {
        ui.label(theme::dim(trf("play.log_saved", &[("file", &log.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())])));
    }
    if s.quick_exit_with_flags {
        ui.add_space(8.0);
        notice(ui, tr("play.quick_exit"), false);
        ui.add_space(6.0);
        if ui.button(tr("play.start_without_flags")).clicked() {
            core.play_without_flags();
            return;
        }
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if theme::primary_button(ui, tr("play.play_again")).clicked() {
            core.ui.start_without_flags = false;
            core.play();
        }
        if ui.button(tr("play.open_logs")).clicked() {
            open_folder(&crate::paths::logs_dir());
        }
    });
}

fn preset_card(core: &mut CoreState, ui: &mut egui::Ui) {
    let file = super::presets::builtin();
    let (_, preset_id) = core.resolved();
    let auto = core.profile.options.preset == "auto";
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::dim(tr("play.preset")));
        let name = file.preset(&preset_id).map(|p| p.name.clone()).unwrap_or_default();
        let current = if auto { trf("play.preset_auto", &[("name", &name)]) } else { name.clone() };
        let mut changed = false;
        egui::ComboBox::from_id_salt("play_preset").selected_text(current).width(ui.available_width().min(380.0)).show_ui(ui, |ui| {
            if ui.selectable_label(auto, tr("play.preset_automatic")).clicked() {
                core.profile.options.preset = "auto".into();
                changed = true;
            }
            for p in &file.preset {
                if ui.selectable_label(!auto && core.profile.options.preset == p.id, &p.name).clicked() {
                    core.profile.options.preset = p.id.clone();
                    changed = true;
                }
            }
        });
        if let Some(p) = file.preset(&preset_id) {
            ui.add_space(4.0);
            ui.label(theme::dim(&p.description).size(12.5));
        }
        if changed {
            core.save_profile();
        }
    });
}

fn machine_card(core: &CoreState, ui: &mut egui::Ui) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("play.your_pc")).size(15.5));
        ui.add_space(6.0);
        match &core.hw {
            None => {
                ui.label(theme::dim(tr("play.checking")));
            }
            Some(hw) => {
                if !hw.note.is_empty() {
                    ui.label(theme::dim(&hw.note));
                }
                kv(ui, tr("hw.gpu"), &hw.gpu_name);
                kv(ui, tr("hw.vram"), &format!("{:.1} GB", hw.vram_mb as f64 / 1024.0));
                kv(ui, tr("hw.driver"), &hw.driver);
                kv(ui, tr("hw.ram"), &format!("{:.0} GB", hw.ram_mb as f64 / 1024.0));
                kv(ui, tr("hw.cpu"), &format!("{} cores, {} threads", hw.cpu_cores, hw.cpu_threads));
                for w in super::hw::warnings(hw) {
                    ui.add_space(8.0);
                    notice(ui, &w.text, w.severity == super::hw::Severity::Blocker);
                }
            }
        }
    });
}
