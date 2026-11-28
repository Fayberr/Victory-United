//! `--ui-demo --capture <dir>`: render every screen to `<dir>/<dark|light>/<name>.png` through eframe's screenshot
//! support, then close by itself. A watchdog thread ends the process if anything gets stuck, so no window
//! can stay open. Only used for review screenshots; normal runs never touch this.
use crate::{app::Tab, core};
use eframe::egui;
use std::path::PathBuf;

const SETTLE_FRAMES: u32 = 6;
const HARD_TIMEOUT_SECS: u64 = 90;

struct Step {
    name: &'static str,
    tab: Tab,
    screen: Option<core::Screen>,
    /// Scroll position of the page (screens taller than the window get a second shot).
    scroll: f32,
}

pub struct Capture {
    dir: PathBuf,
    steps: Vec<Step>,
    idx: usize,
    waited: u32,
    requested: bool,
    failed: bool,
}

impl Capture {
    pub fn new(dir: PathBuf) -> Self {
        let failed = ["dark", "light"].iter().any(|name| {
            std::fs::create_dir_all(dir.join(name)).is_err()
        });
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(HARD_TIMEOUT_SECS));
            eprintln!("capture: hard timeout, exiting");
            std::process::exit(3);
        });
        let s = |name, tab, screen, scroll| Step { name, tab, screen, scroll };
        let steps = vec![
            s("00-first-run", Tab::Play, Some(core::Screen::FirstRun), 0.0),
            s("01-play-warmup", Tab::Play, Some(core::Screen::WarmUp), 0.0),
            s("02-play", Tab::Play, Some(core::Screen::Main), 0.0),
            s("02-play-unsupported", Tab::Play, Some(core::Screen::Unsupported), 0.0),
            s("03-settings", Tab::Settings, Some(core::Screen::Main), 0.0),
            s("03-settings-agent-off", Tab::Settings, Some(core::Screen::Main), 435.0),
            s("03-settings-agent-on", Tab::Settings, Some(core::Screen::Main), 580.0),
            s("03-settings-backups", Tab::Settings, Some(core::Screen::Main), 640.0),
            s("03-settings-storage", Tab::Settings, Some(core::Screen::Main), 1065.0),
            s("03-settings-lower", Tab::Settings, Some(core::Screen::Main), 1530.0),
            s("03-settings-engine", Tab::Settings, Some(core::Screen::Main), 4640.0),
            s("03-saves", Tab::Saves, Some(core::Screen::Idle), 0.0),
            s("04-flags", Tab::Flags, Some(core::Screen::Main), 0.0),
            s("05-monitor", Tab::Monitor, Some(core::Screen::Main), 0.0),
            s("05-monitor-detail", Tab::Monitor, Some(core::Screen::Main), 500.0),
            s("05-monitor-hardware", Tab::Monitor, Some(core::Screen::Main), 1_240.0),
            s("05-monitor-customize", Tab::Monitor, Some(core::Screen::Main), 0.0),
            s("05-overlay", Tab::Monitor, Some(core::Screen::Main), 0.0),
            s("06-updates", Tab::Updates, Some(core::Screen::Main), 0.0),
            s("07-developer", Tab::Dev, Some(core::Screen::Main), 0.0),
            s("07-profiler", Tab::Dev, Some(core::Screen::Main), 0.0),
            s("07-profiler-schedule", Tab::Dev, Some(core::Screen::Main), 420.0),
            s("08-bug-report", Tab::Report, Some(core::Screen::Main), 0.0),
            s("09-about", Tab::About, Some(core::Screen::Main), 0.0),
        ];
        Self { dir, steps, idx: 0, waited: 0, requested: false, failed }
    }

    pub fn page(&self) -> &'static str {
        self.steps[self.idx % self.steps.len()].name
    }

    /// Called at the start of every frame. Returns true when finished (the window is told to close).
    pub fn drive(&mut self, ctx: &egui::Context, tab: &mut Tab, core: &mut core::CoreState) -> bool {
        if self.idx >= 2 * self.steps.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            if self.failed {
                std::process::exit(2);
            }
            return true;
        }
        ctx.request_repaint(); // capture mode only
        let (name, color) = if self.idx < self.steps.len() {
            ("dark", egui::Theme::Dark)
        } else {
            ("light", egui::Theme::Light)
        };
        ctx.set_theme(color);
        let step = &self.steps[self.idx % self.steps.len()];
        *tab = step.tab;
        core.force_screen = step.screen;
        core.page_scroll = Some(step.scroll);
        crate::agent::ui::capture(core, step.name == "03-settings-agent-on");
        // Collect a finished screenshot.
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let path = self.dir.join(name).join(format!("{}.png", step.name));
            let [w, h] = img.size;
            if image::save_buffer(&path, img.as_raw(), w as u32, h as u32, image::ColorType::Rgba8).is_err() {
                eprintln!("capture: cannot write {}", path.display());
                self.failed = true;
            }
            self.idx += 1;
            self.waited = 0;
            self.requested = false;
            return false;
        }
        self.waited += 1;
        if self.waited >= SETTLE_FRAMES && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        false
    }
}
