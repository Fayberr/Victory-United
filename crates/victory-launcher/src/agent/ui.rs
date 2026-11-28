//! Agent control in the launcher's Settings page.
use super::calls::{Call, Outcome};
use crate::{
    core::{CoreState, widgets},
    i18n::{tr, trf},
    theme,
};
use eframe::egui;
use std::path::PathBuf;

#[derive(Default)]
pub struct View {
    pub port: Option<u16>,
    pub endpoint_file: PathBuf,
    pub calls: Vec<Call>,
    pub error: Option<String>,
}

pub fn show(core: &mut CoreState, ui: &mut egui::Ui) {
    let before = core.settings.agent_control;
    let mut enabled = before;
    let view = &core.ui.agent;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("agent.title")).size(16.0));
        ui.add_space(4.0);
        ui.checkbox(&mut enabled, tr("agent.switch"));
        ui.add(egui::Label::new(theme::dim(tr("agent.help")).size(12.5)).wrap());
        if !enabled {
            return;
        }

        ui.add_space(8.0);
        if let Some(port) = view.port {
            widgets::kv(ui, tr("agent.port"), &port.to_string());
            widgets::kv(
                ui,
                tr("agent.token_file"),
                &view.endpoint_file.to_string_lossy(),
            );
        } else if let Some(error) = &view.error {
            widgets::notice(ui, error, true);
        } else {
            ui.label(theme::dim(tr("agent.starting")));
        }

        ui.add_space(10.0);
        ui.separator();
        ui.label(theme::strong(tr("agent.calls")));
        if view.calls.is_empty() {
            ui.label(theme::dim(tr("agent.empty")));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("recent-agent-calls")
                .max_height(118.0)
                .show(ui, |ui| {
                    egui::Grid::new("agent-calls")
                        .spacing([18.0, 5.0])
                        .show(ui, |ui| {
                            for call in &view.calls {
                                let label = match call.outcome {
                                    Outcome::Ok => "agent.ok",
                                    Outcome::Error => "agent.error",
                                    Outcome::Asked => "agent.asked",
                                };
                                let time = crate::core::runner::timestamp_at(call.time);
                                let (_, clock) = time.split_once('-').unwrap_or_default();
                                let clock =
                                    format!("{}:{}:{}", &clock[..2], &clock[2..4], &clock[4..6]);
                                ui.label(theme::dim(trf("agent.time", &[("time", &clock)])))
                                    .on_hover_text(time);
                                ui.label(egui::RichText::new(&call.tool).monospace());
                                ui.label(tr(label));
                                ui.end_row();
                            }
                        });
                });
        }
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new(tr("agent.turn_off")).min_size(egui::vec2(100.0, 32.0)))
            .clicked()
        {
            enabled = false;
        }
    });
    if enabled != before {
        core.settings.agent_control = enabled;
        core.save_settings();
        core.ctx.request_repaint();
    }
}

/// Synthetic connection data for screenshots. Capture mode never starts an endpoint.
pub fn capture(core: &mut CoreState, on: bool) {
    core.settings.agent_control = on;
    core.ui.agent = if on {
        View {
            port: Some(51842),
            endpoint_file: r"C:\Users\Player\AppData\Roaming\Victory-United\agent.json".into(),
            calls: [
                (1_767_268_806, "status", Outcome::Ok),
                (1_767_268_804, "launch_preview", Outcome::Ok),
                (1_767_268_680, "restore_backup", Outcome::Asked),
            ]
            .into_iter()
            .map(|(time, tool, outcome)| Call {
                time,
                tool: tool.into(),
                outcome,
            })
            .collect(),
            error: None,
        }
    } else {
        View::default()
    };
}
