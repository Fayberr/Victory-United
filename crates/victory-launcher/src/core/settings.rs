//! Settings tab, two layers. Everyday: preset, console, a few switches, upscaler. Below that the curated flags
//! as typed fields, a marker and a reset for everything changed from the preset, saved profiles, and a
//! preview of the exact environment and command line.
use super::{
    flagreg::{registry, Flag, Kind},
    launch::{self, Resolved, Source},
    presets::builtin,
    widgets::{open_folder, page_scroll, page_title, pick_file, pick_folder, save_file},
    store::{Appearance, ThemeChoice},
    CoreState,
};
use crate::{app::Shared, i18n::tr, theme};
use eframe::egui;
use std::collections::HashSet;

#[derive(Default)]
pub struct SettingsUi {
    pub new_name: String,
    pub message: String,
}

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    page_title(ui, tr("settings.title"), tr("settings.sub"));
    let scroll = core.page_scroll.take();
    page_scroll(ui, scroll, |ui| {
        ui.set_max_width(820.0);
        let (resolved, preset_id) = core.resolved();
        appearance_section(core, ui);
        ui.add_space(12.0);
        preset_section(core, ui, &preset_id);
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
    core.profile.edits.remove(&flag.name);
    core.profile.removed.retain(|r| r != &flag.name);
    if let Some(v) = value {
        core.profile.edits.insert(flag.name.clone(), v);
    }
    core.save_profile();
}

/// Turns a switch on or off in the profile. Off removes the variable unless the registry says the engine
/// wants an explicit 0.
pub fn set_switch(core: &mut CoreState, flag: &Flag, on: bool) {
    core.profile.edits.remove(&flag.name);
    core.profile.removed.retain(|r| r != &flag.name);
    if on {
        core.profile.edits.insert(flag.name.clone(), "1".into());
    } else if let Some(zero) = flag.off_value() {
        core.profile.edits.insert(flag.name.clone(), zero.into());
    } else {
        core.profile.removed.push(flag.name.clone());
    }
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
        let names: Vec<String> = {
            let mut n: Vec<String> = core.profiles.iter().map(|p| p.name.clone()).collect();
            if !n.contains(&core.profile.name) {
                n.push(core.profile.name.clone());
            }
            n.sort_by_key(|s| s.to_lowercase());
            n
        };
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
                core.profile = launch::Profile { name: name.clone(), ..Default::default() };
                core.ui.settings.new_name.clear();
                core.save_profile();
            }
            if ui.add_enabled(ok, egui::Button::new(tr("settings.duplicate"))).clicked() {
                core.profile.name = name.clone();
                core.ui.settings.new_name.clear();
                core.save_profile();
            }
            if ui.add_enabled(ok, egui::Button::new(tr("settings.rename"))).clicked() {
                let old = core.profile.name.clone();
                core.profile.name = name;
                core.ui.settings.new_name.clear();
                core.save_profile();
                if core.persist {
                    let _ = launch::delete_profile(&crate::paths::profiles_dir(), &old);
                    core.profiles = launch::list_profiles(&crate::paths::profiles_dir());
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(names.len() > 1, egui::Button::new(tr("settings.delete"))).clicked() {
                if core.persist {
                    let _ = launch::delete_profile(&crate::paths::profiles_dir(), &core.profile.name);
                    core.profiles = launch::list_profiles(&crate::paths::profiles_dir());
                }
                core.profile = core.profiles.first().cloned().unwrap_or_default();
                core.settings.active_profile = core.profile.name.clone();
                core.save_settings();
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
