//! Monitor tab: reads the engine's stats block once per second (see `victory-stats`).
use crate::{app::Shared, i18n::{tr, trf}, ops::OpsState, paths, theme};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use std::{collections::VecDeque, time::{Duration, Instant}};
use victory_stats::{Reader, Snapshot};

/// How much history the CSV export keeps.
const KEEP_SECS: f64 = 60.0;

/// Numbers derived from one snapshot (and the one before it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Average over the frames in the ring.
    pub fps_avg: f32,
    /// 1 percent low: fps of the slowest 1 percent of the frames in the ring.
    pub fps_low1: f32,
    pub ms_avg: f32,
    pub ms_worst: f32,
    /// Mean CPU preparation and GPU time per frame in ms (0 when the engine does not know).
    pub cpu_ms: f32,
    pub gpu_ms: f32,
    /// Frames per second from the frame counter between two reads (what the overlay shows).
    pub fps_counter: Option<f32>,
    pub frames: u64,
    pub vram_used: u64,
    pub vram_budget: u64,
    pub ram: u64,
    pub draws: u64,
    pub dispatches: u64,
    pub pipelines_building: u64,
    pub build: String,
    pub frame_ms: Vec<f32>,
    pub cpu_ms_series: Vec<f32>,
    pub gpu_ms_series: Vec<f32>,
}

/// fps from the frame counter and timestamps of two snapshots of the same block.
pub fn fps_between(prev: &Snapshot, now: &Snapshot) -> Option<f32> {
    let frames = now.frames.checked_sub(prev.frames)?;
    let ticks = now.last_qpc.checked_sub(prev.last_qpc)?;
    (frames > 0 && ticks > 0 && now.qpc_freq > 0).then(|| (frames as f64 * now.qpc_freq as f64 / ticks as f64) as f32)
}

pub fn summarize(now: &Snapshot, prev: Option<&Snapshot>) -> Summary {
    let frame_ms = now.frame_times_ms();
    let mut sorted = frame_ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    let mean = |v: &[f32]| if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
    let ms_avg = mean(&sorted);
    // The slowest 1 percent (at least one frame): average of those frame times, as fps.
    let worst = ((n as f32 * 0.01).ceil() as usize).clamp(1, n.max(1));
    let low_ms = if n == 0 { 0.0 } else { mean(&sorted[n - worst..]) };
    let skip = now.frames_recent.len().saturating_sub(frame_ms.len());
    let to_ms = |f: fn(&victory_stats::FrameSample) -> u32| -> Vec<f32> {
        now.frames_recent.iter().skip(skip).map(|s| f(s) as f32 / 1000.0).collect()
    };
    let cpu = to_ms(|s| s.cpu_us);
    let gpu = to_ms(|s| s.gpu_us);
    Summary {
        fps_avg: if ms_avg > 0.0 { 1000.0 / ms_avg } else { 0.0 },
        fps_low1: if low_ms > 0.0 { 1000.0 / low_ms } else { 0.0 },
        ms_avg,
        ms_worst: sorted.last().copied().unwrap_or(0.0),
        cpu_ms: mean(&cpu),
        gpu_ms: mean(&gpu),
        fps_counter: prev.and_then(|p| fps_between(p, now)),
        frames: now.frames,
        vram_used: now.vram_used,
        vram_budget: now.vram_budget,
        ram: now.ram_resident,
        draws: now.draws,
        dispatches: now.dispatches,
        pipelines_building: now.pipelines_building,
        build: now.build.clone(),
        frame_ms,
        cpu_ms_series: cpu,
        gpu_ms_series: gpu,
    }
}

/// One frame kept for the CSV export.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub t_s: f64,
    pub frame_ms: f32,
    pub cpu_ms: f32,
    pub gpu_ms: f32,
}

pub fn csv(rows: &VecDeque<Row>) -> String {
    let mut out = String::from("time_s,frame_ms,cpu_ms,gpu_ms\n");
    for r in rows {
        out.push_str(&format!("{:.4},{:.3},{:.3},{:.3}\n", r.t_s, r.frame_ms, r.cpu_ms, r.gpu_ms));
    }
    out
}

#[derive(Default)]
pub struct MonitorState {
    reader: Option<(u32, Reader)>,
    prev: Option<Snapshot>,
    last_read: Option<Instant>,
    pub summary: Option<Summary>,
    pub history: VecDeque<Row>,
    first_qpc: Option<u64>,
    /// Frame counter value up to which `history` is complete.
    seen_frames: u64,
    pub export_note: Option<String>,
}

impl MonitorState {
    /// The game is gone: drop the mapping and the numbers.
    pub fn forget(&mut self) {
        if self.reader.is_some() || self.summary.is_some() {
            *self = Self { export_note: self.export_note.take(), ..Default::default() };
        }
    }

    /// Reads the block when a second has passed since the last read. Returns true when it read.
    pub fn poll(&mut self, pid: Option<u32>, now: Instant) -> bool {
        let Some(pid) = pid else { return false };
        if self.last_read.is_some_and(|t| now.duration_since(t) < Duration::from_millis(900)) {
            return false;
        }
        self.last_read = Some(now);
        if self.reader.as_ref().map(|(p, _)| *p) != Some(pid) {
            self.reader = Reader::open(pid).map(|r| (pid, r));
            self.prev = None;
            self.history.clear();
            self.seen_frames = 0;
            self.first_qpc = None;
        }
        let Some(snap) = self.reader.as_ref().and_then(|(_, r)| r.snapshot()) else {
            self.summary = None;
            return true;
        };
        self.ingest(snap);
        true
    }

    fn ingest(&mut self, snap: Snapshot) {
        let freq = snap.qpc_freq.max(1) as f64;
        let first = *self.first_qpc.get_or_insert_with(|| snap.frames_recent.first().map(|s| s.qpc).unwrap_or(0));
        let fresh = (snap.frames.saturating_sub(self.seen_frames) as usize).min(snap.frames_recent.len());
        let start = snap.frames_recent.len() - fresh;
        for i in start..snap.frames_recent.len() {
            let s = snap.frames_recent[i];
            let before = if i > 0 { Some(snap.frames_recent[i - 1].qpc) } else { self.history.back().map(|r| first + (r.t_s * freq) as u64) };
            let Some(b) = before else { continue };
            self.history.push_back(Row {
                t_s: s.qpc.saturating_sub(first) as f64 / freq,
                frame_ms: (s.qpc.saturating_sub(b) as f64 * 1000.0 / freq) as f32,
                cpu_ms: s.cpu_us as f32 / 1000.0,
                gpu_ms: s.gpu_us as f32 / 1000.0,
            });
        }
        self.seen_frames = snap.frames;
        if let Some(last) = self.history.back().map(|r| r.t_s) {
            while self.history.front().is_some_and(|r| last - r.t_s > KEEP_SECS) {
                self.history.pop_front();
            }
        }
        self.summary = Some(summarize(&snap, self.prev.as_ref()));
        self.prev = Some(snap);
    }

    /// A believable fake stream for screenshots: about 90 fps with a few hitches.
    pub fn demo() -> Self {
        let mut w = victory_stats::Writer::in_memory(4242, "a1b2c3d4 release");
        let freq = 1_000_000_000u64;
        let mut t = 0u64;
        for i in 0..2000u64 {
            let wave = 11.0 + 1.6 * ((i as f64) / 23.0).sin() + 0.7 * ((i as f64) / 5.3).sin();
            let spike = if i % 311 == 0 { 14.0 } else { 0.0 };
            let ms = wave + spike;
            t += (ms * 1e6) as u64;
            w.frame(t, ((ms * 0.62) * 1000.0) as u32, ((ms * 0.81) * 1000.0) as u32);
        }
        w.set(victory_stats::Slow::VramUsed, 9_400 << 20);
        w.set(victory_stats::Slow::VramBudget, 11_240 << 20);
        w.set(victory_stats::Slow::RamResident, 13_800 << 20);
        w.set(victory_stats::Slow::Draws, 1_920_000);
        w.set(victory_stats::Slow::Dispatches, 740_000);
        w.set(victory_stats::Slow::PipelinesBuilding, 3);
        let mut snap = w.snapshot().expect("demo block is valid");
        snap.qpc_freq = freq;
        let mut prev = snap.clone();
        prev.frames -= 90;
        prev.last_qpc -= 1_000_000_000;
        let mut s = Self::default();
        let sum = summarize(&snap, Some(&prev));
        s.summary = Some(sum);
        s.ingest_history_only(&snap);
        s
    }

    fn ingest_history_only(&mut self, snap: &Snapshot) {
        let mut copy = Self::default();
        copy.ingest(snap.clone());
        self.history = copy.history;
    }
}

fn mb(bytes: u64) -> String {
    if bytes >= 1 << 30 { format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64) } else { format!("{} MB", bytes >> 20) }
}

fn big_number(ui: &mut egui::Ui, value: String, label: &str) {
    ui.vertical(|ui| {
        ui.label(theme::strong(value).size(26.0));
        ui.label(theme::dim(label).size(12.5));
    });
}

pub fn show(ops: &mut OpsState, shared: &Shared, ui: &mut egui::Ui) {
    let before = ops.cfg.clone();
    ui.horizontal(|ui| {
        ui.label(theme::strong(tr("monitor.title")).size(24.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ops.monitor.export_button(ui, shared.game_pid.is_some());
        });
    });
    if let Some(note) = &ops.monitor.export_note {
        ui.label(theme::dim(note.clone()).size(12.5));
    }
    ui.add_space(10.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        match (&ops.monitor.summary, shared.game_pid) {
            (_, None) => waiting(ui, tr("monitor.not_running")),
            (None, Some(_)) => waiting(ui, tr("monitor.waiting")),
            (Some(s), Some(_)) => live(ui, s),
        }
        ui.add_space(14.0);
        overlay_card(ui, ops);
    });
    ops.save_if_changed(&before);
}

fn waiting(ui: &mut egui::Ui, text: &str) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(text));
        ui.label(theme::dim(tr("monitor.waiting_help")));
    });
}

fn live(ui: &mut egui::Ui, s: &Summary) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            big_number(ui, format!("{:.0}", s.fps_avg), tr("monitor.fps"));
            ui.add_space(28.0);
            big_number(ui, format!("{:.0}", s.fps_low1), tr("monitor.fps_low"));
            ui.add_space(28.0);
            big_number(ui, format!("{:.1} ms", s.ms_avg), tr("monitor.frame_time"));
            ui.add_space(28.0);
            big_number(ui, format!("{:.1} ms", s.cpu_ms), tr("monitor.cpu"));
            ui.add_space(28.0);
            big_number(ui, format!("{:.1} ms", s.gpu_ms), tr("monitor.gpu"));
        });
        ui.add_space(10.0);
        let frames = trf("monitor.frames_shown", &[("n", &s.frames.to_string()), ("build", &s.build)]);
        ui.label(theme::dim(frames).size(12.5));
    });
    ui.add_space(12.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("monitor.graph")));
        let colors = theme::palette(ui.visuals()).chart;
        let line = |name: &str, data: &[f32], color| {
            let pts: PlotPoints = data.iter().enumerate().map(|(i, v)| [i as f64, *v as f64]).collect();
            Line::new(name.to_string(), pts).color(color).width(1.4_f32)
        };
        Plot::new("frame_graph")
            .height(190.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .include_y(0.0)
            .show_grid([false, true])
            .legend(egui_plot::Legend::default())
            .x_axis_label(tr("monitor.axis_frames"))
            .y_axis_label(tr("monitor.axis_ms"))
            .show(ui, |p| {
                p.line(line(tr("monitor.frame_time"), &s.frame_ms, colors[0]));
                p.line(line(tr("monitor.cpu"), &s.cpu_ms_series, colors[1]));
                p.line(line(tr("monitor.gpu"), &s.gpu_ms_series, colors[2]));
            });
    });
    ui.add_space(12.0);
    let card = theme::card(ui);
    ui.columns(2, |cols| {
        card.show(&mut cols[0], |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(tr("monitor.memory")));
            ui.add_space(6.0);
            let frac = if s.vram_budget > 0 { s.vram_used as f32 / s.vram_budget as f32 } else { 0.0 };
            ui.label(trf("monitor.vram", &[("used", &mb(s.vram_used)), ("budget", &mb(s.vram_budget))]));
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_height(8.0));
            ui.add_space(6.0);
            ui.label(trf("monitor.ram", &[("used", &mb(s.ram))]));
        });
        card.show(&mut cols[1], |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::strong(tr("monitor.work")));
            ui.add_space(6.0);
            ui.label(trf("monitor.draws", &[("n", &s.draws.to_string())]));
            ui.label(trf("monitor.dispatches", &[("n", &s.dispatches.to_string())]));
            ui.label(trf("monitor.pipelines", &[("n", &s.pipelines_building.to_string())]));
        });
    });
}

fn overlay_card(ui: &mut egui::Ui, ops: &mut OpsState) {
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::strong(tr("overlay.title")));
        ui.add_space(4.0);
        ui.checkbox(&mut ops.cfg.overlay, tr("overlay.enable"));
        ui.horizontal(|ui| {
            ui.label(tr("overlay.corner"));
            egui::ComboBox::from_id_salt("overlay_corner").selected_text(ops.cfg.overlay_corner.label()).show_ui(ui, |ui| {
                for c in super::settings::Corner::ALL {
                    ui.selectable_value(&mut ops.cfg.overlay_corner, c, c.label());
                }
            });
        });
        ui.label(theme::dim(tr("overlay.help")).size(12.5));
    });
}

impl MonitorState {
    fn export_button(&mut self, ui: &mut egui::Ui, running: bool) {
        let enabled = !self.history.is_empty();
        if ui.add_enabled(enabled, egui::Button::new(tr("monitor.export"))).clicked() {
            let dir = paths::reports_dir();
            let file = dir.join(format!("frames-{}.csv", super::stamp()));
            self.export_note = Some(match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, csv(&self.history))) {
                Ok(()) => trf("monitor.exported", &[("file", &file.display().to_string())]),
                Err(e) => trf("monitor.export_failed", &[("error", &e.to_string())]),
            });
        }
        let _ = running;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use victory_stats::Writer;

    fn block(frames: u64, step_ns: u64) -> Snapshot {
        let mut w = Writer::in_memory(1, "t");
        for i in 1..=frames {
            w.frame(i * step_ns, 2000, 3000);
        }
        let mut s = w.snapshot().unwrap();
        s.qpc_freq = 1_000_000_000;
        s
    }

    #[test]
    fn summary_of_a_steady_stream() {
        let s = summarize(&block(300, 10_000_000), None);
        assert!((s.fps_avg - 100.0).abs() < 0.1);
        assert!((s.fps_low1 - 100.0).abs() < 0.1);
        assert!((s.cpu_ms - 2.0).abs() < 0.01 && (s.gpu_ms - 3.0).abs() < 0.01);
        assert_eq!(s.frame_ms.len(), 299);
    }

    #[test]
    fn low_one_percent_sees_the_hitches() {
        let mut w = Writer::in_memory(1, "t");
        let mut t = 0;
        for i in 0..500u64 {
            t += if i % 100 == 50 { 100_000_000 } else { 10_000_000 };
            w.frame(t, 0, 0);
        }
        let mut snap = w.snapshot().unwrap();
        snap.qpc_freq = 1_000_000_000;
        let s = summarize(&snap, None);
        assert!(s.fps_low1 < 15.0, "{}", s.fps_low1);
        assert!(s.fps_avg > 70.0);
    }

    #[test]
    fn counter_fps_between_reads() {
        let (a, b) = (block(100, 10_000_000), block(200, 10_000_000));
        assert!((fps_between(&a, &b).unwrap() - 100.0).abs() < 0.1);
        assert!(fps_between(&b, &a).is_none());
        assert!(fps_between(&a, &a).is_none());
    }

    #[test]
    fn history_is_kept_for_a_minute_and_exported() {
        let mut m = MonitorState::default();
        // 90 s at 10 fps, delivered in seconds worth of frames.
        let mut w = Writer::in_memory(1, "t");
        let mut t = 0u64;
        for second in 0..90 {
            for _ in 0..10 {
                t += 100_000_000;
                w.frame(t, 1000, 2000);
            }
            let mut snap = w.snapshot().unwrap();
            snap.qpc_freq = 1_000_000_000;
            m.ingest(snap);
            let _ = second;
        }
        let span = m.history.back().unwrap().t_s - m.history.front().unwrap().t_s;
        assert!((59.0..=60.1).contains(&span), "{span}");
        assert!(m.history.iter().all(|r| (r.frame_ms - 100.0).abs() < 0.01));
        let text = csv(&m.history);
        assert!(text.starts_with("time_s,frame_ms,cpu_ms,gpu_ms\n"));
        assert_eq!(text.lines().count(), m.history.len() + 1);
    }

    #[test]
    fn missing_game_gives_no_data() {
        let mut m = MonitorState::default();
        assert!(!m.poll(None, Instant::now()));
        assert!(m.poll(Some(u32::MAX - 5), Instant::now()));
        assert!(m.summary.is_none());
    }
}

#[cfg(test)]
mod cost {
    use super::*;

    /// Cost of one once-a-second read: copy 512 frames out of the block, summarise, keep history.
    #[test]
    fn one_read_is_cheap() {
        let mut w = victory_stats::Writer::in_memory(1, "t");
        let mut m = MonitorState::default();
        let mut t = 0u64;
        let mut total = std::time::Duration::ZERO;
        let rounds = 200;
        for _ in 0..rounds {
            for _ in 0..90 {
                t += 11_000_000;
                w.frame(t, 5000, 8000);
            }
            let started = std::time::Instant::now();
            let mut snap = w.snapshot().unwrap();
            snap.qpc_freq = 1_000_000_000;
            m.ingest(snap);
            total += started.elapsed();
        }
        let per = total / rounds;
        eprintln!("one monitor read: {per:?}");
        assert!(per < std::time::Duration::from_millis(5), "{per:?}");
    }
}
