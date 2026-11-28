//! The live view and bounded history of the engine's shared statistics.
use super::{
    OpsState,
    settings::{Card, CardChoice, Field, MonitorSettings, OpsSettings, Preset, Rate},
};
use crate::{
    app::Shared,
    core::hw::Hardware,
    i18n::{tr, trf},
    paths, theme,
};
use eframe::egui;
use egui_plot::{HLine, Line, Plot, PlotBounds, PlotPoints};
use std::{
    collections::VecDeque,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use victory_stats::{Metric, PresentState, Reader, Snapshot, UNKNOWN_US};

const HISTORY_SECONDS: f64 = 120.0;
const HISTORY_FRAMES: usize = 60_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Live {
    pub gpu_load: Option<f32>,
    pub nvml_load: bool,
    pub gpu_temp: Option<u32>,
    pub cpu_load: Option<f32>,
    pub system_ram: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Row {
    pub number: u64,
    qpc: u64,
    pub t_s: f64,
    pub frame_ms: Option<f32>,
    pub cpu_ms: Option<f32>,
    pub gpu_ms: Option<f32>,
    pub idle_cpu_ms: Option<f32>,
    pub present_ms: Option<f32>,
}

impl Row {
    pub fn gpu_estimate(&self) -> Option<f32> {
        let period = self.frame_ms?;
        let gpu = self.gpu_ms?;
        (period > 0.0).then(|| (gpu / period * 100.0).clamp(0.0, 100.0))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub fps: Option<f32>,
    pub low_fps: Option<f32>,
    pub row: Option<Row>,
    pub state: Option<PresentState>,
    pub build: String,
    pub frames: u64,
    pub occluded: Option<u64>,
    pub rejected: Option<u64>,
    pub adapter_luid: Option<u64>,
    pub live: Live,
    legacy: [Option<u64>; 5],
    interval: Option<victory_stats::Interval>,
    stages: Vec<Stage>,
}

#[derive(Clone, Debug)]
struct Stage {
    name: String,
    ms: f64,
    delta_ms: Option<f64>,
}

impl Summary {
    fn metric(&self, metric: Metric) -> Option<u64> {
        if let Some(interval) = &self.interval {
            return interval.metric(metric);
        }
        self.legacy[match metric {
            Metric::VramUsed => 0,
            Metric::VramBudget => 1,
            Metric::RamResident => 2,
            Metric::DrawsExecuted => 3,
            Metric::DispatchesExecuted => 4,
            _ => return None,
        }]
    }

    /// Machine-readable version of the measurements shown by the monitor.
    pub fn snapshot_json(&self) -> serde_json::Value {
        use serde_json::json;
        let present = match self.state {
            Some(PresentState::Waiting) => json!("waiting"),
            Some(PresentState::Drawing) => json!("drawing"),
            Some(PresentState::Occluded) => json!("occluded"),
            Some(PresentState::Other(code)) => json!({ "code": code }),
            None => serde_json::Value::Null,
        };
        json!({
            "fps": self.fps, "low_fps": self.low_fps, "frame": self.row, "present": present,
            "build": self.build, "frames": self.frames, "occluded": self.occluded, "rejected": self.rejected,
            "adapter_luid": self.adapter_luid, "vram_bytes": self.metric(Metric::VramUsed),
            "vram_budget_bytes": self.metric(Metric::VramBudget), "resident_bytes": self.metric(Metric::RamResident),
            "draws": self.metric(Metric::DrawsExecuted), "dispatches": self.metric(Metric::DispatchesExecuted),
            "pipelines_building": self.metric(Metric::PipelinesBuilding),
            "stages": self.stages.iter().map(|stage| json!({
                "name": stage.name, "ms": stage.ms, "delta_ms": stage.delta_ms,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn with_live(&self, live: Live) -> Self {
        Self {
            live,
            ..self.clone()
        }
    }

    pub fn value(&self, field: Field, target_ms: f32, missed: u64) -> Option<String> {
        use Field::*;
        let count = |m| self.metric(m).map(group_digits);
        let bytes = |m| self.metric(m).map(size);
        let ms = |v: Option<f32>| v.map(|v| format!("{v:.1} ms"));
        let percent = |v: Option<f32>| v.map(|v| format!("{v:.0}%"));
        match field {
            Fps => self.fps.map(|v| format!("{v:.0}")),
            LowFps => self.low_fps.map(|v| format!("{v:.0}")),
            FrameMs => ms(self.row?.frame_ms).map(|v| format!("{v} / {target_ms:.1}")),
            GpuEstimate => percent(self.row?.gpu_estimate()),
            GpuIdleCpu => ms(self.row?.idle_cpu_ms),
            CpuMs => ms(self.row?.cpu_ms),
            GpuMs => ms(self.row?.gpu_ms),
            GpuLoad => percent(self.live.gpu_load),
            GpuTemp => self.live.gpu_temp.map(|v| format!("{v} °C")),
            CpuLoad => percent(self.live.cpu_load),
            Vram => bytes(Metric::VramUsed).map(|used| {
                bytes(Metric::VramBudget)
                    .map_or_else(|| used.clone(), |budget| format!("{used} / {budget}"))
            }),
            Ram => bytes(Metric::RamResident),
            SystemRam => self
                .live
                .system_ram
                .map(|(used, total)| format!("{} / {}", size(used), size(total))),
            CopyTo => bytes(Metric::CopiedToGpu),
            CopyFrom => bytes(Metric::CopiedFromGpu),
            InPlaceBytes => bytes(Metric::InPlaceBytes),
            InPlacePages => count(Metric::InPlacePages),
            Draws => count(Metric::DrawsExecuted),
            DrawsIssued => count(Metric::DrawsIssued),
            Dispatches => count(Metric::DispatchesExecuted),
            DispatchesIssued => count(Metric::DispatchesIssued),
            PipelineDrops => count(Metric::PipelineDrops),
            IndirectDrops => count(Metric::IndirectDrops),
            PresentMs => ms(self.row?.present_ms),
            Pipelines => count(Metric::PipelinesBuilding),
            PipelineWait => self
                .metric(Metric::PipelineWaitUs)
                .map(|v| format!("{:.1} ms", v as f64 / 1000.0)),
            TextureRoom => count(Metric::TexturesWithoutRoom),
            TextureTileRoom => count(Metric::TextureLevelsWithoutTiles),
            TextureUploads => bytes(Metric::TextureUploadBytes),
            WriteWatch => count(Metric::WriteWatchQueries),
            Frame => Some(self.frames.to_string()),
            Missed => Some(missed.to_string()),
            Occluded => self.occluded.map(|n| n.to_string()),
            Rejected => self.rejected.map(|n| n.to_string()),
        }
    }
}

/// 1687104 as "1,687,104": long counts are readable at a glance.
fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    }
}

fn micros(us: u32, legacy: bool) -> Option<f32> {
    (us != UNKNOWN_US && (!legacy || us != 0)).then_some(us as f32 / 1000.0)
}

#[derive(Default)]
pub struct MonitorState {
    reader: Option<Reader>,
    pid: Option<u32>,
    last_read: Option<Instant>,
    pub summary: Option<Summary>,
    pub history: VecDeque<Row>,
    first_qpc: Option<u64>,
    last_qpc: Option<u64>,
    seen_frames: u64,
    slow_qpc: u64,
    selected: Option<u64>,
    pub export_note: Option<String>,
    demo: Option<Demo>,
}

impl MonitorState {
    pub fn forget(&mut self) {
        if self.pid.is_some() {
            *self = Self {
                export_note: self.export_note.take(),
                ..Self::default()
            };
        }
    }

    pub fn refresh(&mut self, pid: u32, now: Instant, rate: Duration) -> bool {
        if self.pid != Some(pid) && self.demo.is_none() {
            self.forget();
            self.pid = Some(pid);
        }
        if self.last_read.is_some_and(|last| {
            now.saturating_duration_since(last)
                .saturating_add(Duration::from_millis(12))
                < rate
        }) {
            return false;
        }
        self.last_read = Some(now);
        if let Some(demo) = &mut self.demo {
            demo.advance(87);
            let (snapshot, qpc) = (demo.writer.snapshot(), demo.qpc);
            if let Some(snapshot) = snapshot {
                self.ingest(snapshot, qpc);
            }
            return true;
        }
        if self.reader.is_none() {
            self.reader = Reader::open(pid);
        }
        if let Some(snapshot) = self.reader.as_ref().and_then(Reader::snapshot) {
            self.ingest(snapshot, victory_stats::qpc_now());
        }
        true
    }

    fn ingest(&mut self, snap: Snapshot, at_qpc: u64) {
        let freq = snap.qpc_freq.max(1);
        let legacy = snap.interval.is_none();
        let origin = snap
            .frames_recent
            .first()
            .map(|s| *self.first_qpc.get_or_insert(s.qpc));
        let cursor = self.seen_frames;
        let first_frame = snap.frames.saturating_sub(snap.frames_recent.len() as u64) + 1;
        for (i, sample) in snap.frames_recent.iter().enumerate() {
            let number = first_frame + i as u64;
            if number <= cursor {
                continue;
            }
            let before = if i > 0 {
                Some(snap.frames_recent[i - 1].qpc)
            } else if cursor + 1 == number {
                self.last_qpc
            } else {
                None
            };
            let gap = !legacy && sample.cpu_us == UNKNOWN_US;
            let frame_ms = before
                .filter(|_| !gap)
                .and_then(|b| sample.qpc.checked_sub(b))
                .map(|ticks| (ticks as f64 * 1_000.0 / freq as f64) as f32);
            let gpu_ms = frame_ms
                .zip(micros(sample.timing.gpu_idle_us, false))
                .map(|(frame, idle)| (frame - idle).max(0.0));
            self.history.push_back(Row {
                number,
                qpc: sample.qpc,
                t_s: sample.qpc.saturating_sub(origin.unwrap_or(sample.qpc)) as f64 / freq as f64,
                frame_ms,
                cpu_ms: micros(sample.cpu_us, legacy),
                gpu_ms,
                idle_cpu_ms: micros(sample.timing.gpu_idle_cpu_us, false),
                present_ms: micros(sample.timing.call_us, false),
            });
        }
        self.seen_frames = snap.frames;
        self.last_qpc = snap.frames_recent.last().map(|s| s.qpc);
        let oldest = at_qpc.saturating_sub((HISTORY_SECONDS * freq as f64) as u64);
        while self.history.len() > HISTORY_FRAMES
            || self.history.front().is_some_and(|r| r.qpc < oldest)
        {
            self.history.pop_front();
        }
        let state = snap.present_state;
        let drawing = matches!(state, None | Some(PresentState::Drawing));
        let recent = at_qpc.saturating_sub(freq);
        let fps = (drawing && snap.frames > 0).then(|| {
            self.history
                .iter()
                .rev()
                .take_while(|r| r.qpc > recent)
                .count() as f32
        });
        let mut tail = self
            .history
            .iter()
            .rev()
            .take_while(|r| r.qpc >= at_qpc.saturating_sub(freq.saturating_mul(5)))
            .filter_map(|r| r.frame_ms)
            .collect::<Vec<_>>();
        tail.sort_unstable_by(|a, b| b.total_cmp(a));
        let n = tail.len().div_ceil(100);
        let low_fps = (drawing && n > 0)
            .then(|| 1_000.0 / (tail[..n].iter().sum::<f32>() / n as f32).max(0.001));
        let previous = self.summary.take().unwrap_or_default();
        let mut stages = previous.stages;
        if self.slow_qpc != snap.slow_qpc
            && let Some(interval) = &snap.interval
            && interval.frames > 0
        {
            stages = interval
                .stage_us
                .iter()
                .map(|(name, us)| {
                    let ms = *us as f64 / 1_000.0 / interval.frames as f64;
                    let delta_ms = stages
                        .iter()
                        .find(|stage| &stage.name == name)
                        .map(|stage| ms - stage.ms);
                    Stage {
                        name: name.clone(),
                        ms,
                        delta_ms,
                    }
                })
                .collect();
            self.slow_qpc = snap.slow_qpc;
        }
        let legacy_value = |v| (v != 0).then_some(v);
        self.summary = Some(Summary {
            fps,
            low_fps,
            row: (drawing && snap.last_qpc >= recent)
                .then(|| self.history.back().copied())
                .flatten(),
            state,
            build: snap.build,
            frames: snap.frames,
            occluded: snap.occluded,
            rejected: snap.rejected,
            adapter_luid: snap
                .interval
                .as_ref()
                .and_then(|v| v.metric(Metric::AdapterLuid)),
            live: previous.live,
            legacy: [
                legacy_value(snap.vram_used),
                legacy_value(snap.vram_budget),
                legacy_value(snap.ram_resident),
                (snap.slow_qpc != 0).then_some(snap.draws),
                (snap.slow_qpc != 0).then_some(snap.dispatches),
            ],
            interval: snap.interval,
            stages,
        });
    }

    pub fn demo() -> Self {
        let mut demo = Demo::new();
        let mut state = Self::default();
        for _ in 0..24 {
            demo.advance(87);
            if let Some(snap) = demo.writer.snapshot() {
                state.ingest(snap, demo.qpc);
            }
        }
        state.summary.as_mut().unwrap().live = Live {
            gpu_load: Some(77.0),
            nvml_load: true,
            gpu_temp: Some(63),
            cpu_load: Some(46.0),
            system_ram: Some((22 << 30, 64 << 30)),
        };
        state.demo = Some(demo);
        state
    }
}

struct Demo {
    writer: victory_stats::Writer,
    qpc: u64,
    frame: u64,
    freq: u64,
}

impl Demo {
    fn new() -> Self {
        let writer = victory_stats::Writer::in_memory_named(
            4242,
            "a1b2c3d4 release",
            &["Buffer refresh", "Texture refresh", "Readback"],
        );
        Self {
            freq: writer.qpc_freq(),
            writer,
            qpc: 0,
            frame: 0,
        }
    }

    fn advance(&mut self, count: u64) {
        for _ in 0..count {
            self.frame += 1;
            let n = self.frame as f64;
            let hitch = if self.frame.is_multiple_of(311) {
                13_500.0
            } else {
                0.0
            };
            let us = 11_200.0 + 1_450.0 * (n / 23.0).sin() + 660.0 * (n / 5.3).sin() + hitch;
            self.qpc += (us * self.freq as f64 / 1e6) as u64;
            let t = victory_stats::PresentTiming {
                call_us: 280,
                gpu_idle_us: (us * 0.22) as u32,
                gpu_idle_cpu_us: (us * 0.15) as u32,
            };
            self.writer
                .frame_with_timing(self.qpc, (us * 0.48) as u32, 700, t);
        }
        for (metric, value) in [
            (Metric::AdapterLuid, 1),
            (Metric::VramUsed, 9300 << 20),
            (Metric::VramBudget, 11240 << 20),
            (Metric::RamResident, 13700 << 20),
            (Metric::CopiedToGpu, 287 << 20),
            (Metric::CopiedFromGpu, 144 << 20),
            (Metric::InPlaceBytes, 420 << 20),
            (Metric::InPlacePages, 6720),
            (Metric::DrawsIssued, self.frame * 815),
            (Metric::DrawsExecuted, self.frame * 808),
            (Metric::DispatchesIssued, self.frame * 251),
            (Metric::DispatchesExecuted, self.frame * 250),
            (Metric::PipelineDrops, 14),
            (Metric::IndirectDrops, 3),
            (Metric::IndirectBudgetDrops, 1),
            (Metric::PipelinesBuilding, 3),
            (Metric::PipelineWaitUs, 348_000),
            (Metric::TexturesWithoutRoom, 6),
            (Metric::TextureLevelsWithoutTiles, 2),
            (Metric::TextureUploadBytes, 17 << 30),
            (Metric::WriteWatchQueries, self.frame * 115),
        ] {
            self.writer.set_metric(metric, Some(value));
        }
        self.writer.set_stages(
            count,
            &[
                count * (2160 + self.frame % 180),
                count * (1380 + self.frame % 420),
                count * 791,
            ],
        );
        self.writer.slow_done(self.qpc);
    }
}

pub fn export_csv(path: &Path, history: &VecDeque<Row>, seconds: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        (10..=120).contains(&seconds),
        "CSV history must be 10 to 120 seconds."
    );
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(
        out,
        "frame,time_s,frame_ms,cpu_submission_ms,estimated_gpu_ms,gpu_idle_while_cpu_ms,present_call_ms"
    )?;
    for r in recent(history, seconds) {
        let format = |v: Option<f32>| v.map_or_else(String::new, |v| format!("{v:.3}"));
        writeln!(
            out,
            "{},{:.6},{},{},{},{},{}",
            r.number,
            r.t_s,
            format(r.frame_ms),
            format(r.cpu_ms),
            format(r.gpu_ms),
            format(r.idle_cpu_ms),
            format(r.present_ms)
        )?;
    }
    out.flush()?;
    Ok(())
}

/// Writes the last `seconds` of history to a new `frames-<time>.csv` in `dir`.
pub fn export_to(dir: &Path, history: &VecDeque<Row>, seconds: u32) -> anyhow::Result<PathBuf> {
    let file = dir.join(format!("frames-{}.csv", super::stamp()));
    export_csv(&file, history, seconds)?;
    Ok(file)
}

/// The rows of the last `seconds`, oldest first.
pub fn recent(history: &VecDeque<Row>, seconds: u32) -> impl Iterator<Item = &Row> {
    let cutoff = history.back().map_or(0.0, |r| r.t_s - f64::from(seconds));
    let start = history.partition_point(|r| r.t_s < cutoff);
    history.iter().skip(start)
}

pub fn missed_intervals(history: &VecDeque<Row>, seconds: u32, target_ms: f32) -> u64 {
    recent(history, seconds)
        .filter_map(|frame| frame.frame_ms)
        .map(|ms| ((ms / target_ms.max(1.0)).floor() as u64).saturating_sub(1))
        .sum()
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Live,
    Customize,
}

pub fn show(
    ops: &mut OpsState,
    shared: &Shared,
    hw: Option<&Hardware>,
    scroll: Option<f32>,
    ui: &mut egui::Ui,
) {
    let before = ops.cfg.clone();
    // The panel is drawn by the diagnostics engine build only.
    let release_engine = crate::core::actions::build_name(shared.engine_exe.as_deref()).ends_with("-release");
    ui.horizontal(|ui| {
        ui.label(theme::strong(tr("monitor.title")).size(24.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !ops.monitor.history.is_empty(),
                    egui::Button::new(tr("monitor.export")),
                )
                .clicked()
            {
                let dir = paths::reports_dir();
                let result = if ops.persist {
                    export_to(&dir, &ops.monitor.history, ops.cfg.monitor.csv_seconds)
                } else {
                    Ok(dir.join("frames-demo.csv"))
                };
                ops.monitor.export_note = Some(match result {
                    Ok(file) => trf("monitor.exported", &[("file", &file.display().to_string())]),
                    Err(e) => trf("monitor.export_failed", &[("error", &e.to_string())]),
                });
            }
        });
    });
    ui.horizontal(|ui| {
        for (page, label) in [
            (Page::Live, "monitor.live"),
            (Page::Customize, "monitor.customize"),
        ] {
            ui.selectable_value(&mut ops.monitor_page, page, tr(label));
        }
    });
    if let Some(note) = &ops.monitor.export_note {
        ui.label(theme::dim(note).size(12.5));
    }
    ui.add_space(7.0);
    let mut area = egui::ScrollArea::vertical()
        .id_salt(ops.monitor_page as u8)
        .auto_shrink([false, false]);
    if let Some(scroll) = scroll {
        area = area.vertical_scroll_offset(scroll);
    }
    area.show(ui, |ui| match ops.monitor_page {
        Page::Live => monitor_view(ui, &mut ops.monitor, &ops.cfg.monitor, shared, hw, ops.live),
        Page::Customize => customize(ui, &mut ops.cfg, release_engine),
    });
    ops.save_if_changed(&before);
}

fn monitor_view(
    ui: &mut egui::Ui,
    state: &mut MonitorState,
    config: &MonitorSettings,
    shared: &Shared,
    hw: Option<&Hardware>,
    live: Live,
) {
    let summary = state.summary.as_ref();
    let status = summary.and_then(|s| s.state);
    let message = match (shared.game_pid, summary, status) {
        (None, _, _) | (_, None, _) | (_, _, Some(PresentState::Waiting)) => {
            Some(tr("monitor.waiting"))
        }
        (_, _, Some(PresentState::Occluded)) => Some(tr("monitor.occluded")),
        (_, _, Some(PresentState::Other(_))) => Some(tr("monitor.rejected")),
        _ => None,
    };
    if let Some(message) = message {
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(message));
            if message == tr("monitor.waiting") {
                ui.label(theme::dim(tr("monitor.waiting_help")));
            }
        });
        ui.add_space(9.0);
    }
    let Some(s) = summary else { return };
    let effective = s.with_live(live);
    let missed = missed_intervals(&state.history, config.chart_seconds, config.target_ms);
    for c in config.cards.iter().filter(|c| c.visible) {
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match c.card {
                Card::Overview => headlines(ui, c, s, config.target_ms, missed),
                Card::Chart => frame_chart(ui, &state.history, config, &mut state.selected),
                Card::Activity => activity(ui, &state.history),
                Card::Stages => stages(ui, &s.stages),
                Card::Hardware => {
                    rows(ui, c, &effective, config.target_ms, missed);
                    if effective.live.gpu_load.is_some() {
                        let source = if effective.live.nvml_load {
                            "monitor.gpu_nvml"
                        } else {
                            "monitor.gpu_pdh"
                        };
                        ui.label(theme::dim(tr(source)).size(12.0));
                    }
                    capabilities(ui, hw, effective.adapter_luid);
                }
                _ => {
                    rows(ui, c, &effective, config.target_ms, missed);
                    if c.card == Card::Memory
                        && let (Some(used), Some(budget)) = (
                            effective.metric(Metric::VramUsed),
                            effective.metric(Metric::VramBudget),
                        )
                        && budget > 0
                    {
                        let ratio = (used as f32 / budget as f32).clamp(0.0, 1.0);
                        let pal = theme::palette(ui.visuals());
                        let fill = if ratio < 0.80 {
                            pal.good
                        } else if ratio < 0.95 {
                            pal.warn
                        } else {
                            pal.bad
                        };
                        ui.add_space(6.0);
                        ui.add(
                            egui::ProgressBar::new(ratio)
                                .desired_height(6.0)
                                .fill(fill),
                        )
                        .on_hover_text(format!("VRAM {} / {}", size(used), size(budget)));
                    }
                    if c.card == Card::Commands
                        && c.rows.iter().any(|r| {
                            r.visible
                                && matches!(r.field, Field::PipelineDrops | Field::IndirectDrops)
                        })
                    {
                        drop_reasons(ui, s);
                    }
                    if c.card == Card::Footer {
                        ui.label(theme::dim(&s.build));
                    }
                }
            }
        });
        ui.add_space(9.0);
    }
}

fn headings(ui: &mut egui::Ui, card: Card) {
    ui.label(theme::strong(card.label()).size(15.0));
    ui.add_space(5.0);
}

fn headlines(ui: &mut egui::Ui, card: &CardChoice, s: &Summary, target: f32, missed: u64) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 30.0;
        for row in card.rows.iter().filter(|r| r.visible) {
            ui.vertical(|ui| {
                ui.set_min_width(96.0);
                ui.add(
                    egui::Label::new(
                        theme::strong(
                            s.value(row.field, target, missed)
                                .unwrap_or_else(|| tr("monitor.unavailable_short").to_owned()),
                        )
                        .size(23.0),
                    )
                    .extend(),
                );
                ui.label(theme::dim(row.field.label()).size(12.0));
            })
            .response
            .on_hover_text(match row.field {
                Field::GpuEstimate => tr("monitor.estimate_help"),
                Field::FrameMs => tr("monitor.target_help"),
                _ => row.field.label(),
            });
        }
    });
}

fn rows(ui: &mut egui::Ui, card: &CardChoice, s: &Summary, target: f32, missed: u64) {
    headings(ui, card.card);
    egui::Grid::new(card.card)
        .min_col_width(170.0)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            for r in card.rows.iter().filter(|r| r.visible) {
                ui.label(r.field.label());
                ui.monospace(
                    s.value(r.field, target, missed)
                        .unwrap_or_else(|| tr("monitor.unavailable").to_owned()),
                );
                ui.end_row();
            }
        });
}

pub fn capabilities(ui: &mut egui::Ui, hw: Option<&Hardware>, active: Option<u64>) {
    let Some(hardware) = hw else {
        ui.label(theme::dim(tr("monitor.unavailable")));
        return;
    };
    egui::Grid::new("adapter-features")
        .min_col_width(170.0)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            for (key, value) in hardware.capabilities(active) {
                ui.label(tr(key));
                ui.label(value);
                ui.end_row();
            }
        });
    if hardware.hybrid_adapters() {
        ui.label(theme::dim(tr("monitor.hybrid_hint")).size(12.5));
    }
}

/// A small key above the chart, so the lines are named without covering the data.
fn chart_key(ui: &mut egui::Ui, pal: &theme::Palette, selected: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let entry = |ui: &mut egui::Ui, color: egui::Color32, dashed: bool, name: &str| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 14.0), egui::Sense::hover());
            let y = rect.center().y;
            let stroke = egui::Stroke::new(1.75_f32, color);
            if dashed {
                for x in [0.0, 8.0, 16.0] {
                    ui.painter().line_segment(
                        [egui::pos2(rect.left() + x, y), egui::pos2(rect.left() + x + 5.0, y)],
                        stroke,
                    );
                }
            } else {
                ui.painter().line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    stroke,
                );
            }
            ui.label(theme::dim(name).size(12.5));
            ui.add_space(12.0);
        };
        entry(ui, pal.chart[0], false, tr("monitor.frame"));
        entry(ui, pal.target, true, tr("monitor.target"));
        if selected {
            entry(ui, pal.chart[1], false, tr("monitor.selected"));
        }
    });
}

fn frame_chart(
    ui: &mut egui::Ui,
    history: &VecDeque<Row>,
    config: &MonitorSettings,
    selected: &mut Option<u64>,
) {
    headings(ui, Card::Chart);
    let end = history.back().map_or(0.0, |r| r.t_s);
    let window = f64::from(config.chart_seconds.clamp(10, 120));
    let timeline = recent(history, config.chart_seconds)
        .filter(|r| r.frame_ms.is_some())
        .collect::<Vec<_>>();
    let target = f64::from(config.target_ms.clamp(1.0, 250.0));
    let automatic = timeline
        .iter()
        .filter_map(|r| r.frame_ms)
        .map(f64::from)
        .fold(target, f64::max)
        * 1.12;
    let max_y = config
        .fixed_scale_ms
        .map_or(automatic, |ms| f64::from(ms.max(1)));
    let pal = theme::palette(ui.visuals());
    chart_key(ui, &pal, selected.is_some());
    let plot = Plot::new("live-frame-chart")
        .height(168.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_grid([false, true])
        .x_axis_label(tr("monitor.axis_seconds"))
        .y_axis_label("ms")
        .show(ui, |p| {
            p.set_plot_bounds(PlotBounds::from_min_max([-window, 0.0], [0.25, max_y]));
            p.hline(
                HLine::new(tr("monitor.target"), target)
                    .color(pal.target)
                    .style(egui_plot::LineStyle::dashed_dense()),
            );
            let points: PlotPoints = timeline
                .iter()
                .filter_map(|r| Some([r.t_s - end, f64::from(r.frame_ms?)]))
                .collect();
            p.line(
                Line::new(tr("monitor.frame"), points)
                    .color(pal.chart[0])
                    .width(1.75_f32),
            );
            if let Some(row) = selected.and_then(|n| timeline.iter().find(|r| r.number == n)) {
                p.points(
                    egui_plot::Points::new(
                        tr("monitor.selected"),
                        vec![[row.t_s - end, f64::from(row.frame_ms.unwrap_or_default())]],
                    )
                    .color(pal.chart[1])
                    .radius(4.0_f32),
                );
            }
        });
    if plot.response.clicked()
        && let Some(target) = plot.response.interact_pointer_pos()
    {
        let x = plot.transform.value_from_position(target).x + end;
        *selected = timeline
            .iter()
            .min_by(|a, b| (a.t_s - x).abs().total_cmp(&(b.t_s - x).abs()))
            .map(|p| p.number);
    }
    if let Some(r) = selected.and_then(|n| timeline.iter().find(|r| r.number == n)) {
        ui.label(
            theme::dim(trf(
                "monitor.inspect",
                &[
                    ("frame", &r.number.to_string()),
                    ("period", &format!("{:.2}", r.frame_ms.unwrap_or_default())),
                    (
                        "cpu",
                        &r.cpu_ms.map_or_else(
                            || tr("monitor.unavailable").into(),
                            |v| format!("{v:.2} ms"),
                        ),
                    ),
                    (
                        "gpu",
                        &r.gpu_ms.map_or_else(
                            || tr("monitor.unavailable").into(),
                            |v| format!("{v:.2} ms"),
                        ),
                    ),
                ],
            ))
            .size(12.5),
        );
    } else {
        ui.label(theme::dim(tr("monitor.inspect_help")).size(12.5));
    }
}

fn activity(ui: &mut egui::Ui, history: &VecDeque<Row>) {
    headings(ui, Card::Activity);
    let frames = history
        .iter()
        .rev()
        .take(28)
        .filter_map(|r| Some((r, r.frame_ms?)))
        .collect::<Vec<_>>();
    let Some((last, _)) = frames.first() else {
        return;
    };
    let start = frames
        .last()
        .map_or(last.t_s, |(r, ms)| r.t_s - f64::from(*ms) / 1000.0);
    let total = (last.t_s - start).max(0.001);
    let colors = theme::palette(ui.visuals()).chart;
    for (lane, label) in ["monitor.interval", "monitor.cpu", "monitor.gpu"]
        .into_iter()
        .enumerate()
    {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(128.0, 16.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().rect_filled(swatch, 2.0, colors[lane]);
                    ui.label(theme::dim(tr(label)).size(12.0));
                },
            );
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
            let x =
                |t: f64| rect.left() + rect.width() * ((t - start) / total).clamp(0.0, 1.0) as f32;
            for (r, period) in &frames {
                let width = [Some(*period), r.cpu_ms, r.gpu_ms][lane];
                if let Some(width) = width {
                    let left = r.t_s - f64::from(*period) / 1000.0;
                    let end = (left + f64::from(width) / 1000.0).min(r.t_s);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(x(left), rect.top()),
                            egui::pos2((x(end) - 1.0).max(x(left)), rect.bottom()),
                        ),
                        1,
                        colors[lane],
                    );
                }
            }
        });
    }
    ui.label(theme::dim(tr("monitor.activity_help")).size(12.5));
}

fn stages(ui: &mut egui::Ui, stages: &[Stage]) {
    headings(ui, Card::Stages);
    if stages.is_empty() {
        ui.label(theme::dim(tr("monitor.unavailable")));
        return;
    }
    let sum = stages.iter().map(|s| s.ms).sum::<f64>().max(0.001);
    let colors = theme::palette(ui.visuals()).chart;
    let (area, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
    let mut x = area.left();
    for (i, stage) in stages.iter().enumerate() {
        let right = x + area.width() * (stage.ms / sum) as f32;
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, area.top()),
                egui::pos2((right - 1.0).max(x), area.bottom()),
            ),
            1,
            colors[i % colors.len()],
        );
        x = right;
    }
    ui.add_space(6.0);
    egui::Grid::new("stage-breakdown")
        .min_col_width(115.0)
        .spacing([16.0, 5.0])
        .show(ui, |ui| {
            for key in ["monitor.stage", "monitor.stage_ms", "monitor.stage_delta"] {
                ui.label(theme::dim(tr(key)));
            }
            ui.end_row();
            for (i, stage) in stages.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, 2.0, colors[i % colors.len()]);
                    ui.label(&stage.name);
                });
                ui.monospace(format!("{:.2} ms", stage.ms));
                ui.monospace(
                    stage
                        .delta_ms
                        .map_or_else(|| "·".into(), |d| format!("{d:+.2} ms")),
                );
                ui.end_row();
            }
        });
}

fn drop_reasons(ui: &mut egui::Ui, s: &Summary) {
    let mut reasons = [
        (Metric::PipelineDrops, "monitor.drop_pipeline"),
        (Metric::IndirectDrops, "monitor.drop_indirect"),
        (Metric::IndirectBudgetDrops, "monitor.drop_budget"),
    ]
    .into_iter()
    .filter_map(|(m, key)| s.metric(m).filter(|&n| n != 0).map(|n| (n, tr(key))))
    .collect::<Vec<_>>();
    reasons.sort_unstable_by_key(|(n, _)| std::cmp::Reverse(*n));
    egui::CollapsingHeader::new(tr("monitor.reasons")).show(ui, |ui| {
        if reasons.is_empty() {
            ui.label(theme::dim(tr("monitor.no_reasons")));
        }
        for (n, reason) in reasons {
            ui.label(format!("{n}  {reason}"));
        }
    });
}

fn customize(ui: &mut egui::Ui, settings: &mut OpsSettings, release_engine: bool) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.checkbox(&mut settings.panel, tr("panel.enable"));
        ui.label(theme::dim(tr("panel.help")).size(12.5));
        if release_engine {
            ui.add_space(4.0);
            ui.label(theme::dim(tr("panel.needs_diagnostics")).size(12.5));
        }
    });
    ui.add_space(9.0);
    let cfg = &mut settings.monitor;
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            for (p, key) in [
                (Preset::Minimal, "monitor.minimal"),
                (Preset::Standard, "monitor.standard"),
                (Preset::Everything, "monitor.everything"),
            ] {
                if ui.button(tr(key)).clicked() {
                    cfg.cards = MonitorSettings::preset(p);
                }
            }
            if ui.button(tr("monitor.reset")).clicked() {
                *cfg = MonitorSettings::default();
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("monitor.window"))
                .on_hover_text(tr("monitor.window_help"));
            ui.add(
                egui::DragValue::new(&mut cfg.chart_seconds)
                    .range(10..=120)
                    .suffix(" s"),
            );
            ui.label(tr("monitor.scale"))
                .on_hover_text(tr("monitor.scale_help"));
            let mut fixed = cfg.fixed_scale_ms.is_some();
            if ui.checkbox(&mut fixed, tr("monitor.fixed")).changed() {
                cfg.fixed_scale_ms = fixed.then_some(35);
            }
            if let Some(max) = &mut cfg.fixed_scale_ms {
                ui.add(egui::DragValue::new(max).range(1..=500).suffix(" ms"));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("monitor.target"))
                .on_hover_text(tr("monitor.target_help"));
            for fps in [30, 60] {
                let ms = 1000.0 / fps as f32;
                if ui
                    .selectable_label((cfg.target_ms - ms).abs() < 0.001, format!("{fps} FPS"))
                    .clicked()
                {
                    cfg.target_ms = ms;
                }
            }
            ui.add(
                egui::DragValue::new(&mut cfg.target_ms)
                    .range(1.0..=250.0)
                    .speed(0.1)
                    .max_decimals(2)
                    .suffix(" ms"),
            );
            ui.label(tr("monitor.csv_length"))
                .on_hover_text(tr("monitor.csv_help"));
            ui.add(
                egui::DragValue::new(&mut cfg.csv_seconds)
                    .range(10..=120)
                    .suffix(" s"),
            );
        });
        rate_picker(ui, "monitor-rate", &mut cfg.rate);
        ui.checkbox(&mut cfg.live_sensors, tr("monitor.sensors"))
            .on_hover_text(tr("monitor.sensors_help"));
    });
    ui.add_space(9.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("monitor.cards")));
        ui.label(theme::dim(tr("monitor.cards_help")));
        let mut card_move = None;
        let length = cfg.cards.len();
        for (i, c) in cfg.cards.iter_mut().enumerate() {
            ui.push_id(c.card, |ui| {
                ui.horizontal(|ui| {
                    if let Some(j) = move_buttons(ui, i, length) {
                        card_move = Some((i, j));
                    }
                    ui.checkbox(&mut c.visible, c.card.label());
                });
                if !c.rows.is_empty() {
                    egui::CollapsingHeader::new(tr("monitor.rows")).show(ui, |ui| {
                        let len = c.rows.len();
                        let mut row_move = None;
                        for (j, r) in c.rows.iter_mut().enumerate() {
                            ui.push_id(r.field, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some(k) = move_buttons(ui, j, len) {
                                        row_move = Some((j, k));
                                    }
                                    ui.checkbox(&mut r.visible, r.field.label());
                                });
                            });
                        }
                        if let Some((i, j)) = row_move {
                            c.rows.swap(i, j);
                        }
                    });
                }
            });
        }
        if let Some((i, j)) = card_move {
            cfg.cards.swap(i, j);
        }
    });
}

fn move_buttons(ui: &mut egui::Ui, index: usize, len: usize) -> Option<usize> {
    let up = crate::core::widgets::chevron_button(ui, true, index > 0)
        .on_hover_text(tr("monitor.move_up"))
        .clicked();
    let down = crate::core::widgets::chevron_button(ui, false, index + 1 < len)
        .on_hover_text(tr("monitor.move_down"))
        .clicked();
    if up {
        Some(index - 1)
    } else {
        down.then_some(index + 1)
    }
}

pub fn rate_picker(ui: &mut egui::Ui, id: &str, rate: &mut Rate) {
    ui.horizontal(|ui| {
        ui.label(tr("monitor.refresh"))
            .on_hover_text(tr("monitor.refresh_help"));
        egui::ComboBox::from_id_salt(id)
            .selected_text(rate.label())
            .show_ui(ui, |ui| {
                for value in Rate::ALL {
                    ui.selectable_value(rate, *value, value.label());
                }
            })
            .response
            .on_hover_text(tr("monitor.refresh_help"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_counts_get_separators() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(1687104), "1,687,104");
    }

    #[test]
    fn live_history_uses_timestamps_and_does_not_duplicate_frames() {
        let mut d = Demo::new();
        let mut state = MonitorState::default();
        for _ in 0..4 {
            d.advance(87);
            let snap = d.writer.snapshot().unwrap();
            state.ingest(snap.clone(), d.qpc);
            state.ingest(snap, d.qpc);
        }
        assert_eq!(state.history.len(), 348);
        let sum = state.summary.as_ref().unwrap();
        assert!(sum.fps.unwrap() > 70.0);
        assert!(sum.row.unwrap().gpu_estimate().unwrap() > 75.0);
        assert!(sum.stages[0].delta_ms.is_some());
        d.writer.present_rejected(1, true);
        state.ingest(d.writer.snapshot().unwrap(), d.qpc);
        let sum = state.summary.as_ref().unwrap();
        assert_eq!(sum.state, Some(PresentState::Occluded));
        assert_eq!(sum.occluded, Some(1));
        assert_eq!(sum.fps, None);
        assert_eq!(state.history.len(), 348);
    }

    #[test]
    fn csv_keeps_only_requested_history_and_leaves_missing_values_empty() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("frames.csv");
        let mut history = VecDeque::new();
        for t in 0..=12 {
            history.push_back(Row {
                number: t,
                qpc: t,
                t_s: t as f64,
                frame_ms: Some(16.),
                cpu_ms: None,
                gpu_ms: None,
                idle_cpu_ms: None,
                present_ms: None,
            });
        }
        assert!(export_csv(&path, &history, 9).is_err());
        export_csv(&path, &history, 10).unwrap();
        let csv = std::fs::read_to_string(path).unwrap();
        assert!(csv.contains("2,2.000000,16.000,,,,"));
        assert_eq!(csv.lines().count(), 12);
    }
}
