//! Flags tab, for developers and testers: every flag the engine reads, searchable, with risk labels and
//! descriptions, a box to add any flag by name and value, and the list of extra flags kept in
//! `launch-flags.txt`. Extra flags are applied last, so they beat the preset, the options and the edits.
use super::{
    flagreg::{registry, Flag, Kind, Risk},
    launch::{valid_name, ExtraFlag},
    widgets::page_title,
    CoreState,
};
use crate::{app::Shared, i18n::{tr, trf}, theme};
use eframe::egui;

#[derive(Default)]
pub struct FlagsUi {
    pub search: String,
    pub risk: Option<Risk>,
    pub documented_only: bool,
    pub add_name: String,
    pub add_value: String,
    pub message: String,
    pub initialised: bool,
}

pub fn show(core: &mut CoreState, _shared: &Shared, ui: &mut egui::Ui) {
    if !core.ui.flags.initialised {
        core.ui.flags.initialised = true;
        core.ui.flags.documented_only = true;
    }
    page_title(ui, tr("flags.title"), tr("flags.sub"));
    mine(core, ui);
    ui.add_space(14.0);
    registry_list(core, ui);
}

fn risk_badge(ui: &mut egui::Ui, risk: Risk) {
    // Gray tones plus the word: the meaning never depends on color alone.
    theme::badge(ui, risk.label(), risk != Risk::Safe);
}

fn mine(core: &mut CoreState, ui: &mut egui::Ui) {
    let reg = registry();
    theme::card().show(ui, |ui| {
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
                ui.label(egui::RichText::new(&f.name).monospace().color(if f.enabled { theme::TEXT_STRONG } else { theme::TEXT_DIM }));
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
            ui.add_enabled_ui(!core.runner.busy(), |ui| {
                if ui.button(tr("play.start_without_flags")).clicked() {
                    core.play_without_flags();
                }
            });
            ui.label(theme::dim(tr("flags.without_help")).size(12.5));
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
        ui.label(theme::warn_text(tr("flags.bad_name")).size(12.5));
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
    let reg = registry();
    let typed = if value.is_some() { value } else { Some(core.ui.flags.add_value.trim().to_string()).filter(|v| !v.is_empty()) };
    let value = match (typed, reg.get(name)) {
        (Some(v), _) => Some(v),
        (None, Some(f)) if f.kind == Kind::Switch && !f.presence => Some("1".to_string()),
        (None, Some(f)) if f.kind == Kind::Switch => None,
        (None, Some(f)) if !f.default.is_empty() => Some(f.default.clone()),
        (None, _) => None,
    };
    if let Some(existing) = core.extra.iter_mut().find(|e| e.name == name) {
        existing.value = value;
        existing.enabled = true;
    } else {
        core.extra.push(ExtraFlag { name: name.to_string(), value, enabled: true });
    }
    core.ui.flags.add_name.clear();
    core.ui.flags.add_value.clear();
    core.ui.flags.message = trf("flags.added", &[("name", name)]);
}

fn registry_list(core: &mut CoreState, ui: &mut egui::Ui) {
    let reg = registry();
    let state = &mut core.ui.flags;
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(260.0).hint_text(tr("flags.search")));
        if ui.selectable_label(state.risk.is_none(), tr("flags.all")).clicked() {
            state.risk = None;
        }
        for r in [Risk::Safe, Risk::Advanced, Risk::Experimental] {
            if ui.selectable_label(state.risk == Some(r), r.label()).clicked() {
                state.risk = Some(r);
            }
        }
        ui.checkbox(&mut state.documented_only, tr("flags.documented_only"));
    });
    let needle = state.search.trim().to_lowercase();
    let rows: Vec<&Flag> = reg
        .flags
        .iter()
        .filter(|f| !state.documented_only || !f.doc.is_empty())
        .filter(|f| state.risk.is_none_or(|r| f.risk == r))
        .filter(|f| needle.is_empty() || f.name.to_lowercase().contains(&needle) || f.doc.to_lowercase().contains(&needle))
        .collect();
    ui.label(theme::dim(trf("flags.count", &[("shown", &rows.len().to_string()), ("total", &reg.flags.len().to_string()), ("documented", &reg.documented().count().to_string())])).size(12.5));
    ui.add_space(4.0);
    let row_h = 56.0;
    let scroll = core.page_scroll.take();
    let mut to_add: Option<String> = None;
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if let Some(y) = scroll {
        area = area.vertical_scroll_offset(y);
    }
    area.show_rows(ui, row_h, rows.len(), |ui, range| {
        for f in &rows[range] {
            ui.horizontal(|ui| {
                ui.set_height(row_h - 8.0);
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 90.0).max(200.0));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&f.name).monospace().color(theme::TEXT_STRONG));
                        risk_badge(ui, f.risk);
                        let kind = match f.kind {
                            Kind::Switch => "switch",
                            Kind::Number => "number",
                            Kind::Text => "text",
                            Kind::Choice => "choice",
                        };
                        ui.label(theme::dim(kind).size(12.0));
                        if !f.default.is_empty() {
                            ui.label(theme::dim(format!("default {}", f.default)).size(12.0));
                        }
                    });
                    let doc = if f.doc.is_empty() { tr("flags.undocumented") } else { f.doc.as_str() };
                    ui.add(egui::Label::new(theme::dim(doc).size(12.5)).truncate());
                });
                if ui.small_button(tr("flags.add")).clicked() {
                    to_add = Some(f.name.clone());
                }
            });
        }
    });
    if let Some(name) = to_add {
        add_flag(core, &name, None);
        core.save_extra();
    }
}
