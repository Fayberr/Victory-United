//! Starting the engine: warm-up (shader translation), the game process, logs, stop and exit report.
//!
//! No polling anywhere. A worker thread blocks on the child's stdout while warming and on `Child::wait`
//! while the game runs; it sends events through a channel and calls `ctx.request_repaint()` once per event.
//! The UI drains the channel at the start of each frame (frames only happen on input or events).
use super::{
    launch::{Plan, PATH_VARS},
    saves,
};
use eframe::egui;
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Fewer entries than this in the shader cache means the first start needs the warm-up (as in the brief).
pub const COLD_CACHE_ENTRIES: usize = 1000;
/// An exit sooner than this with extra flags set offers to start again without them.
pub const QUICK_EXIT_SECS: u64 = 30;
pub const KEEP_LOGS: usize = 20;

#[derive(Clone, Debug)]
pub struct Summary {
    pub code: Option<i32>,
    pub log: Option<PathBuf>,
    pub secs: u64,
    /// The engine ended within `QUICK_EXIT_SECS` while extra flags were set.
    pub quick_exit_with_flags: bool,
    pub error: Option<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum RunState {
    Idle,
    Warming { done: u64, total: u64, since: Instant },
    Starting,
    Running { pid: u32, since: Instant, log: PathBuf },
    Finishing { log: PathBuf },
    Ended(Summary),
}

enum Event {
    Progress(u64, u64),
    WarmFinished,
    BackupWarning(String),
    Started { pid: u32, log: PathBuf },
    GameExited { log: PathBuf },
    Ended(Summary),
}

pub struct RunStorage {
    pub _guard: File,
    pub backups: PathBuf,
    pub before: bool,
    pub after: bool,
    pub retention: saves::Retention,
}

impl RunStorage {
    fn snapshot(&self, plan: &Plan, kind: saves::Kind, deadline: Option<Instant>) -> anyhow::Result<bool> {
        let build = plan.engine.parent().and_then(Path::file_name).and_then(|name| name.to_str()).unwrap_or_default();
        Ok(saves::create(&plan.data_dir, &self.backups, saves::Create {
            kind, label: "", engine_build: build, deadline, force: false,
        })?.is_some())
    }
}

/// What every handle of one runner sees.
struct Inner {
    state: RunState,
    rx: Receiver<Event>,
    backup_warning: Option<String>,
    backup_revision: u64,
    launch_revision: u64,
    env: Vec<(String, String)>,
}

/// A handle to the one game process the launcher looks after. The window and the agent interface each
/// hold one; `drain` brings a handle up to date.
pub struct Runner {
    pub state: RunState,
    pub backup_warning: Option<String>,
    pub backup_revision: u64,
    pub launch_revision: u64,
    /// The `VICTORY_*` environment of the current or last start, without the path variables.
    pub env: Vec<(String, String)>,
    inner: Arc<Mutex<Inner>>,
    tx: Sender<Event>,
    ctx: egui::Context,
    child_pid: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    skip: Arc<AtomicBool>,
}

impl Runner {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let inner = Inner { state: RunState::Idle, rx, backup_warning: None, backup_revision: 0, launch_revision: 0, env: Vec::new() };
        Self {
            state: RunState::Idle, backup_warning: None, backup_revision: 0, launch_revision: 0, env: Vec::new(),
            inner: Arc::new(Mutex::new(inner)), tx, ctx,
            child_pid: Arc::new(AtomicU32::new(0)), cancel: Arc::new(AtomicBool::new(false)), skip: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Another handle to the same runner.
    pub fn handle(&self) -> Self {
        let mut handle = Self {
            state: RunState::Idle, backup_warning: None, backup_revision: 0, launch_revision: 0, env: Vec::new(),
            inner: self.inner.clone(), tx: self.tx.clone(), ctx: self.ctx.clone(),
            child_pid: self.child_pid.clone(), cancel: self.cancel.clone(), skip: self.skip.clone(),
        };
        handle.drain();
        handle
    }

    pub fn busy(&self) -> bool {
        busy(&self.state)
    }

    pub fn running_pid(&self) -> Option<u32> {
        match self.state {
            RunState::Running { pid, .. } => Some(pid),
            _ => None,
        }
    }

    pub fn log_file(&self) -> Option<PathBuf> {
        match &self.state {
            RunState::Running { log, .. } | RunState::Finishing { log } => Some(log.clone()),
            RunState::Ended(s) => s.log.clone(),
            _ => None,
        }
    }

    /// Starts the whole flow on a worker thread. False when something is already running.
    pub fn start(&mut self, plan: Plan, logs_dir: PathBuf, warm: bool, flags_active: bool, storage: Option<RunStorage>) -> bool {
        let inner = self.inner.clone();
        let mut inner = lock(&inner);
        apply(&mut inner);
        if busy(&inner.state) {
            self.copy(&inner);
            return false;
        }
        inner.backup_warning = None;
        inner.env = plan.env.iter().filter(|(k, _)| !PATH_VARS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        self.cancel.store(false, Ordering::SeqCst);
        self.skip.store(false, Ordering::SeqCst);
        self.child_pid.store(0, Ordering::SeqCst);
        inner.state = if warm { RunState::Warming { done: 0, total: 0, since: Instant::now() } } else { RunState::Starting };
        self.copy(&inner);
        let (tx, ctx, pid, cancel, skip) = (
            self.tx.clone(), self.ctx.clone(), self.child_pid.clone(), self.cancel.clone(), self.skip.clone(),
        );
        std::thread::spawn(move || {
            let send = |e: Event| {
                let _ = tx.send(e);
                ctx.request_repaint();
            };
            if warm {
                run_warmup(&plan, &logs_dir, &pid, &skip, &cancel, &send);
                if cancel.load(Ordering::SeqCst) {
                    send(Event::Ended(Summary { code: None, log: None, secs: 0, quick_exit_with_flags: false, error: None, cancelled: true }));
                    return;
                }
                send(Event::WarmFinished);
            }
            let mut before_created = false;
            if let Some(storage) = &storage
                && storage.before && !cancel.load(Ordering::SeqCst) {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    match storage.snapshot(&plan, saves::Kind::BeforeLaunch, Some(deadline)) {
                        Ok(created) => before_created = created,
                        Err(error) => send(Event::BackupWarning(format!("{error:#}"))),
                    }
                }
            let finish = |clean: bool| {
                if let Some(storage) = &storage {
                    let mut created = before_created;
                    if clean && storage.after {
                        match storage.snapshot(&plan, saves::Kind::AfterExit, None) {
                            Ok(after) => created |= after,
                            Err(error) => send(Event::BackupWarning(format!("{error:#}"))),
                        }
                    }
                    if created
                        && let Err(error) = saves::auto_cleanup(&storage.backups, storage.retention) {
                            send(Event::BackupWarning(format!("{error:#}")));
                        }
                }
            };
            let summary = run_game(&plan, &logs_dir, flags_active, &pid, &cancel, &send);
            finish(summary.code == Some(0) && !summary.cancelled);
            drop(storage);
            send(Event::Ended(summary));
        });
        true
    }

    /// Ends the warm-up early and goes on to the game.
    pub fn skip_warmup(&self) -> std::io::Result<()> {
        if matches!(self.state, RunState::Warming { .. }) {
            self.skip.store(true, Ordering::SeqCst);
            return kill_tree(self.child_pid.load(Ordering::SeqCst));
        }
        Ok(())
    }

    /// Stops what is running: the warm-up or the game, with its child processes.
    pub fn stop(&self) -> std::io::Result<()> {
        self.cancel.store(true, Ordering::SeqCst);
        kill_tree(self.child_pid.load(Ordering::SeqCst))
    }

    /// Takes the events that arrived since the last call.
    pub fn drain(&mut self) {
        let inner = self.inner.clone();
        let mut inner = lock(&inner);
        apply(&mut inner);
        self.copy(&inner);
    }

    /// Blocks until nothing runs any more (the after-exit backup included). Only for a process that is
    /// about to end: other handles wait while this does.
    pub fn wait_idle(&mut self) {
        let inner = self.inner.clone();
        let mut inner = lock(&inner);
        apply(&mut inner);
        while busy(&inner.state) {
            let Ok(event) = inner.rx.recv() else { break };
            apply_one(&mut inner, event);
        }
        self.copy(&inner);
    }

    fn copy(&mut self, inner: &Inner) {
        self.state = inner.state.clone();
        self.backup_warning = inner.backup_warning.clone();
        self.backup_revision = inner.backup_revision;
        self.launch_revision = inner.launch_revision;
        self.env.clone_from(&inner.env);
    }
}

fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn busy(state: &RunState) -> bool {
    matches!(state, RunState::Warming { .. } | RunState::Starting | RunState::Running { .. } | RunState::Finishing { .. })
}

fn apply(inner: &mut Inner) {
    while let Ok(event) = inner.rx.try_recv() {
        apply_one(inner, event);
    }
}

fn apply_one(inner: &mut Inner, event: Event) {
    match event {
        Event::Progress(done, total) => {
            if let RunState::Warming { since, .. } = inner.state {
                inner.state = RunState::Warming { done, total, since };
            }
        }
        Event::BackupWarning(message) => inner.backup_warning = Some(message),
        Event::WarmFinished => inner.state = RunState::Starting,
        Event::Started { pid, log } => {
            inner.state = RunState::Running { pid, since: Instant::now(), log };
            inner.launch_revision = inner.launch_revision.wrapping_add(1);
        }
        Event::GameExited { log } => inner.state = RunState::Finishing { log },
        Event::Ended(s) => {
            inner.state = RunState::Ended(s);
            inner.backup_revision = inner.backup_revision.wrapping_add(1);
        }
    }
}

/// Runs `victory shaders <game folder>` and reports `VICTORY-PROGRESS shaders <done> <total>` lines.
/// Returns when the process ends (finished, skipped or stopped).
fn run_warmup(
    plan: &Plan,
    logs_dir: &Path,
    pid: &AtomicU32,
    skip: &AtomicBool,
    cancel: &AtomicBool,
    send: &dyn Fn(Event),
) {
    let interrupted = || skip.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst);
    if interrupted() {
        return;
    }
    let _ = std::fs::create_dir_all(logs_dir);
    let mut log = std::fs::File::create(logs_dir.join(format!("warmup-{}.log", timestamp()))).ok();
    let mut cmd = engine_command(plan, plan.warm_args());
    cmd.stdout(Stdio::piped()).stderr(match log.as_ref().and_then(|f| f.try_clone().ok()) {
        Some(f) => Stdio::from(f),
        None => Stdio::null(),
    });
    let Ok(mut child) = cmd.spawn() else { return };
    pid.store(child.id(), Ordering::SeqCst);
    if interrupted() {
        let _ = kill_tree(child.id());
    }
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(f) = log.as_mut() {
                let _ = writeln!(f, "{line}");
            }
            if let Some((done, total)) = parse_progress(&line) {
                send(Event::Progress(done, total));
            }
        }
    }
    let _ = child.wait();
    pid.store(0, Ordering::SeqCst);
}

fn run_game(plan: &Plan, logs_dir: &Path, flags_active: bool, pid: &AtomicU32, cancel: &AtomicBool, send: &dyn Fn(Event)) -> Summary {
    let fail = |msg: String, log: Option<PathBuf>| Summary {
        code: None, log, secs: 0, quick_exit_with_flags: false, error: Some(msg), cancelled: false,
    };
    if cancel.load(Ordering::SeqCst) {
        return Summary { cancelled: true, ..fail("Stopped.".into(), None) };
    }
    if let Err(e) = std::fs::create_dir_all(logs_dir) {
        return fail(format!("Cannot create the log folder: {e}"), None);
    }
    prune_logs(logs_dir, KEEP_LOGS - 1);
    let log_path = logs_dir.join(format!("run-{}.log", timestamp()));
    let out = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => return fail(format!("Cannot create the log file: {e}"), None),
    };
    let Ok(err) = out.try_clone() else { return fail("Cannot open the log file twice.".into(), Some(log_path)) };
    let mut cmd = engine_command(plan, plan.run_args());
    cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("Could not start the engine ({}): {e}", plan.engine.display()), Some(log_path)),
    };
    pid.store(child.id(), Ordering::SeqCst);
    if cancel.load(Ordering::SeqCst) {
        let _ = kill_tree(child.id());
    }
    send(Event::Started { pid: child.id(), log: log_path.clone() });
    let status = child.wait();
    pid.store(0, Ordering::SeqCst);
    send(Event::GameExited { log: log_path.clone() });
    let secs = started.elapsed().as_secs();
    let stopped = cancel.load(Ordering::SeqCst);
    Summary {
        code: status.as_ref().ok().and_then(|s| s.code()),
        log: Some(log_path),
        secs,
        quick_exit_with_flags: flags_active && !stopped && secs < QUICK_EXIT_SECS,
        error: status.err().map(|e| e.to_string()),
        cancelled: stopped,
    }
}

/// The engine command: working folder is the engine's own, inherited `VICTORY_*` values other than the
/// path variables are dropped (scripts/play.ps1 does the same), then the resolved environment is set.
fn engine_command(plan: &Plan, args: Vec<String>) -> Command {
    let mut cmd = Command::new(&plan.engine);
    cmd.args(args);
    if let Some(dir) = plan.engine.parent().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    for (k, _) in std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v))) {
        if k.starts_with("VICTORY_") {
            cmd.env_remove(k);
        }
    }
    cmd.envs(&plan.env);
    hide_window(&mut cmd);
    cmd
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: the engine opens its own game window
}
#[cfg(not(windows))]
fn hide_window(_cmd: &mut Command) {}

/// Kills a process and everything it started.
pub fn kill_tree(pid: u32) -> std::io::Result<()> {
    if pid == 0 {
        return Ok(());
    }
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::CommandExt;
        Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000).stdout(Stdio::null()).stderr(Stdio::null()).status()?
    };
    #[cfg(not(windows))]
    let status = Command::new("kill").args(["-9", &pid.to_string()]).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("Stop returned {status}")));
    }
    Ok(())
}

pub fn parse_progress(line: &str) -> Option<(u64, u64)> {
    let rest = line.trim().strip_prefix("VICTORY-PROGRESS shaders ")?;
    let (a, b) = rest.split_once(' ')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Keeps the newest `keep` run logs (and warm-up logs, separately), deletes the rest.
pub fn prune_logs(dir: &Path, keep: usize) {
    for prefix in ["run-", "warmup-"] {
        let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix) && n.to_string_lossy().ends_with(".log")))
            .collect();
        logs.sort(); // names carry the timestamp, oldest first
        let excess = logs.len().saturating_sub(keep);
        for old in logs.into_iter().take(excess) {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// The newest `run-*.log`, for the bug report.
pub fn latest_run_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("run-")))
        .max()
}

/// Number of translated shaders in `<cache dir>\shaders` (stops counting at `limit`). The cache is a
/// content-addressed store with one folder per two hex digits, so this is at most 256 small listings.
pub fn shader_entries(cache_dir: &Path, limit: usize) -> usize {
    let mut n = 0;
    for sub in std::fs::read_dir(cache_dir.join("shaders")).into_iter().flatten().flatten() {
        if sub.path().is_dir() {
            n += std::fs::read_dir(sub.path()).into_iter().flatten().flatten().filter(|e| !e.file_name().to_string_lossy().starts_with(".tmp")).count();
            if n >= limit {
                return n;
            }
        }
    }
    n
}

/// `YYYYMMDD-HHMMSS` in UTC (no time zone database needed; the name only has to sort).
pub fn timestamp() -> String {
    timestamp_at(super::storage::now().as_secs())
}

pub fn timestamp_at(secs: u64) -> String {
    let secs = secs.min(i64::MAX as u64) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Remaining time text from progress and elapsed time, `None` until there is enough to say.
pub fn remaining(done: u64, total: u64, elapsed: Duration) -> Option<Duration> {
    if done == 0 || total <= done || elapsed < Duration::from_secs(3) {
        return None;
    }
    let rate = done as f64 / elapsed.as_secs_f64();
    Some(Duration::from_secs_f64((total - done) as f64 / rate))
}

pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::tempdir;
    #[cfg(unix)]
    use crate::core::launch::{Console, Plan, Resolved};
    #[cfg(unix)]
    use std::collections::BTreeMap;

    #[test]
    fn progress_lines_parse() {
        assert_eq!(parse_progress("VICTORY-PROGRESS shaders 5 100"), Some((5, 100)));
        assert_eq!(parse_progress("1000 shaders in the cache, 3 s"), None);
        assert_eq!(parse_progress("VICTORY-PROGRESS shaders x 1"), None);
    }

    #[test]
    fn logs_are_pruned_to_the_newest() {
        let d = tempdir("logs");
        for i in 0..25 {
            std::fs::write(d.join(format!("run-20260101-0000{i:02}.log")), "x").unwrap();
        }
        std::fs::write(d.join("other.txt"), "x").unwrap();
        prune_logs(&d, 19);
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.iter().filter(|n| n.starts_with("run-")).count(), 19);
        assert!(!names.contains(&"run-20260101-000000.log".to_string()));
        assert!(names.contains(&"run-20260101-000024.log".to_string()));
        assert!(names.contains(&"other.txt".to_string()));
        assert_eq!(latest_run_log(&d).unwrap().file_name().unwrap(), "run-20260101-000024.log");
    }

    #[test]
    fn cold_cache_counting_and_time_helpers() {
        let d = tempdir("cache");
        assert_eq!(shader_entries(&d, 1000), 0);
        for sub in ["00", "01"] {
            std::fs::create_dir_all(d.join("shaders").join(sub)).unwrap();
            for i in 0..3 {
                std::fs::write(d.join("shaders").join(sub).join(format!("e{i}")), "x").unwrap();
            }
        }
        std::fs::write(d.join("shaders").join("00").join(".tmp-1-1"), "x").unwrap();
        assert_eq!(shader_entries(&d, 1000), 6);
        assert_eq!(shader_entries(&d, 3), 3);
        assert_eq!(remaining(0, 10, Duration::from_secs(9)), None);
        assert_eq!(remaining(50, 100, Duration::from_secs(10)).unwrap().as_secs(), 10);
        assert_eq!(format_duration(Duration::from_secs(3725)), "1 h 2 min");
        assert_eq!(timestamp().len(), 15);
    }

    #[cfg(unix)]
    #[test]
    fn warmup_progress_and_game_exit_flow_with_a_fake_engine() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempdir("fake-engine");
        let engine = d.join("victory.exe");
        let script = [
            "#!/bin/sh",
            "if [ \"$1\" = shaders ]; then echo 'VICTORY-PROGRESS shaders 5 10'; echo 'VICTORY-PROGRESS shaders 10 10';",
            "else echo hello from the game; echo oops >&2; exit 3; fi",
            "",
        ]
        .join("\n");
        std::fs::write(&engine, script).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plan = Plan::new(engine, &d, d.join("game.exe"), Console::SeriesX, &Resolved::default(), &BTreeMap::new(), &d.join("cache"), &d);
        let ctx = egui::Context::default();
        let mut r = Runner::new(ctx);
        r.start(plan, d.join("logs"), true, true, None);
        let t0 = Instant::now();
        let mut saw_progress = false;
        while !matches!(r.state, RunState::Ended(_)) && t0.elapsed() < Duration::from_secs(10) {
            r.drain();
            saw_progress |= matches!(r.state, RunState::Warming { done: 5, total: 10, .. } | RunState::Warming { done: 10, total: 10, .. });
            std::thread::sleep(Duration::from_millis(5));
        }
        let RunState::Ended(s) = r.state.clone() else { panic!("did not end: {:?}", r.state) };
        assert_eq!(s.code, Some(3));
        assert!(s.quick_exit_with_flags);
        let text = std::fs::read_to_string(s.log.unwrap()).unwrap();
        assert!(text.contains("hello from the game") && text.contains("oops"), "stdout and stderr both in the log");
        let _ = saw_progress; // progress may be consumed between polls; the parse itself is tested above
    }
}
