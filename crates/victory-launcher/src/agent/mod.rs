//! The agent interface: AI agents drive the launcher through the same functions the screens use.
//!
//! One `Backend` runs the tools. In the window it shares the game runner with the screens; in
//! `--headless` and `--mcp-stdio` it is the whole launcher. Tools run on the thread of the request, never
//! on the window's thread, so a minimised window cannot stall an agent. After a tool changed files the
//! backend raises a revision and wakes the window, which reads everything again.
//!
//! Nothing here polls: the server blocks on `accept`, headless mode blocks on a condition variable.
pub mod auth;
pub mod calls;
pub mod docs;
pub mod headless;
pub mod http;
pub mod mcp;
pub mod redact;
pub mod schema;
pub mod stdio;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod ui;

use crate::{
    core::{actions::Dirs, hw, runner::Runner},
    ops::monitor::MonitorState,
};
use anyhow::{Result, anyhow};
use eframe::egui;
use std::{
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use victory_update::Update;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A flag somebody can wait for.
#[derive(Default)]
pub struct Signal {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    pub fn set(&self) {
        *lock(&self.set) = true;
        self.changed.notify_all();
    }
    pub fn is_set(&self) -> bool {
        *lock(&self.set)
    }
    /// Blocks until the flag is set.
    pub fn wait(&self) {
        let mut set = lock(&self.set);
        while !*set {
            set = self
                .changed
                .wait(set)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    /// Blocks until the flag is set or `time` passed. True when set.
    pub fn wait_for(&self, time: Duration) -> bool {
        let set = lock(&self.set);
        let (set, _) = self
            .changed
            .wait_timeout_while(set, time, |set| !*set)
            .unwrap_or_else(PoisonError::into_inner);
        *set
    }
}

/// Keeps the window and a tool from writing settings at the same time. The window holds it for the
/// length of one frame, a tool for the length of one change.
#[derive(Default)]
pub struct Gate {
    held: Mutex<bool>,
    freed: Condvar,
}

impl Gate {
    pub fn enter(self: &Arc<Self>) -> GateGuard {
        let mut held = lock(&self.held);
        while *held {
            held = self
                .freed
                .wait(held)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *held = true;
        GateGuard(self.clone())
    }

    pub fn enter_within(self: &Arc<Self>, time: Duration) -> Option<GateGuard> {
        let held = lock(&self.held);
        let (mut held, _) = self
            .freed
            .wait_timeout_while(held, time, |held| *held)
            .unwrap_or_else(PoisonError::into_inner);
        if *held {
            return None;
        }
        *held = true;
        Some(GateGuard(self.clone()))
    }
}

pub struct GateGuard(Arc<Gate>);

impl Drop for GateGuard {
    fn drop(&mut self) {
        *lock(&self.0.held) = false;
        self.0.freed.notify_one();
    }
}

/// A long piece of work running in the background: a storage move or an engine install.
pub struct Job {
    pub kind: &'static str,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Set when the job ended. `Ok` holds a short sentence, `Err` the reason.
    pub result: Mutex<Option<Result<String, String>>>,
    pub finished: Signal,
}

pub struct Backend {
    pub dirs: Dirs,
    /// No window: `--headless` or `--mcp-stdio`.
    pub headless: bool,
    ctx: egui::Context,
    runner: Mutex<Runner>,
    revision: AtomicU64,
    pub gate: Arc<Gate>,
    hardware: OnceLock<hw::Hardware>,
    token: OnceLock<String>,
    closed: AtomicBool,
    pub monitor: Mutex<MonitorState>,
    job: Mutex<Option<Arc<Job>>>,
    /// The result of the last update check.
    pub update: Mutex<Option<Update>>,
    pub calls: calls::CallLog,
    /// Set when this process should end (headless only).
    pub stop: Signal,
    /// The shutdown tool asked; `stop` is set once the answer was sent.
    pub shutdown_requested: AtomicBool,
}

impl Backend {
    pub fn new(dirs: Dirs, headless: bool, ctx: egui::Context, runner: Runner) -> Arc<Self> {
        let calls = calls::CallLog::open(dirs.logs_dir().join("agent.log"));
        Arc::new(Self {
            dirs,
            headless,
            ctx,
            runner: Mutex::new(runner),
            revision: AtomicU64::new(0),
            gate: Default::default(),
            hardware: OnceLock::new(),
            token: OnceLock::new(),
            closed: AtomicBool::new(false),
            monitor: Default::default(),
            job: Mutex::new(None),
            update: Mutex::new(None),
            calls,
            stop: Signal::default(),
            shutdown_requested: AtomicBool::new(false),
        })
    }

    /// A backend of its own, for a process without a window. Nothing touches the graphics card here.
    pub fn standalone() -> Arc<Self> {
        Self::standalone_in(Dirs::real())
    }

    fn standalone_in(dirs: Dirs) -> Arc<Self> {
        let ctx = egui::Context::default();
        Self::new(dirs, true, ctx.clone(), Runner::new(ctx))
    }

    /// The runner, brought up to date.
    pub fn runner(&self) -> MutexGuard<'_, Runner> {
        let mut runner = lock(&self.runner);
        runner.drain();
        runner
    }

    /// The graphics card and the rest of the machine. The first call looks at the hardware.
    pub fn hardware(&self) -> &hw::Hardware {
        self.hardware.get_or_init(hw::probe)
    }

    /// The window already looked at the hardware.
    pub fn set_hardware(&self, hardware: &hw::Hardware) {
        let _ = self.hardware.set(hardware.clone());
    }

    pub fn has_hardware(&self) -> bool {
        self.hardware.get().is_some()
    }

    fn token(&self) -> Option<&str> {
        self.token.get().map(String::as_str)
    }

    fn ensure_open(&self) -> Result<()> {
        anyhow::ensure!(
            !self.closed.load(Ordering::Acquire),
            "Agent control has been turned off."
        );
        Ok(())
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Wakes the window, if there is one.
    pub fn wake(&self) {
        self.ctx.request_repaint();
    }

    /// Files changed: the window has to read them again.
    pub fn changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.wake();
    }

    /// Runs a change of settings, profiles or flags while the window is not writing them.
    pub fn edit<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let guard = self
            .gate
            .enter_within(Duration::from_secs(10))
            .ok_or_else(|| {
                anyhow!("The launcher window is busy, a dialog may be open. Try again.")
            })?;
        self.ensure_open()?;
        let out = work();
        self.changed();
        drop(guard);
        out
    }

    pub fn job(&self) -> Option<Arc<Job>> {
        lock(&self.job).clone()
    }

    pub fn job_running(&self) -> bool {
        self.job().is_some_and(|job| !job.finished.is_set())
    }

    /// Starts background work. Only one job runs at a time.
    pub fn start_job(
        self: &Arc<Self>,
        kind: &'static str,
        work: impl FnOnce(&Arc<Backend>, &Job) -> Result<String> + Send + 'static,
    ) -> Result<Arc<Job>> {
        let mut slot = lock(&self.job);
        if slot.as_ref().is_some_and(|job| !job.finished.is_set()) {
            return Err(anyhow!("Another storage or engine job is still running."));
        }
        let job = Arc::new(Job {
            kind,
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            result: Mutex::new(None),
            finished: Signal::default(),
        });
        *slot = Some(job.clone());
        let (backend, running) = (self.clone(), job.clone());
        std::thread::spawn(move || {
            let result = work(&backend, &running).map_err(|e| format!("{e:#}"));
            *lock(&running.result) = Some(result);
            running.finished.set();
            backend.changed();
        });
        Ok(job)
    }

    /// Before the process ends: pause a running job (a move can be resumed) and wait for it.
    pub fn finish_jobs(&self) {
        if let Some(job) = self.job() {
            job.cancel.store(true, Ordering::Relaxed);
            job.finished.wait_for(Duration::from_secs(30));
        }
    }
}

/// What the window keeps for the agent interface. Costs nothing while it is switched off.
#[derive(Default)]
pub struct Controller {
    live: Option<Live>,
    finishing: Option<Arc<Backend>>,
    /// Starting failed; not tried again until the switch is toggled.
    failed: Option<String>,
}

struct Live {
    backend: Arc<Backend>,
    server: http::Server,
    seen: u64,
}

impl Controller {
    /// Called before each frame of the window. Starts or stops the server as the switch says, reads
    /// changes an agent made, and returns the guard that keeps tools from writing during the frame.
    pub fn frame(
        &mut self,
        core: &mut crate::core::CoreState,
        ops: &mut crate::ops::OpsState,
    ) -> Option<GateGuard> {
        if !core.persist {
            return None;
        }
        if self
            .finishing
            .as_ref()
            .is_some_and(|backend| !backend.job_running())
        {
            self.finishing = None;
            core.reload_from_disk();
            ops.reload_from_disk();
        }
        let wanted = core.settings.agent_control;
        if !wanted {
            self.close();
            self.failed = None;
        } else if self.live.is_none() && self.failed.is_none() {
            let backend = Backend::new(Dirs::real(), false, core.ctx.clone(), core.runner.handle());
            match http::Server::start(backend.clone()) {
                Ok(server) => {
                    let seen = 0;
                    self.live = Some(Live {
                        backend,
                        server,
                        seen,
                    });
                }
                Err(error) => self.failed = Some(format!("{error:#}")),
            }
        }
        let mut guard = None;
        let view = &mut core.ui.agent;
        view.error = self.failed.clone();
        view.port = self.live.as_ref().map(|l| l.server.port());
        view.endpoint_file = auth::endpoint_file(&Dirs::real());
        if let Some(live) = &mut self.live {
            guard = Some(live.backend.gate.enter());
            view.calls = live.backend.calls.recent();
            if let Some(hardware) = &core.hw {
                live.backend.set_hardware(hardware);
            }
            let revision = live.backend.revision();
            if revision != live.seen {
                live.seen = revision;
                core.reload_from_disk();
                ops.reload_from_disk();
            }
        } else {
            view.calls.clear();
        }
        guard
    }

    /// The window is closing.
    pub fn close(&mut self) {
        if let Some(live) = self.live.take()
            && live.backend.job().is_some() {
                self.finishing = Some(live.backend.clone());
                live.backend.wake();
            }
    }
}

/// `--headless` and `--mcp-stdio`. `None` means: open the window as usual.
pub fn run_from_args(args: impl IntoIterator<Item = String>) -> Option<i32> {
    let args: Vec<String> = args.into_iter().collect();
    if let [flag, path] = args.as_slice()
        && flag == "--write-agent-docs" {
            return Some(match std::fs::write(path, docs::render()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("Could not write agent documentation: {error}");
                    1
                }
            });
        }
    if args.iter().any(|a| a == "--mcp-stdio") {
        Some(stdio::run())
    } else if args.iter().any(|a| a == "--headless") {
        Some(headless::run())
    } else {
        None
    }
}
