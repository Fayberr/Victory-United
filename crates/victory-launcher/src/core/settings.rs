//! Settings tab, two layers. Everyday: preset, console, a few switches, upscaler. Below that the curated flags
//! as typed fields, a marker and a reset for everything changed from the preset, saved profiles, and a
//! preview of the exact environment and command line.
use super::{
    flagreg::{registry, Flag, Kind},
    launch::{self, Resolved, Source},
    presets::builtin,
    widgets::{open_folder, page_scroll, page_title, pick_file, pick_folder, save_file},
    saves, storage,
    store::{AfterPlay, Appearance, Backups, Behavior, ThemeChoice, UpdateSchedule},
    actions::{self, Dirs, SaveAction, StorageAction},
    CoreState,
};
use crate::{app::Shared, i18n::{tr, trf}, paths, theme};
use eframe::egui;
use std::{collections::HashSet, path::PathBuf, sync::mpsc::{self, Receiver}};

#[derive(Default)]
pub struct SettingsUi {
    pub new_name: String,
    pub message: String,
    saves: SavesUi,
    storage: StorageUi,
}

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    page_title(ui, tr("settings.title"), tr("settings.sub"));
    let scroll = core.page_scroll.take();
    page_scroll(ui, scroll, |ui| {
        ui.set_max_width(820.0);
        let (resolved, preset_id) = core.resolved();
        appearance_section(core, ui);
        ui.add_space(12.0);
        behavior_section(core, ui);
        ui.add_space(12.0);
        startup_updates_section(core, ui);
        ui.add_space(12.0);
        backup_section(core, ui);
        ui.add_space(12.0);
        storage_section(core, ui);
        ui.add_space(12.0);
        preset_section(core, ui, &preset_id);
        ui.add_space(12.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(tr("monitor.hardware")).show(ui, |ui| {
                crate::ops::monitor::capabilities(ui, core.hw.as_ref(), core.rendering_adapter);
            });
        });
        ui.add_space(12.0);
        options_section(core, ui);
        ui.add_space(12.0);
        for (group, title) in [("graphics", "settings.g_graphics"), ("memory", "settings.g_memory"), ("threads", "settings.g_threads"), ("debug", "settings.g_debug")] {
            flag_group(core, ui, &resolved, group, tr(title));
            ui.add_space(12.0);
        }
        profiles_section(core, ui);
        ui.add_space(12.0);
        engine_section(core, ui);
        ui.add_space(12.0);
        preview_section(core, ui, &resolved);
        ui.add_space(24.0);
    });
}

fn appearance_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let before = core.settings.appearance;
    let mut appearance = before;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("appearance.title")).size(16.0));
            if ui.add_enabled(appearance != Appearance::default(), egui::Button::new(tr("appearance.reset"))).clicked() {
                appearance = Appearance::default();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("appearance.theme"));
            for (choice, label) in [
                (ThemeChoice::System, "appearance.system"),
                (ThemeChoice::Dark, "appearance.dark"),
                (ThemeChoice::Light, "appearance.light"),
            ] {
                ui.selectable_value(&mut appearance.theme, choice, tr(label));
            }
        });
        ui.label(theme::dim(tr("appearance.theme_help")).size(12.5));
        ui.add(egui::Slider::new(&mut appearance.scale_percent, 80..=150).suffix("%").text(tr("appearance.scale")));
        ui.label(theme::dim(tr("appearance.scale_help")).size(12.5));
    });
    if appearance != before {
        core.settings.appearance = appearance;
        core.save_settings();
        theme::set_appearance(ui.ctx(), &appearance);
    }
}

fn behavior_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let before = core.settings.behavior;
    let behavior = &mut core.settings.behavior;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("behavior.title")).size(16.0));
            if ui.add_enabled(*behavior != Behavior::default(), egui::Button::new(tr("appearance.reset"))).clicked() {
                *behavior = Behavior::default();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("behavior.after"));
            for (action, name) in [
                (AfterPlay::KeepOpen, "behavior.keep"), (AfterPlay::Minimize, "behavior.minimize"),
                (AfterPlay::Close, "behavior.close"),
            ] {
                ui.selectable_value(&mut behavior.after_play, action, tr(name));
            }
        });
        ui.label(theme::dim(tr("behavior.after_help")).size(12.5));
        ui.checkbox(&mut behavior.remember_window, tr("behavior.remember"));
        ui.label(theme::dim(tr("behavior.remember_help")).size(12.5));
        ui.checkbox(&mut behavior.confirm_stop, tr("behavior.confirm_stop"));
        ui.label(theme::dim(tr("behavior.confirm_help")).size(12.5));
    });
    if core.settings.behavior != before {
        if !core.settings.behavior.remember_window {
            core.settings.window = None;
        }
        core.save_settings();
    }
}

fn startup_updates_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let before = core.settings.update_schedule;
    let schedule = &mut core.settings.update_schedule;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("startup_updates.title")).size(16.0));
            if ui.add_enabled(*schedule != UpdateSchedule::default(), egui::Button::new(tr("appearance.reset"))).clicked() {
                *schedule = UpdateSchedule::default();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (choice, name) in [
                (UpdateSchedule::Off, "startup_updates.off"), (UpdateSchedule::Daily, "startup_updates.daily"),
                (UpdateSchedule::AtStartup, "startup_updates.startup"),
            ] {
                ui.selectable_value(schedule, choice, tr(name));
            }
        });
        ui.label(theme::dim(tr("startup_updates.help")).size(12.5));
        if let Some(t) = core.settings.last_update_check {
            ui.label(theme::dim(trf("startup_updates.last", &[("time", &backup_time(t))])).size(12.5));
        }
    });
    if core.settings.update_schedule != before {
        core.save_settings();
    }
}

// ---------------------------------------------------------------- preset

fn preset_section(core: &mut CoreState, ui: &mut egui::Ui, preset_id: &str) {
    let file = builtin();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("settings.preset")).size(16.0));
        ui.add_space(4.0);
        let auto = core.profile.options.preset == "auto";
        let name = file.preset(preset_id).map(|p| p.name.clone()).unwrap_or_default();
        let mut changed = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("settings_preset")
                .selected_text(if auto { format!("{} {}", tr("settings.automatic"), name) } else { name.clone() })
                .width(340.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(auto, tr("settings.automatic")).clicked() {
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
            let any_edit = !core.profile.edits.is_empty() || !core.profile.removed.is_empty();
            if ui.add_enabled(any_edit, egui::Button::new(tr("settings.reset_all"))).clicked() {
                core.profile.edits.clear();
                core.profile.removed.clear();
                changed = true;
            }
        });
        if let Some(p) = file.preset(preset_id) {
            ui.add_space(4.0);
            ui.label(theme::dim(&p.description).size(12.5));
        }
        if changed {
            core.save_profile();
        }
    });
}

// ---------------------------------------------------------------- options

fn options_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let file = builtin();
    let mut changed = false;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("settings.options")).size(16.0));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(theme::dim(tr("settings.console")));
            for c in [launch::Console::SeriesX, launch::Console::SeriesS] {
                if ui.selectable_label(core.profile.options.console == c, c.arg()).clicked() {
                    core.profile.options.console = c;
                    changed = true;
                }
            }
        });
        ui.label(theme::dim(tr("settings.console_help")).size(12.5));
        ui.add_space(6.0);
        for t in &file.toggle {
            let mut on = core.profile.options.toggles.get(&t.id).copied().unwrap_or(t.default);
            if ui.checkbox(&mut on, &t.name).changed() {
                core.profile.options.toggles.insert(t.id.clone(), on);
                changed = true;
            }
            ui.label(theme::dim(&t.description).size(12.5));
        }
        ui.add_space(6.0);
        if ui.checkbox(&mut core.profile.options.safe, tr("settings.safe")).changed() {
            changed = true;
        }
        ui.label(theme::dim(tr("settings.safe_help")).size(12.5));

        ui.add_space(10.0);
        ui.label(theme::strong(tr("settings.upscaling")).size(15.0));
        ui.add_space(4.0);
        let cur = file.upscaler.iter().find(|u| u.id == core.profile.options.upscaler).unwrap_or(&file.upscaler[0]);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("settings_upscaler").selected_text(&cur.name).width(200.0).show_ui(ui, |ui| {
                for u in &file.upscaler {
                    let label = if u.available { u.name.clone() } else { format!("{} ({})", u.name, tr("settings.unavailable")) };
                    let resp = ui.add_enabled(u.available, egui::Button::selectable(core.profile.options.upscaler == u.id, label));
                    if resp.clicked() {
                        core.profile.options.upscaler = u.id.clone();
                        if !u.modes.contains(&core.profile.options.upscaler_mode) {
                            core.profile.options.upscaler_mode = u.modes.first().cloned().unwrap_or_default();
                        }
                        changed = true;
                    }
                }
            });
            if !cur.modes.is_empty() {
                egui::ComboBox::from_id_salt("settings_mode").selected_text(&core.profile.options.upscaler_mode).width(160.0).show_ui(ui, |ui| {
                    for m in &cur.modes {
                        if ui.selectable_label(core.profile.options.upscaler_mode == *m, m).clicked() {
                            core.profile.options.upscaler_mode = m.clone();
                            changed = true;
                        }
                    }
                });
            }
        });
        // What is not available is shown with its reason, straight from the data file.
        for u in file.upscaler.iter().filter(|u| !u.available) {
            ui.label(theme::dim(format!("{}: {}", u.name, u.reason.as_deref().unwrap_or(""))).size(12.5));
            if let Some(note) = &u.files_note {
                files_row(core, ui, u.files_env.as_deref(), note);
            }
        }
        if let (Some(note), true) = (&cur.files_note, cur.available) {
            files_row(core, ui, cur.files_env.as_deref(), note);
        }
        let fg = &file.frame_generation;
        ui.add_space(6.0);
        let usable = fg.available && fg.requires_upscaler.as_deref().is_none_or(|r| r == core.profile.options.upscaler);
        ui.add_enabled_ui(usable, |ui| {
            if ui.checkbox(&mut core.profile.options.frame_generation, tr("settings.framegen")).changed() {
                changed = true;
            }
        });
        let why = if !fg.available { fg.reason.clone().unwrap_or_default() } else { String::new() };
        if !why.is_empty() {
            ui.label(theme::dim(why).size(12.5));
        }
        if let Some(n) = &fg.support_note {
            ui.label(theme::dim(n).size(12.5));
        }
    });
    if changed {
        core.save_profile();
    }
}

/// "Files needed" note with a folder chooser for the variable that points at them.
fn files_row(core: &mut CoreState, ui: &mut egui::Ui, env: Option<&str>, note: &str) {
    ui.label(theme::dim(note).size(12.5));
    let Some(env) = env else { return };
    ui.horizontal(|ui| {
        let cur = core.settings.dirs.get(env).cloned().unwrap_or_default();
        ui.label(theme::dim(if cur.is_empty() { tr("settings.no_folder").to_string() } else { cur }).size(12.5));
        if ui.small_button(tr("settings.choose_folder")).clicked() {
            if let Some(p) = pick_folder() {
                core.settings.dirs.insert(env.to_string(), p.to_string_lossy().into_owned());
                core.save_settings();
            }
        }
        if core.settings.dirs.contains_key(env) && ui.small_button(tr("settings.clear")).clicked() {
            core.settings.dirs.remove(env);
            core.save_settings();
        }
    });
}

// ---------------------------------------------------------------- curated flags

/// Flags that an option control already owns; they are not shown a second time as raw fields.
fn owned_by_options() -> HashSet<String> {
    let f = builtin();
    let mut s: HashSet<String> = HashSet::new();
    for t in &f.toggle {
        s.extend(t.on.keys().cloned());
    }
    for u in &f.upscaler {
        s.extend(u.env.keys().cloned());
    }
    s.extend(f.frame_generation.env.keys().cloned());
    s
}

fn flag_group(core: &mut CoreState, ui: &mut egui::Ui, resolved: &Resolved, group: &str, title: &str) {
    let owned = owned_by_options();
    let flags: Vec<&Flag> = registry().group(group).filter(|f| !owned.contains(&f.name)).collect();
    if flags.is_empty() {
        return;
    }
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(title).size(16.0));
        ui.add_space(4.0);
        for flag in flags {
            field_row(core, ui, resolved, flag);
            ui.add_space(4.0);
        }
    });
}

fn is_changed(core: &CoreState, name: &str) -> bool {
    core.profile.edits.contains_key(name) || core.profile.removed.iter().any(|r| r == name)
}

/// Stores one edit in the profile. `None` resets the field to what the preset gives.
pub fn set_edit(core: &mut CoreState, flag: &Flag, value: Option<String>) {
    core.profile.set_edit(&flag.name, value);
    core.save_profile();
}

/// Turns a switch on or off in the profile (see `Profile::set_switch`).
pub fn set_switch(core: &mut CoreState, flag: &Flag, on: bool) {
    core.profile.set_switch(flag, on);
    core.save_profile();
}

fn switch_is_on(value: Option<&str>) -> bool {
    value.is_some_and(|v| v != "0" && !v.is_empty())
}

fn field_row(core: &mut CoreState, ui: &mut egui::Ui, resolved: &Resolved, flag: &Flag) {
    let value = resolved.env.get(&flag.name).cloned();
    let changed = is_changed(core, &flag.name);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(430.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(flag.name.trim_start_matches("VICTORY_")).monospace().color(ui.visuals().strong_text_color()));
                if changed {
                    theme::badge(ui, tr("settings.changed"), true);
                }
            });
            ui.label(theme::dim(&flag.doc).size(12.5));
        });
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                edit_control(core, ui, flag, value.as_deref());
                if changed && ui.small_button(tr("settings.reset")).clicked() {
                    set_edit(core, flag, None);
                }
            });
        });
    });
}

fn edit_control(core: &mut CoreState, ui: &mut egui::Ui, flag: &Flag, value: Option<&str>) {
    match flag.kind {
        Kind::Switch => {
            let mut on = switch_is_on(value);
            if ui.checkbox(&mut on, "").changed() {
                set_switch(core, flag, on);
            }
        }
        Kind::Number => {
            let (lo, hi) = (flag.min.unwrap_or(0.0), flag.max.unwrap_or(1_000_000.0));
            let shown = value.and_then(|v| v.parse::<f64>().ok()).or_else(|| flag.default.parse().ok());
            let mut n = shown.unwrap_or(lo);
            let is_float = flag.max.is_some_and(|m| m <= 10.0) || shown.is_some_and(|v| v.fract() != 0.0);
            let mut drag = egui::DragValue::new(&mut n).range(lo..=hi).speed(if is_float { 0.05 } else { 1.0 });
            if is_float {
                drag = drag.max_decimals(2);
            }
            if ui.add(drag).changed() {
                let text = if is_float { format!("{n}") } else { format!("{}", n.round() as i64) };
                set_edit(core, flag, Some(text));
            }
            if value.is_none() && flag.default.is_empty() {
                ui.label(theme::dim(tr("settings.default")).size(12.0));
            }
        }
        Kind::Choice => {
            let current = value.unwrap_or("").to_string();
            let text = if current.is_empty() { tr("settings.default").to_string() } else { current.clone() };
            egui::ComboBox::from_id_salt(&flag.name).selected_text(text).width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(current.is_empty(), tr("settings.default")).clicked() {
                    set_edit(core, flag, None);
                }
                for c in &flag.choices {
                    if ui.selectable_label(&current == c, c).clicked() {
                        set_edit(core, flag, Some(c.clone()));
                    }
                }
            });
        }
        Kind::Text => {
            let mut text = value.unwrap_or("").to_string();
            let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(150.0).hint_text(tr("settings.default")));
            if resp.changed() {
                let t = text.trim().to_string();
                set_edit(core, flag, (!t.is_empty()).then_some(t));
            }
        }
    }
}

// ---------------------------------------------------------------- profiles

fn profiles_section(core: &mut CoreState, ui: &mut egui::Ui) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("settings.profiles")).size(16.0));
        ui.label(theme::dim(tr("settings.profiles_help")).size(12.5));
        ui.add_space(6.0);
        let names = launch::profile_names(&core.profiles, &core.profile.name);
        ui.horizontal(|ui| {
            ui.label(theme::dim(tr("settings.active")));
            egui::ComboBox::from_id_salt("profile_pick").selected_text(&core.profile.name).width(220.0).show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(*n == core.profile.name, n).clicked() && *n != core.profile.name {
                        if let Some(p) = core.profiles.iter().find(|p| &p.name == n).cloned() {
                            core.profile = p;
                            core.settings.active_profile = n.clone();
                            core.save_settings();
                        }
                    }
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut core.ui.settings.new_name).desired_width(200.0).hint_text(tr("settings.name_hint")));
            let name = core.ui.settings.new_name.trim().to_string();
            let ok = !name.is_empty() && !names.contains(&name);
            if ui.add_enabled(ok, egui::Button::new(tr("settings.new"))).clicked() {
                core.add_profile(&name, false);
            }
            if ui.add_enabled(ok, egui::Button::new(tr("settings.duplicate"))).clicked() {
                core.add_profile(&name, true);
            }
            if ui.add_enabled(ok, egui::Button::new(tr("settings.rename"))).clicked() {
                core.rename_profile(&name);
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(names.len() > 1, egui::Button::new(tr("settings.delete"))).clicked() {
                core.delete_active_profile();
            }
            if ui.button(tr("settings.export")).clicked() {
                if let Some(path) = save_file(&format!("{}.toml", launch::file_safe_name(&core.profile.name)), ("Profile", &["toml"])) {
                    let text = toml::to_string_pretty(&core.profile).unwrap_or_default();
                    core.ui.settings.message = match std::fs::write(&path, text) {
                        Ok(()) => tr("settings.exported").to_string(),
                        Err(e) => e.to_string(),
                    };
                }
            }
            if ui.button(tr("settings.import")).clicked() {
                if let Some(path) = pick_file(("Profile", &["toml"])) {
                    match launch::load_profile_file(&path) {
                        Some(mut p) => {
                            if p.name.is_empty() || p.name == "Default" {
                                p.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into());
                            }
                            core.profile = p;
                            core.save_profile();
                            core.ui.settings.message = tr("settings.imported").to_string();
                        }
                        None => core.ui.settings.message = tr("settings.import_failed").to_string(),
                    }
                }
            }
            if ui.button(tr("settings.open_profiles")).clicked() {
                open_folder(&crate::paths::profiles_dir());
            }
        });
        if !core.ui.settings.message.is_empty() {
            ui.label(theme::dim(&core.ui.settings.message).size(12.5));
        }
    });
}

// ---------------------------------------------------------------- engine, game

fn engine_section(core: &mut CoreState, ui: &mut egui::Ui) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("settings.engine")).size(16.0));
        ui.add_space(4.0);
        let exe = core.engine_exe();
        let own = core.settings.engine_override.is_some();
        match &exe {
            Some(p) => {
                ui.label(if own { tr("settings.engine_own") } else { tr("settings.engine_installed") });
                ui.label(theme::dim(p.to_string_lossy()).size(12.5));
            }
            None => {
                ui.label(theme::warn_text(ui, tr("settings.engine_none")));
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button(tr("settings.engine_choose")).clicked() {
                if let Some(p) = pick_file(("victory.exe", &["exe"])) {
                    core.settings.engine_override = Some(p);
                    core.save_settings();
                }
            }
            if own && ui.button(tr("settings.engine_use_installed")).clicked() {
                core.settings.engine_override = None;
                core.save_settings();
            }
        });
        ui.add_space(8.0);
        ui.label(theme::strong(tr("settings.game")).size(15.0));
        match &core.game {
            Some(g) => {
                ui.label(theme::dim(g.exe.to_string_lossy()).size(12.5));
            }
            None => {
                ui.label(theme::warn_text(ui, tr("play.need_game")));
            }
        }
        if ui.button(tr("settings.game_change")).clicked() {
            if let Some(p) = pick_folder() {
                match super::game::from_pick(&p) {
                    Some(g) => core.set_game(g),
                    None => core.notice = Some(tr("play.not_a_game").to_string()),
                }
            }
        }
    });
}

// ---------------------------------------------------------------- preview

pub fn preview_text(core: &CoreState, resolved: &Resolved) -> String {
    let mut out = String::new();
    match core.plan() {
        Some(plan) => {
            out.push_str(&format!("Command line:\n{}\n\nEnvironment:\n", plan.command_line()));
        }
        None => out.push_str("Environment:\n"),
    }
    for (k, v) in &resolved.env {
        out.push_str(&format!("{k}={v}\n"));
    }
    out
}

fn preview_section(core: &mut CoreState, ui: &mut egui::Ui, resolved: &Resolved) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::CollapsingHeader::new(theme::strong(tr("settings.final")).size(16.0)).default_open(core.ui.demo).show(ui, |ui| {
            ui.label(theme::dim(tr("settings.final_help")).size(12.5));
            ui.add_space(4.0);
            if let Some(plan) = core.plan() {
                ui.label(egui::RichText::new(plan.command_line()).monospace().size(12.5));
            }
            ui.add_space(4.0);
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (k, v) in &resolved.env {
                    let src = resolved.source.get(k).copied().unwrap_or(Source::Preset);
                    ui.label(egui::RichText::new(format!("{k}={v}")).monospace().size(12.5).color(if src == Source::Preset { ui.visuals().weak_text_color() } else { ui.visuals().strong_text_color() }))
                        .on_hover_text(src.label());
                }
            });
            ui.add_space(4.0);
            if ui.button(tr("settings.copy")).clicked() {
                let text = preview_text(core, resolved);
                ui.ctx().copy_text(text);
            }
        });
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_are_removed_unless_the_engine_wants_zero() {
        let mut core = CoreState::demo();
        let reg = registry();
        // CARD_RESERVE_BUDGETS is a presence style switch: on is 1, off removes it from the environment.
        let flag = reg.get("VICTORY_CARD_RESERVE_BUDGETS").unwrap();
        set_switch(&mut core, flag, true);
        assert_eq!(core.resolved().0.env["VICTORY_CARD_RESERVE_BUDGETS"], "1");
        set_switch(&mut core, flag, false);
        assert!(!core.resolved().0.env.contains_key("VICTORY_CARD_RESERVE_BUDGETS"));
        assert!(!core.profile.edits.values().any(|v| v == "0"), "never written as 0");
        // UNDER_WIDE is read as "0 means off", so off writes 0.
        let wide = reg.get("VICTORY_UNDER_WIDE").unwrap();
        set_switch(&mut core, wide, false);
        assert_eq!(core.resolved().0.env["VICTORY_UNDER_WIDE"], "0");
        set_edit(&mut core, wide, None);
        assert!(!core.is_changed_for_test("VICTORY_UNDER_WIDE"));
    }

    #[test]
    fn an_edit_marks_the_field_and_reset_clears_it() {
        let mut core = CoreState::demo();
        let flag = registry().get("VICTORY_TEXTURE_ROOM_PERCENT").unwrap();
        set_edit(&mut core, flag, Some("70".into()));
        assert!(is_changed(&core, &flag.name));
        assert_eq!(core.resolved().0.env["VICTORY_TEXTURE_ROOM_PERCENT"], "70");
        set_edit(&mut core, flag, None);
        assert!(!is_changed(&core, &flag.name));
    }

    #[test]
    fn option_controlled_flags_are_not_listed_twice() {
        let owned = owned_by_options();
        assert!(owned.contains("VICTORY_FULLSCREEN") && owned.contains("VICTORY_FSR_4K"));
        // every group on the Settings tab has at least one raw field left
        for g in ["graphics", "memory", "threads", "debug"] {
            assert!(registry().group(g).any(|f| !owned.contains(&f.name)), "{g}");
        }
    }
}

type SavesResult = anyhow::Result<(Vec<saves::Snapshot>, String)>;

#[derive(Default)]
struct SavesUi {
    root: Option<PathBuf>,
    snapshots: Vec<saves::Snapshot>,
    pending: Option<Receiver<SavesResult>>,
    label: String,
    selected: Option<String>,
    rename: String,
    revision: u64,
    confirm: Option<SaveAction>,
    note: Option<(bool, String)>,
}

fn start_saves_job(core: &mut CoreState, action: Option<SaveAction>) {
    let root = paths::backups_root(&core.settings);
    let game = core.game.as_ref().map(|g| g.folder.clone());
    let engine = core.engine_exe();
    let policy = core.settings.backups.retention;
    let ctx = core.ctx.clone();
    let (tx, rx) = mpsc::channel();
    let state = &mut core.ui.settings.saves;
    state.root = Some(root.clone());
    state.revision = core.runner.backup_revision;
    state.pending = Some(rx);
    state.note = None;
    std::thread::spawn(move || {
        let result = (|| {
            let note = if let Some(action) = action {
                let note = actions::save_action(&Dirs::real(), &root, game.as_deref(), engine.as_deref(), policy, action)?;
                tr(note).into()
            } else {
                String::new()
            };
            Ok((saves::list(&root)?, note))
        })();
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

fn backup_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let before = core.settings.backups.clone();
    let mut cfg = before.clone();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("saves.settings")).size(16.0));
            if ui.add_enabled(cfg != Backups::default(), egui::Button::new(tr("appearance.reset"))).clicked() {
                cfg = Backups::default();
            }
        });
        ui.checkbox(&mut cfg.before_launch, tr("saves.before"));
        ui.label(theme::dim(tr("saves.before_help")).size(12.5));
        ui.checkbox(&mut cfg.after_exit, tr("saves.after"));
        ui.label(theme::dim(tr("saves.after_help")).size(12.5));
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::DragValue::new(&mut cfg.retention.keep_recent).range(0..=1000).prefix(tr("saves.recent")));
            ui.add(egui::DragValue::new(&mut cfg.retention.keep_days).range(0..=3650).prefix(tr("saves.days")));
            ui.add(egui::DragValue::new(&mut cfg.retention.max_gib).range(0..=10_000).prefix(tr("saves.cap")).suffix(" GiB"));
        });
        ui.label(theme::dim(tr("saves.retention_help")).size(12.5));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("saves.folder"));
            if ui.button(tr("settings.choose_folder")).clicked() && !core.ui.demo {
                if let Some(p) = pick_folder() {
                    let settings = paths::settings_file();
                    let engine = paths::engine_root();
                    let game = core.game.as_ref().map(|g| g.folder.as_path());
                    let ctx = storage::MoveContext { settings: &settings, engine: &engine, game };
                    let valid = storage::validate_backup_root(&p, &ctx, &core.settings);
                    match valid {
                        Ok(p) => cfg.root = Some(p),
                        Err(error) => core.ui.settings.message = format!("{error:#}"),
                    }
                }
            }
        });
        let root = if core.ui.demo { PathBuf::from("C:/Users/Player/AppData/Roaming/Victory-United/backups") } else { paths::backups_root(&core.settings) };
        ui.label(theme::dim(root.to_string_lossy()).size(12.5));
        ui.label(theme::dim(tr("saves.folder_help")).size(12.5));
        if !core.ui.settings.message.is_empty() {
            ui.label(theme::warn_text(ui, &core.ui.settings.message).size(12.5));
        }
    });
    if cfg != before {
        core.settings.backups = cfg;
        core.save_settings();
    }
}

pub fn show_saves(core: &mut CoreState, ui: &mut egui::Ui) {
    page_title(ui, tr("saves.title"), tr("saves.subtitle"));
    let scroll = core.page_scroll.take();
    let root = paths::backups_root(&core.settings);
    if core.ui.demo {
        if core.ui.settings.saves.snapshots.is_empty() {
            core.ui.settings.saves.snapshots = demo_saves();
            if let Some(snap) = core.ui.settings.saves.snapshots.first() {
                core.ui.settings.saves.selected = Some(snap.id.clone());
                core.ui.settings.saves.rename = snap.manifest.label.clone();
            }
        }
    } else {
        let state = &mut core.ui.settings.saves;
        if let Some(result) = state.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            state.pending = None;
            match result {
                Ok((snapshots, note)) => {
                    state.snapshots = snapshots;
                    state.note = (!note.is_empty()).then_some((false, note));
                }
                Err(error) => state.note = Some((true, format!("{error:#}"))),
            }
        }
        if state.pending.is_none() && (state.root.as_ref() != Some(&root) || state.revision != core.runner.backup_revision) {
            if state.root.as_ref() != Some(&root) {
                state.snapshots.clear();
                state.selected = None;
            }
            state.confirm = None;
            start_saves_job(core, None);
        }
    }

    let mut refresh = false;
    let mut action = None;
    let busy = (core.runner.busy() || core.ui.settings.storage.pending()) && !core.ui.demo;
    let state = &mut core.ui.settings.saves;
    page_scroll(ui, scroll, |ui| {
        ui.set_max_width(820.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut state.label).hint_text(tr("saves.optional_label")).desired_width(180.0).char_limit(120));
                if ui.add_enabled(!busy && state.pending.is_none(), egui::Button::new(tr("saves.create"))).clicked() {
                    action = Some(SaveAction::Create(state.label.clone()));
                }
                if ui.add_enabled(!busy && state.pending.is_none(), egui::Button::new(tr("saves.import"))).clicked() && !core.ui.demo {
                    action = pick_file(("ZIP", &["zip"])).map(SaveAction::Import);
                }
                if ui.add_enabled(state.pending.is_none(), egui::Button::new(tr("saves.refresh"))).clicked() {
                    refresh = true;
                }
                if ui.button(tr("saves.open")).clicked() && !core.ui.demo {
                    open_folder(&root);
                }
            });
            if busy {
                ui.label(theme::dim(tr("saves.busy")));
            }
            if state.pending.is_some() {
                ui.label(theme::dim(tr("saves.working")));
            }
            if let Some((error, note)) = &state.note {
                ui.label(if *error { theme::error_text(ui, note) } else { theme::dim(note) });
            }
        });
        ui.add_space(12.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(tr("saves.list")).size(16.0));
            if state.snapshots.is_empty() {
                ui.label(theme::dim(tr("saves.empty")));
                return;
            }
            for snap in &state.snapshots {
                let m = &snap.manifest;
                let when = backup_time(m.created);
                let title = if m.label.is_empty() { when } else { format!("{}   |   {when}", m.label) };
                let selected = state.selected.as_ref() == Some(&snap.id);
                if ui.selectable_label(selected, title).clicked() {
                    state.selected = Some(snap.id.clone());
                    state.rename = m.label.clone();
                    state.confirm = None;
                }
                let kind = kind_label(m.kind);
                let short = m.files.first().and_then(|f| f.sha256.get(..10)).unwrap_or_default();
                let build = if m.engine_build.is_empty() { tr("saves.unknown_build") } else { &m.engine_build };
                ui.label(theme::dim(format!("{kind}   |   {}   |   {build}   |   {short}", display_size(m.total_size))).size(12.5));
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_enabled_ui(!busy && state.pending.is_none(), |ui| {
                if let Some(id) = &state.selected {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("saves.restore")).clicked() {
                            state.confirm = Some(SaveAction::Restore(id.clone()));
                        }
                        if ui.button(tr("saves.verify")).clicked() {
                            action = Some(SaveAction::Verify(id.clone()));
                        }
                        if ui.button(tr("saves.export")).clicked() && !core.ui.demo {
                            let file = format!("{id}.zip");
                            action = save_file(&file, ("ZIP", &["zip"])).map(|out| SaveAction::Export(id.clone(), out));
                        }
                        if ui.button(tr("saves.delete")).clicked() {
                            state.confirm = Some(SaveAction::Delete(id.clone()));
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut state.rename).hint_text(tr("saves.optional_label")).desired_width(180.0).char_limit(120));
                        if ui.button(tr("saves.rename")).clicked() {
                            action = Some(SaveAction::Rename(id.clone(), state.rename.clone()));
                        }
                    });
                }
                if ui.button(tr("saves.cleanup")).clicked() {
                    match saves::retention_plan(&state.snapshots, core.settings.backups.retention, storage::now().as_secs()) {
                        Ok(ids) if ids.is_empty() => state.note = Some((false, tr("saves.cleanup_empty").into())),
                        Ok(ids) => state.confirm = Some(SaveAction::Cleanup(ids)),
                        Err(error) => state.note = Some((true, error.to_string())),
                    }
                }
                if let Some(confirm) = &state.confirm {
                    match confirm {
                        SaveAction::Restore(_) => { ui.label(theme::warn_text(ui, tr("saves.confirm_restore"))); }
                        SaveAction::Delete(_) => { ui.label(theme::warn_text(ui, tr("saves.confirm_delete"))); }
                        SaveAction::Cleanup(ids) => {
                            ui.label(theme::warn_text(ui, tr("saves.confirm_cleanup")));
                            for id in ids {
                                ui.label(id);
                            }
                        }
                        _ => {}
                    };
                    ui.horizontal(|ui| {
                        if ui.button(tr("saves.confirm")).clicked() {
                            action = state.confirm.take();
                        }
                        if ui.button(tr("play.cancel")).clicked() {
                            state.confirm = None;
                        }
                    });
                }
            });
        });
    });
    if !core.ui.demo && (refresh || action.is_some()) {
        start_saves_job(core, action);
    }
}

fn kind_label(kind: saves::Kind) -> &'static str {
    tr(match kind {
        saves::Kind::BeforeLaunch => "saves.kind_before",
        saves::Kind::AfterExit => "saves.kind_after",
        saves::Kind::Manual => "saves.kind_manual",
        saves::Kind::BeforeRestore => "saves.kind_restore",
        saves::Kind::BeforeMove => "saves.kind_move",
    })
}

fn backup_time(secs: u64) -> String {
    let stamp = super::runner::timestamp_at(secs);
    match stamp.split_once('-') {
        Some((d, t)) if d.len() == 8 && t.len() == 6 => format!("{}-{}-{} {}:{}:{} UTC", &d[..4], &d[4..6], &d[6..], &t[..2], &t[2..4], &t[4..]),
        _ => stamp,
    }
}

fn display_size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.2} GiB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn demo_saves() -> Vec<saves::Snapshot> {
    let sample = |id: &str, created, kind, label: &str, total_size, hash: &str| saves::Snapshot {
        id: id.into(),
        manifest: saves::Manifest {
            schema: 1, created, kind, engine_build: "a1b2c3d4-release".into(), launcher_version: env!("CARGO_PKG_VERSION").into(),
            label: label.into(), total_size,
            files: vec![saves::SavedFile { path: "DEADBEEF/saves/profile.bin".into(), size: total_size, modified_ns: None, sha256: hash.into() }],
        },
    };
    vec![
        sample("20261127-184600-example-manual", 1_795_805_160, saves::Kind::Manual, "Before the next build", 3_670_016, "71fc7622ac734d60"),
        sample("20261127-182400-example-after-exit", 1_795_803_840, saves::Kind::AfterExit, "", 3_670_016, "9fc250ac4673c8b4"),
        sample("20261126-203000-example-before-launch", 1_795_725_000, saves::Kind::BeforeLaunch, "", 3_145_728, "ee71033d81ad7192"),
    ]
}

use std::{
    sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::{Duration, Instant},
};
use storage::{Location, MoveJournal, MovePhase};

const LOCATIONS: [Location; 2] = [Location::Cache, Location::Data];
type Measurements = [Result<storage::Usage, String>; 2];

#[derive(Default)]
struct StorageUi {
    usage: Option<Measurements>,
    measuring: Option<Receiver<Measurements>>,
    job: Option<Receiver<StorageOutcome>>,
    progress: Arc<MoveProgress>,
    plan: Option<StorageConfirm>,
    journal: Option<MoveJournal>,
    journal_read: bool,
    note: Option<(bool, String)>,
}

impl StorageUi {
    fn pending(&self) -> bool { self.job.is_some() }
}

#[derive(Default)]
struct MoveProgress {
    cancel: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
}

struct StorageOutcome {
    status: anyhow::Result<()>,
    journal: anyhow::Result<Option<MoveJournal>>,
    locations: super::store::StorageSettings,
    message: &'static str,
}

enum StorageConfirm {
    Move { location: Location, target: PathBuf, network: bool },
    Clear { location: Location, phrase: String },
    DeleteOld,
}

/// Drops what the screen cached from disk, so it is read again.
pub fn reload(core: &mut CoreState) {
    core.ui.settings.saves.root = None;
    let storage = &mut core.ui.settings.storage;
    if storage.job.is_none() {
        storage.usage = None;
        storage.journal_read = false;
    }
}

pub fn storage_busy(core: &CoreState) -> bool {
    core.ui.settings.storage.pending()
}

pub fn tick_storage(core: &mut CoreState) {
    let state = &mut core.ui.settings.storage;
    if !state.journal_read {
        state.journal_read = true;
        match storage::read_journal(&paths::move_journal()) {
            Ok(journal) => state.journal = journal,
            Err(error) => state.note = Some((true, format!("{error:#}"))),
        }
    }
    if let Some(result) = state.measuring.as_ref().and_then(|rx| rx.try_recv().ok()) {
        state.usage = Some(result);
        state.measuring = None;
    }
    if let Some(result) = state.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
        state.job = None;
        state.measuring = None;
        state.usage = None;
        state.note = Some(match result.status {
            Ok(()) => (false, tr(result.message).to_string()),
            Err(error) => (true, format!("{error:#}")),
        });
        match result.journal {
            Ok(journal) => state.journal = journal,
            Err(error) => state.note = Some((true, format!("{error:#}"))),
        }
        core.settings.storage = result.locations;
        paths::configure_storage(&core.settings);
        core.ui.settings.saves.root = None;
    }
}

fn start_measurement(core: &mut CoreState) {
    let roots = [paths::cache_root(), paths::game_data_root()];
    let ctx = core.ctx.clone();
    let (tx, rx) = mpsc::channel();
    core.ui.settings.storage.measuring = Some(rx);
    std::thread::spawn(move || {
        let result = roots.map(|path| storage::measure(&path).map_err(|error| format!("{error:#}")));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

fn start_storage_job(core: &mut CoreState, action: StorageAction) {
    let ctx = core.ctx.clone();
    let game = core.game.as_ref().map(|g| g.folder.clone());
    let build = core.engine_exe().and_then(|p| p.parent()?.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
    let (tx, rx) = mpsc::channel();
    let state = &mut core.ui.settings.storage;
    state.job = Some(rx);
    state.note = None;
    state.progress = Arc::new(MoveProgress::default());
    let progress = state.progress.clone();
    std::thread::spawn(move || {
        let dirs = Dirs::real();
        let message = action.done_message();
        let mut last = Instant::now();
        let update = |done, total| {
            progress.done.store(done, Ordering::Relaxed);
            progress.total.store(total, Ordering::Relaxed);
            if last.elapsed() >= Duration::from_millis(120) {
                last = Instant::now();
                ctx.request_repaint();
            }
        };
        let status = actions::storage_action(&dirs, game.as_deref(), &build, action, &progress.cancel, update);
        let settings = dirs.settings_file();
        let result = StorageOutcome {
            status, message,
            journal: storage::read_journal(&dirs.move_journal()),
            locations: super::store::Settings::load_from(&settings).storage,
        };
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

fn location_label(location: Location) -> &'static str {
    tr(match location {
        Location::Cache => "storage.cache",
        Location::Data => "storage.data",
    })
}

fn storage_section(core: &mut CoreState, ui: &mut egui::Ui) {
    let demo = core.ui.demo;
    let busy = (core.runner.busy() || core.ui.settings.saves.pending.is_some()) && !demo;
    let before = core.settings.storage.verify_cache_by_hash;
    if !demo && !core.ui.settings.storage.pending() && core.ui.settings.storage.measuring.is_none()
        && core.ui.settings.storage.usage.is_none()
    {
        start_measurement(core);
    }
    let mut action = None;
    let mut refresh = false;
    let state = &mut core.ui.settings.storage;
    let can_change = !busy && !state.pending() && state.journal.is_none();
    let active = [paths::cache_root(), paths::game_data_root()];
    let base = paths::data_dir();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(theme::strong(tr("storage.title")).size(16.0));
            if ui.add_enabled(before && !state.pending(), egui::Button::new(tr("appearance.reset"))).clicked() {
                core.settings.storage.verify_cache_by_hash = false;
            }
            if ui.add_enabled(state.measuring.is_none(), egui::Button::new(tr("saves.refresh"))).clicked() {
                refresh = true;
            }
        });
        if busy {
            ui.label(theme::dim(tr("saves.busy")));
        }
        for (i, location) in LOCATIONS.into_iter().enumerate() {
            ui.separator();
            ui.label(theme::strong(location_label(location)));
            let location_help = if location == Location::Cache { "storage.cache_help" } else { "storage.data_help" };
            ui.label(theme::dim(tr(location_help)).size(12.5));
            let current = &active[i];
            let path = if demo {
                if location == Location::Cache { "D:/Victory United/cache".into() }
                else { "C:/Users/Player/AppData/Roaming/Victory-United/data".into() }
            } else {
                current.to_string_lossy()
            };
            ui.label(path);
            let demo_usage = Ok(storage::Usage { bytes: if i == 0 { 3_035_000_000 } else { 6_344_000 }, free: 107_592_000_000, network: false });
            let measure = if demo { Some(&demo_usage) } else { state.usage.as_ref().map(|results| &results[i]) };
            match measure {
                Some(Ok(usage)) => {
                    ui.label(theme::dim(trf("storage.usage", &[("size", &display_size(usage.bytes)), ("free", &display_size(usage.free))])));
                    if usage.network {
                        ui.label(theme::warn_text(ui, tr("storage.network")));
                    }
                }
                Some(Err(error)) => { ui.label(theme::warn_text(ui, error)); }
                None => { ui.label(theme::dim(tr("storage.counting"))); }
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("storage.open")).clicked() && !demo {
                    open_folder(current);
                }
                if ui.add_enabled(can_change, egui::Button::new(tr("storage.move"))).clicked() && !demo {
                    if let Some(target) = pick_folder() {
                        plan_move(state, location, target);
                    }
                }
                if ui.add_enabled(can_change, egui::Button::new(tr("storage.clear"))).clicked() {
                    state.plan = Some(StorageConfirm::Clear { location, phrase: String::new() });
                }
                let default = location.default_path(&base);
                if current != &default && ui.add_enabled(can_change, egui::Button::new(tr("storage.default"))).clicked() {
                    plan_move(state, location, default);
                }
            });
        }
        ui.separator();
        ui.add_enabled_ui(!state.pending(), |ui| {
            ui.checkbox(&mut core.settings.storage.verify_cache_by_hash, tr("storage.hash"));
        });
        ui.label(theme::dim(tr("storage.hash_help")).size(12.5));
        if state.pending() {
            let total = state.progress.total.load(Ordering::Relaxed);
            if total > 0 {
                let done = state.progress.done.load(Ordering::Relaxed);
                ui.add(egui::ProgressBar::new((done as f64 / total as f64) as f32).show_percentage());
            } else {
                ui.label(theme::dim(tr("storage.working")));
            }
            if ui.button(tr("storage.pause")).clicked() {
                state.progress.cancel.store(true, Ordering::Relaxed);
            }
        } else {
            if let Some(journal) = &state.journal {
                let switched = journal.phase == MovePhase::Switched;
                ui.label(theme::strong(tr(if switched { "storage.moved" } else { "storage.interrupted" })));
                ui.label(theme::dim(trf("storage.move_paths", &[
                    ("from", &journal.source.to_string_lossy()), ("to", &journal.destination.to_string_lossy()),
                ])));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if switched {
                            if ui.button(tr("storage.keep_old")).clicked() { action = Some(StorageAction::Finish(false)); }
                            if ui.button(tr("storage.delete_old")).clicked() { state.plan = Some(StorageConfirm::DeleteOld); }
                        } else {
                            if ui.button(tr("storage.resume")).clicked() { action = Some(StorageAction::Resume); }
                            if ui.button(tr("storage.cancel")).clicked() { action = Some(StorageAction::Finish(false)); }
                        }
                    });
                });
            }
            ui.add_enabled_ui(!busy, |ui| {
                if let Some(confirm) = &mut state.plan {
                    let approved = match confirm {
                        StorageConfirm::Move { location, target, network } => {
                            ui.label(trf("storage.confirm_move", &[("location", location_label(*location)), ("path", &target.to_string_lossy())]));
                            if *network { ui.label(theme::warn_text(ui, tr("storage.network"))); }
                            true
                        }
                        StorageConfirm::Clear { location: Location::Cache, .. } => {
                            ui.label(theme::warn_text(ui, tr("storage.clear_cache")));
                            true
                        }
                        StorageConfirm::Clear { location: Location::Data, phrase } => {
                            ui.label(theme::warn_text(ui, tr("storage.clear_data")));
                            ui.text_edit_singleline(phrase);
                            phrase == tr("storage.clear_phrase")
                        }
                        StorageConfirm::DeleteOld => {
                            ui.label(theme::warn_text(ui, tr("storage.confirm_old")));
                            true
                        }
                    };
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(approved, egui::Button::new(tr("saves.confirm"))).clicked() {
                            action = state.plan.take().map(|confirm| match confirm {
                                StorageConfirm::Move { location, target, .. } => StorageAction::Move(location, target),
                                StorageConfirm::Clear { location, .. } => StorageAction::Clear(location),
                                StorageConfirm::DeleteOld => StorageAction::Finish(true),
                            });
                        }
                        if ui.button(tr("play.cancel")).clicked() { state.plan = None; }
                    });
                }
            });
        }
        if let Some((error, note)) = &state.note {
            ui.label(if *error { theme::error_text(ui, note) } else { theme::dim(note) });
        }
    });
    if !demo {
        if before != core.settings.storage.verify_cache_by_hash { core.save_settings(); }
        if let Some(action) = action { start_storage_job(core, action); }
        else if refresh { start_measurement(core); }
    }
}

fn plan_move(state: &mut StorageUi, location: Location, target: PathBuf) {
    match storage::network_drive(&target) {
        Ok(network) => state.plan = Some(StorageConfirm::Move { location, target, network }),
        Err(error) => state.note = Some((true, format!("{error:#}"))),
    }
}
