use super::{
    widgets::{notice, pick_folder},
    CoreState,
};
use crate::{app::Shared, i18n::tr, theme};
use eframe::egui;

pub fn show(core: &mut CoreState, _shared: &Shared, logo: &theme::Logo, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| ui.vertical_centered(|ui| {
        ui.add_space(12.0);
        theme::mark(ui, logo, 58.0);
        ui.add_space(10.0);
        ui.label(theme::strong(tr("firstrun.title")).size(28.0));
        ui.add_space(4.0);
        ui.label(theme::dim(tr("firstrun.sub")));
        ui.add_space(12.0);
        let width = ui.available_width().min(640.0);
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            ui.set_width(width);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| body(core, ui));
        });
    }));
}

fn step_header(ui: &mut egui::Ui, n: u8, title: &str, done: bool) {
    ui.horizontal(|ui| {
        let badge = if done { tr("firstrun.done") } else { tr("firstrun.todo") };
        ui.label(theme::strong(format!("{n}.  {title}")).size(16.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::badge(ui, badge, done));
    });
    ui.add_space(6.0);
}

fn body(core: &mut CoreState, ui: &mut egui::Ui) {
    // 1: game folder
    theme::card(ui).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        step_header(ui, 1, tr("firstrun.step_game"), core.game.is_some());
        if let Some(g) = core.game.clone() {
            ui.label(g.title);
            ui.label(theme::dim(g.folder.to_string_lossy()));
        } else {
            ui.label(theme::dim(tr("firstrun.game_text")));
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui.button(tr("firstrun.choose_folder")).clicked() {
                match pick_folder() {
                    Some(p) => match super::game::from_pick(&p) {
                        Some(g) => core.set_game(g),
                        None => core.notice = Some(tr("play.not_a_game").to_string()),
                    },
                    None => {}
                }
            }
            ui.label(theme::dim(tr("firstrun.drop_hint")));
        });
    });
    ui.add_space(8.0);
    // The engine ships with the launcher, so there is nothing to choose. Only a launcher folder that lost
    // its engine says so here.
    if core.engine_exe().is_none() {
        theme::card(ui).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::warn_text(ui, tr("firstrun.engine_missing")));
        });
        ui.add_space(8.0);
    }
    // 3: play
    theme::card(ui).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        step_header(ui, 2, tr("firstrun.step_play"), false);
        ui.label(theme::dim(tr("firstrun.play_text")));
        ui.add_space(8.0);
        let ready = core.game.is_some();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(ready, |ui| {
                if theme::primary_button(ui, tr("firstrun.continue")).clicked() {
                    core.settings.setup_done = true;
                    core.save_settings();
                }
            });
            if ui.button(tr("firstrun.skip")).clicked() {
                core.settings.setup_done = true;
                core.save_settings();
            }
        });
    });
    if let Some(n) = core.notice.clone() {
        ui.add_space(10.0);
        notice(ui, &n, true);
    }
}
