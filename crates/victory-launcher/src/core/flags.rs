//! Flags tab, for developers and testers: every flag the engine reads, searchable, with risk labels and
//! descriptions, a box to add any flag by name and value, and the list of extra flags kept in
//! `launch-flags.txt`. Extra flags are applied last, so they beat the preset, the options and the edits.
use super::{
    flagreg::{registry, Flag, Kind, Risk},
    launch::{default_value, put_extra, valid_name},
    widgets::{page_scroll, page_title},
    CoreState,
};
use crate::{app::Shared, i18n::{tr, trf}, theme};
use eframe::egui;

#[derive(Default)]
pub struct FlagsUi {
    pub search: String,
    pub risk: Option<Risk>,
    pub show_undocumented: bool,
    pub add_name: String,
    pub add_value: String,
    pub message: String,
    pub initialised: bool,
}

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    core.ui.flags.initialised = true;
    page_title(ui, tr("flags.title"), tr("flags.sub"));
    let scroll = core.page_scroll.take();
    page_scroll(ui, scroll, |ui| {
        mine(core, ui);
        ui.add_space(14.0);
        registry_list(core, ui);
        ui.add_space(8.0);
    });
}

fn risk_badge(ui: &mut egui::Ui, risk: Risk) {
    // Gray tones plus the word: the meaning never depends on color alone.
    theme::badge(ui, risk.label(), risk != Risk::Safe);
}

fn mine(core: &mut CoreState, ui: &mut egui::Ui) {
    let reg = registry();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("flags.mine")).size(16.0));
        ui.label(theme::dim(tr("flags.mine_help")).size(12.5));
        ui.add_space(6.0);
        let mut changed = false;
        let mut remove: Option<usize> = None;
        if core.extra.is_empty() {
            ui.label(theme::dim(tr("flags.none")));
        }
        for (i, f) in core.extra.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut f.enabled, "").changed();
                ui.label(egui::RichText::new(&f.name).monospace().color(if f.enabled { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() }));
                match reg.get(&f.name) {
                    Some(flag) => risk_badge(ui, flag.risk),
                    None => theme::badge(ui, tr("flags.custom"), false),
                }
                let mut text = f.value.clone().unwrap_or_default();
                let hint = if f.value.is_none() { tr("flags.presence") } else { "" };
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(150.0).hint_text(hint)).changed() {
                    f.value = if text.is_empty() { None } else { Some(text) };
                    changed = true;
                }
                if ui.small_button(tr("flags.remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            core.extra.remove(i);
            changed = true;
        }
        ui.add_space(8.0);
        add_row(core, ui, &mut changed);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let allowed = super::play::hardware_allows_play(core.hw.as_ref(), false).is_ok();
            ui.add_enabled_ui(!core.runner.busy() && allowed, |ui| {
                if ui.button(tr("play.start_without_flags")).clicked() {
                    core.play_without_flags(false);
                }
            });
            ui.label(theme::dim(tr(if allowed { "flags.without_help" } else { "play.mesh_flags_help" })).size(12.5));
        });
        if changed {
            core.save_extra();
        }
    });
}

fn add_row(core: &mut CoreState, ui: &mut egui::Ui, changed: &mut bool) {
    let reg = registry();
    let mut add_now: Option<String> = None;
    ui.horizontal(|ui| {
        let state = &mut core.ui.flags;
        ui.add(egui::TextEdit::singleline(&mut state.add_name).desired_width(280.0).hint_text(tr("flags.name_hint")).font(egui::TextStyle::Monospace));
        ui.add(egui::TextEdit::singleline(&mut state.add_value).desired_width(140.0).hint_text(tr("flags.value_hint")));
        let name = state.add_name.trim().to_ascii_uppercase();
        if ui.add_enabled(valid_name(&name), egui::Button::new(tr("flags.add"))).clicked() {
            add_now = Some(name);
        }
    });
    if let Some(name) = add_now {
        add_flag(core, &name, None);
        *changed = true;
    }
    let state = &mut core.ui.flags;
    let typed = state.add_name.trim().to_ascii_uppercase();
    if !typed.is_empty() && !valid_name(&typed) {
        ui.label(theme::warn_text(ui, tr("flags.bad_name")).size(12.5));
    } else if !typed.is_empty() && reg.get(&typed).is_none() {
        ui.label(theme::dim(tr("flags.unknown_ok")).size(12.5));
    }
    // Autocomplete from the registry.
    if !typed.is_empty() && reg.get(&typed).is_none() {
        let hits: Vec<&Flag> = reg.flags.iter().filter(|f| f.name.contains(&typed)).take(6).collect();
        ui.horizontal_wrapped(|ui| {
            for f in hits {
                if ui.small_button(&f.name).clicked() {
                    state.add_name = f.name.clone();
                }
            }
        });
    }
    if !state.message.is_empty() {
        ui.label(theme::dim(&state.message).size(12.5));
    }
}

/// Adds or replaces an extra flag. An empty value means: switch on (`1`) for a known switch, presence only
/// for anything else.
fn add_flag(core: &mut CoreState, name: &str, value: Option<String>) {
    let typed = if value.is_some() { value } else { Some(core.ui.flags.add_value.trim().to_string()).filter(|v| !v.is_empty()) };
    put_extra(&mut core.extra, name, default_value(name, typed));
    core.ui.flags.add_name.clear();
    core.ui.flags.add_value.clear();
    core.ui.flags.message = trf("flags.added", &[("name", name)]);
}

/// Groups shown on this tab, in this order. Flags without a group come last as "Experimental and other".
const GROUPS: [(&str, &str); 5] = [
    ("graphics", "settings.g_graphics"),
    ("memory", "settings.g_memory"),
    ("threads", "settings.g_threads"),
    ("paths", "flags.g_paths"),
    ("debug", "settings.g_debug"),
];

fn registry_list(core: &mut CoreState, ui: &mut egui::Ui) {
    let reg = registry();
    let state = &mut core.ui.flags;
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(240.0).hint_text(tr("flags.search")));
        if ui.selectable_label(state.risk.is_none(), tr("flags.all")).clicked() {
            state.risk = None;
        }
        for r in [Risk::Safe, Risk::Advanced, Risk::Experimental] {
            let word = r.label();
            let word = format!("{}{}", word[..1].to_uppercase(), &word[1..]);
            if ui.selectable_label(state.risk == Some(r), word).clicked() {
                state.risk = Some(r);
            }
        }
    });
    let undocumented_total = reg.flags.len() - reg.documented().count();
    ui.checkbox(&mut state.show_undocumented, trf("flags.show_undocumented", &[("count", &undocumented_total.to_string())]));
    let needle = state.search.trim().to_lowercase();
    let matches = |f: &&Flag| {
        state.risk.is_none_or(|r| f.risk == r)
            && (needle.is_empty() || f.name.to_lowercase().contains(&needle) || f.doc.to_lowercase().contains(&needle))
    };
    let described: Vec<&Flag> = reg.flags.iter().filter(|f| !f.doc.is_empty()).filter(matches).collect();
    // A search always looks at every flag, so a name that is not described yet can still be found.
    let plain: Vec<&Flag> = if state.show_undocumented || !needle.is_empty() {
        reg.flags.iter().filter(|f| f.doc.is_empty()).filter(matches).collect()
    } else {
        Vec::new()
    };
    ui.add_space(6.0);
    let mut to_add: Option<String> = None;
    if described.is_empty() && plain.is_empty() {
        ui.label(theme::dim(tr("flags.nothing")));
    }
    let mut sections: Vec<(String, Vec<&Flag>)> = Vec::new();
    for (group, title) in GROUPS {
        sections.push((tr(title).to_string(), described.iter().copied().filter(|f| f.group.as_deref() == Some(group)).collect()));
    }
    sections.push((tr("flags.g_other").to_string(), described.iter().copied().filter(|f| f.group.is_none()).collect()));
    for (title, flags) in sections.iter().filter(|(_, f)| !f.is_empty()) {
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(title).size(16.0));
            for (i, f) in flags.iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                    ui.separator();
                }
                ui.add_space(2.0);
                flag_row(ui, f, &mut to_add);
            }
        });
        ui.add_space(10.0);
    }
    if !plain.is_empty() {
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(trf("flags.g_plain", &[("count", &plain.len().to_string())])).size(16.0));
            ui.label(theme::dim(tr("flags.g_plain_help")).size(12.5));
            ui.add_space(4.0);
            for f in &plain {
                ui.horizontal(|ui| {
                    flag_name(ui, &f.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(tr("flags.add")).clicked() {
                            to_add = Some(f.name.clone());
                        }
                    });
                });
            }
        });
    }
    if let Some(name) = to_add {
        add_flag(core, &name, None);
        core.save_extra();
    }
}

/// `VICTORY_` in dim text, the rest of the name in strong monospace: the part that tells flags apart stands out.
fn flag_name(ui: &mut egui::Ui, name: &str) {
    let (prefix, rest) = name.strip_prefix("VICTORY_").map_or(("", name), |r| ("VICTORY_", r));
    ui.spacing_mut().item_spacing.x = 0.0;
    if !prefix.is_empty() {
        ui.label(egui::RichText::new(prefix).monospace().weak());
    }
    ui.label(egui::RichText::new(rest).monospace().color(ui.visuals().strong_text_color()));
}

fn flag_row(ui: &mut egui::Ui, f: &Flag, to_add: &mut Option<String>) {
    ui.horizontal(|ui| {
        flag_name(ui, &f.name);
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.add_space(4.0);
        risk_badge(ui, f.risk);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(tr("flags.add")).on_hover_text(tr("flags.add_help")).clicked() {
                *to_add = Some(f.name.clone());
            }
        });
    });
    ui.add(egui::Label::new(theme::dim(&f.doc).size(13.0)).wrap());
    let mut facts = vec![match f.kind {
        Kind::Switch => tr("flags.kind_switch").to_string(),
        Kind::Number => tr("flags.kind_number").to_string(),
        Kind::Text => tr("flags.kind_text").to_string(),
        Kind::Choice if f.choices.is_empty() => tr("flags.kind_choice").to_string(),
        Kind::Choice => trf("flags.kind_choice_of", &[("choices", &f.choices.join(", "))]),
    }];
    if !f.default.is_empty() {
        facts.push(trf("flags.default", &[("value", &f.default)]));
    }
    ui.label(theme::dim(facts.join("  |  ")).size(11.5));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::launch::ExtraFlag;

    #[test]
    fn every_described_flag_lands_in_a_section() {
        let known: Vec<&str> = GROUPS.iter().map(|(g, _)| *g).collect();
        for f in registry().flags.iter().filter(|f| !f.doc.is_empty()) {
            if let Some(g) = &f.group {
                assert!(known.contains(&g.as_str()), "{} has the group {g}, which the Flags tab does not list", f.name);
            }
        }
    }

    #[test]
    fn adding_flags_picks_sensible_values() {
        let mut core = CoreState::demo();
        add_flag(&mut core, "VICTORY_UNDER_WIDE", None);
        add_flag(&mut core, "VICTORY_MY_OWN_THING", None);
        core.ui.flags.add_value = "5".into();
        add_flag(&mut core, "VICTORY_THIN_THREADS", None);
        fn get(core: &CoreState, n: &str) -> ExtraFlag {
            core.extra.iter().find(|e| e.name == n).unwrap().clone()
        }
        let get = |n: &str| get(&core, n);
        assert_eq!(get("VICTORY_UNDER_WIDE").value.as_deref(), Some("1"), "a known switch is switched on");
        assert_eq!(get("VICTORY_MY_OWN_THING").value, None, "an unknown name is presence only");
        assert_eq!(get("VICTORY_THIN_THREADS").value.as_deref(), Some("5"));
        let _ = get;
        add_flag(&mut core, "VICTORY_THIN_THREADS", Some("7".into()));
        assert_eq!(core.extra.iter().filter(|e| e.name == "VICTORY_THIN_THREADS").count(), 1, "adding again replaces");
        assert_eq!(core.extra.iter().find(|e| e.name == "VICTORY_THIN_THREADS").unwrap().value.as_deref(), Some("7"));
    }
}
