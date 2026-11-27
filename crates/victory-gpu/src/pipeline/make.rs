//! Host pipelines are made by workers, not by the title thread that first
//! records a draw with them. Translating a shader and the host driver's
//! compile take from milliseconds to seconds each (structurisation of the
//! biggest compute programs ~0.4 s, some driver compiles 3.4 s), and
//! the title's first world frame wants over 1,200 new pipelines: made
//! on its recording threads, that frame took more than 30 s and the
//! title's GPU watchdog ended the run (decision 0007, "Pipelines made off
//! the title's threads").
//!
//! A first use queues the pipeline and waits for it within a budget per
//! frame, so the cheap ones (menus, one-off passes such as a colour-grading
//! bake) are there at once as before; past the budget a draw or dispatch
//! whose pipeline is not ready is dropped, as an untranslated one is.
//!
//! The default budget is half a second per title frame for the first
//! [`WARMUP_FRAMES`] (boot and one-shot init passes compile reliably), then
//! [`STEADY_MS`]: half a second in every frame that met a new pipeline made
//! the intro and menus stutter while a cold library was filled. A pass the
//! title runs only once and whose pipeline takes longer is lost.
//! Passes over the
//! screen (no vertex input, no depth) have workers and a longer budget of
//! their own ([`screen_budget`]): the title fills its lighting tables with
//! such draws once, at the world's first frame, and behind a world's worth of
//! pipelines they were dropped; that run had no lamps and no diffuse light to
//! its end (run textures-d2, the first after a new translation; Belobug
//! 18e57db8 found the same).
//! Every compute pipeline is started when the title creates it, so the
//! workers have usually made it by its first dispatch.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Shader;
use super::host::{GraphicsState, HostPipeline, TargetFormats};
/// `VICTORY_DENY_PIPELINES=<16-hex shader hash>,...` (off unless set): the
/// hash of a shader of `shaders` that is listed there. Such a pipeline is
/// never built (no driver compile) and counts as refused, so its draws and
/// dispatches are dropped. Unlike `VICTORY_SKIP_PIPELINES`, in release builds too.
fn denied(shaders: &[Arc<Shader>]) -> Option<&str> {
    static LIST: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    let list = LIST.get_or_init(|| {
        // `VICTORY_DENY_PIPELINES_FILE=<path>`: more hashes, any 16 hex
        // digits in the file (one a line, say), with the list's own.
        let file = std::env::var_os("VICTORY_DENY_PIPELINES_FILE")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        let list: std::collections::HashSet<String> = std::env::var("VICTORY_DENY_PIPELINES")
            .unwrap_or_default()
            .split(|c: char| !c.is_ascii_hexdigit())
            .chain(file.split(|c: char| !c.is_ascii_hexdigit()))
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| h.len() == 16)
            .collect();
        if !list.is_empty() {
            tracing::info!(pipelines = list.len(), "shaders whose pipelines are not built (VICTORY_DENY_PIPELINES / _FILE)");
        }
        list
    });
    if list.is_empty() {
        return None;
    }
    shaders.iter().find(|s| list.contains(&s.hash)).map(|s| s.hash.as_str())
}

/// The shaders of `p` (for logs).
pub(crate) fn shaders(p: &Pending) -> &[Arc<Shader>] {
    &p.shaders
}

/// What a host pipeline is made from, and the host pipeline once made.
pub(crate) struct Pending {
    pub(crate) shaders: Vec<Arc<Shader>>,
    pub(crate) graphics: Option<Box<GraphicsState>>,
    queue_state: AtomicU8,
    /// Taken by a worker from [`Queue::slow`].
    was_slow: AtomicBool,
    made: OnceLock<Option<HostPipeline>>,
    /// What identifies it across runs (shader containers and state), and
    /// whether a draw or dispatch has requested it yet.
    key: String,
    used: AtomicBool,
    /// A pass over the screen ([`GraphicsState::screen_pass`]) or a confirmed
    /// opening-scene / bake producer that must never sit behind cold RT shaders.
    screen: bool,
    /// Confirmed first-scene / bake producer prepared early even on a cold cache.
    scene_input: bool,
    /// Why this pipeline is intentionally skipped (e.g. RT resolve without BVH).
    left_out: Option<&'static str>,
    /// LOCAL: how long draws have waited for it so far, in microseconds
    /// ([`wait_once`]).
    waited_us: AtomicU64,
    /// Whether the made pipeline has gone to a draw or dispatch yet
    /// (`crate::host::first_use`: for a device removal's report).
    handed: AtomicBool,
    /// The formats its state declares for what a draw draws into (zeros
    /// without a graphics state).
    pub(crate) formats: TargetFormats,
    /// The same shaders and state for other formats, bound at a draw
    /// ([`for_formats`]): a chain from the title's pipeline, one link per
    /// set of formats met, never shortened.
    variant: OnceLock<Arc<Pending>>,
    /// First title frame in which a recording thread waited on this pipeline
    /// (`u64::MAX`: never waited yet). Caps waiting to one frame per pipeline
    /// so a slow or hung shader compiles asynchronously without stalling every
    /// subsequent frame for the full wait budget.
    waited_frame: AtomicU64,
}

fn named(shaders: &[Arc<Shader>], name: &[u8]) -> bool {
    shaders
        .iter()
        .any(|s| s.stage == "compute" && s.container.windows(name.len()).any(|w| w == name))
}

fn left_out(shaders: &[Arc<Shader>], skip_hashes: &[String]) -> Option<&'static str> {
    static SKIP_RT_RESOLVE: OnceLock<bool> = OnceLock::new();
    let skip_rt = *SKIP_RT_RESOLVE.get_or_init(|| {
        let builder = std::env::var_os("VICTORY_BVH_BUILDER").is_some_and(|v| v == "1");
        if builder {
            return false;
        }
        // Without BVH, ~1,000 CS_RaytracingResolve shaders shade no hits while
        // clogging the cold compiler queue; skip unless VICTORY_RT_RESOLVE=1.
        std::env::var_os("VICTORY_RT_RESOLVE").is_none_or(|v| v == "0")
    });
    if skip_rt && named(shaders, b"CS_RaytracingResolve") {
        return Some("shades ray-tracing hits without BVH, left out");
    }
    static SKIP_ACTIVATE_RAYS: OnceLock<bool> = OnceLock::new();
    let skip_activate = *SKIP_ACTIVATE_RAYS.get_or_init(|| {
        let builder = std::env::var_os("VICTORY_BVH_BUILDER").is_some_and(|v| v == "1");
        if builder {
            return std::env::var_os("VICTORY_ACTIVATE_RAYS").is_some_and(|v| v == "0");
        }
        std::env::var_os("VICTORY_ACTIVATE_RAYS").is_none_or(|v| v == "0")
    });
    if skip_activate && named(shaders, b"CS_ScreenProbes_ActivateRays") {
        return Some("ray activation shader without BVH, left out");
    }
    if super::skipped_in_list(shaders, skip_hashes).is_some() {
        return Some("skipped by VICTORY_SKIP_PIPELINES");
    }
    None
}

/// Confirmed first-scene producers. Prepare these at creation even on a cold
/// launch so cutscene lighting, tiling, and bakes never miss their first frame.
fn scene_input(shaders: &[Arc<Shader>]) -> bool {
    shaders.iter().any(|s| {
        matches!(
            s.hash.as_str(),
            "970c4fbb3b017c0e"
                | "068c90badc86f1fd"
                | "e4a7e088c7780ce8"
                | "ebe4e53f031f5249"
                | "2cc4c197ee29e291"
                | "369cbe906ea72764"
        ) || crate::draw::BAKE_PIXEL_SHADERS.contains(&s.hash.as_str())
    })
}

// SAFETY: D3D12 objects are free-threaded; the graphics state (whose input
// elements point into its own names) is only read after creation.
unsafe impl Send for Pending {}
// SAFETY: as above.
unsafe impl Sync for Pending {}

impl Pending {
    pub(crate) fn new(shaders: Vec<Arc<Shader>>, graphics: Option<Box<GraphicsState>>) -> Pending {
        Self::new_with_skip_hashes(shaders, graphics, super::skip_hashes())
    }

    fn new_with_skip_hashes(
        shaders: Vec<Arc<Shader>>,
        graphics: Option<Box<GraphicsState>>,
        skip_hashes: &[String],
    ) -> Pending {
        let mut key = blake3::Hasher::new();
        for s in &shaders {
            key.update(s.stage.as_bytes());
            key.update(s.hash.as_bytes());
        }
        key.update(graphics.as_ref().map(|g| g.describe()).unwrap_or_default().as_bytes());
        let scene_input = scene_input(&shaders);
        let screen = (graphics.as_ref().is_some_and(|g| g.screen_pass())
            && shaders.iter().all(|s| s.stage != "mesh"))
            || scene_input;
        let left_out = left_out(&shaders, skip_hashes);
        Pending {
            screen,
            scene_input,
            left_out,
            formats: graphics.as_ref().map_or([0; 9], |g| g.formats()),
            variant: OnceLock::new(),
            shaders,
            graphics,
            queue_state: AtomicU8::new(NOT_QUEUED),
            was_slow: AtomicBool::new(false),
            made: OnceLock::new(),
            key: key.finalize().to_hex()[..32].to_owned(),
            used: AtomicBool::new(false),
            waited_us: AtomicU64::new(0),
            waited_frame: AtomicU64::new(u64::MAX),
            handed: AtomicBool::new(false),
        }
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }

    /// A compute pipeline: one program, of the compute stage.
    fn compute(&self) -> bool {
        matches!(self.shaders.as_slice(), [cs] if cs.stage == "compute")
    }

    fn make_guarded(&self) {
        // Not through `OnceLock::get_or_init`: a worker the driver's compiler
        // crashed under stays inside the call for good (see [`guard_start`]),
        // and anyone else asking would wait for it forever.
        if self.made.get().is_none() {
            let made = if is_bad_key(&self.key) {
                SKIPPED_BAD.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(key = self.key, shaders = ?self.shaders, "not made: the driver's compiler crashed on it before");
                None
            } else {
                let _building = crate::timeline::span("pipeline: build a host pipeline");
                if let Some(hash) = denied(&self.shaders) {
                    tracing::warn!(hash, shaders = ?self.shaders, "pipeline not built: its shader is in VICTORY_DENY_PIPELINES");
                    let _ = self.made.set(None);
                    return;
                }
                let began = Instant::now();
                let made = HostPipeline::new(&self.shaders, self.graphics.as_deref());
                super::library::note_time(&self.key, began.elapsed().as_millis() as u64);
                made
            };
            let _ = self.made.set(made);
        }
    }

    fn make(&self) {
        if !guard_on() {
            self.make_with(|| {
                let _building = crate::timeline::span("pipeline: build a host pipeline");
                if let Some(hash) = denied(&self.shaders) {
                    tracing::warn!(hash, shaders = ?self.shaders, "pipeline not built: its shader is in VICTORY_DENY_PIPELINES");
                    return None;
                }
                let began = Instant::now();
                let made = HostPipeline::new(&self.shaders, self.graphics.as_deref());
                super::library::note_time(&self.key, began.elapsed().as_millis() as u64);
                made
            });
        } else {
            self.make_guarded();
        }
        let (lock, done) = &DONE;
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        done.notify_all();
    }

    fn make_with(&self, build: impl FnOnce() -> Option<HostPipeline>) {
        self.made.get_or_init(|| {
            // A failed translation must complete this queue entry and wake its waiters.
            match catch_unwind(AssertUnwindSafe(build)) {
                Ok(pipeline) => pipeline,
                Err(_) => {
                    tracing::warn!(shaders = ?self.shaders, "host pipeline build panicked");
                    None
                }
            }
        });
    }
}

/// Pipelines not made for being on the list of those the driver's compiler
/// crashes on.
static SKIPPED_BAD: AtomicU64 = AtomicU64::new(0);

/// The keys of the pipelines the driver's compiler crashed on
/// (`warm-bad.txt` beside the pipeline library, one key to a line, then
/// anything; `#` begins a comment), loaded on first use. `VICTORY_WARM_BAD=0`
/// leaves the file unread.
static BAD_KEYS: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);

fn bad_keys<R>(f: impl FnOnce(&mut std::collections::HashSet<String>) -> R) -> R {
    let mut keys = BAD_KEYS.lock().unwrap_or_else(|e| e.into_inner());
    let keys = keys.get_or_insert_with(|| {
        if std::env::var_os("VICTORY_WARM_BAD").is_some_and(|v| v == "0") {
            return Default::default();
        }
        let listed = super::library::folder()
            .and_then(|d| std::fs::read_to_string(d.join("warm-bad.txt")).ok())
            .unwrap_or_default();
        listed
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_whitespace().next())
            .map(str::to_owned)
            .collect()
    });
    f(keys)
}

fn is_bad_key(key: &str) -> bool {
    bad_keys(|keys| keys.contains(key))
}

/// Whether the AMD driver's compiler crashes are caught. Other adapters keep
/// the original pipeline initialization path.
/// (`VICTORY_COMPILER_GUARD=0`: not).
fn guard_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::host::is_amd() && std::env::var_os("VICTORY_COMPILER_GUARD").is_none_or(|v| v != "0"))
}

/// Worker threads that can be told apart by the guard, over the run.
const MAX_SLOTS: usize = 128;
/// Per worker slot: its compile crashed (set in the handler), where.
static CRASHED: [AtomicBool; MAX_SLOTS] = [const { AtomicBool::new(false) }; MAX_SLOTS];
static FAULT_ADDR: [AtomicU64; MAX_SLOTS] = [const { AtomicU64::new(0) }; MAX_SLOTS];
static FAULT_RIP: [AtomicU64; MAX_SLOTS] = [const { AtomicU64::new(0) }; MAX_SLOTS];
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
/// The event the handler signals (a Win32 handle).
static CRASH_EVENT: AtomicUsize = AtomicUsize::new(0);
/// What each slot's worker is making, and whether it takes passes over the
/// screen alone. Never locked by the handler.
static ACTIVE_JOBS: Mutex<Vec<Option<(Arc<Pending>, bool)>>> = Mutex::new(Vec::new());

thread_local! {
    /// This worker's slot (`usize::MAX`: none).
    static WORKER_SLOT: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
    /// Whether this thread is inside the compile of a pipeline.
    static COMPILING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A worker's compile of `p`: noted for the guard while it lasts.
struct Compile {
    slot: usize,
}

impl Compile {
    fn begin(slot: usize, p: &Arc<Pending>, screens_only: bool) -> Compile {
        if slot < MAX_SLOTS && guard_on() {
            let mut jobs = ACTIVE_JOBS.lock().unwrap_or_else(|e| e.into_inner());
            if jobs.len() < MAX_SLOTS {
                jobs.resize(MAX_SLOTS, None);
            }
            jobs[slot] = Some((p.clone(), screens_only));
            COMPILING.set(true);
        }
        Compile { slot }
    }
}

impl Drop for Compile {
    fn drop(&mut self) {
        if COMPILING.replace(false) && self.slot < MAX_SLOTS {
            let mut jobs = ACTIVE_JOBS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(job) = jobs.get_mut(self.slot) {
                *job = None;
            }
        }
    }
}

/// Installs the handler for the driver's compiler crashing (AMD's does on an
/// RX 9070 XT, and not on the same pipelines in every run: an access of the
/// first pages in `amdxc64.dll`) and the thread that deals with what it
/// left. The handler does the least there is: it notes where, signals, and
/// leaves the worker asleep for good. No allocation, no lock, no file: the
/// crash may have been under any of them. The watcher puts the pipeline on
/// the list of those left out (`warm-bad.txt`), says it is not made, gives
/// the worker's place in the counts back and starts another worker.
fn guard_start() {
    use windows_sys::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler;
    use windows_sys::Win32::System::Threading::CreateEventW;
    static ONCE: std::sync::Once = std::sync::Once::new();
    if !guard_on() {
        return;
    }
    ONCE.call_once(|| {
        // SAFETY: an unnamed auto-reset event, no security attributes.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            tracing::warn!("no event for the driver's compiler crashes: they are not caught");
            return;
        }
        CRASH_EVENT.store(event as usize, Ordering::Release);
        let spawned = std::thread::Builder::new().name("victory-pipeline-watch".into()).spawn(watch);
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "no thread for the driver's compiler crashes: they are not caught");
            return;
        }
        // SAFETY: a handler that lives as long as the process.
        unsafe {
            AddVectoredExceptionHandler(1, Some(compiler_crashed));
        }
        tracing::info!("the driver's compiler crashing on a worker is caught (VICTORY_COMPILER_GUARD=0: not)");
    });
}

/// The handler: a worker inside a compile read or wrote one of the first
/// pages. See [`guard_start`].
unsafe extern "system" fn compiler_crashed(
    info: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    use windows_sys::Win32::System::Threading::{SetEvent, Sleep};
    const ACCESS_VIOLATION: i32 = 0xc000_0005_u32 as i32;
    const CONTINUE_SEARCH: i32 = 0;
    // SAFETY: the system passes a live record.
    let record = unsafe { &*(*info).ExceptionRecord };
    if record.ExceptionCode != ACCESS_VIOLATION || record.NumberParameters < 2 || record.ExceptionInformation[1] >= 0x10000 {
        return CONTINUE_SEARCH;
    }
    if !COMPILING.try_with(|c| c.get()).unwrap_or(false) {
        return CONTINUE_SEARCH;
    }
    let slot = WORKER_SLOT.try_with(|s| s.get()).unwrap_or(usize::MAX);
    if slot >= MAX_SLOTS {
        return CONTINUE_SEARCH;
    }
    let _ = COMPILING.try_with(|c| c.set(false));
    FAULT_ADDR[slot].store(record.ExceptionInformation[1] as u64, Ordering::Relaxed);
    #[cfg(target_arch = "x86_64")]
    // SAFETY: the system passes a live context.
    FAULT_RIP[slot].store(unsafe { (*(*info).ContextRecord).Rip }, Ordering::Relaxed);
    CRASHED[slot].store(true, Ordering::Release);
    let event = CRASH_EVENT.load(Ordering::Acquire);
    if event != 0 {
        // SAFETY: the event lives as long as the process.
        unsafe { SetEvent(event as windows_sys::Win32::Foundation::HANDLE) };
    }
    loop {
        // SAFETY: a plain sleep.
        unsafe { Sleep(u32::MAX - 1) };
    }
}

/// The thread that deals with the workers the compiler crashed under.
fn watch() {
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    loop {
        let event = CRASH_EVENT.load(Ordering::Acquire);
        // SAFETY: the event lives as long as the process.
        unsafe { WaitForSingleObject(event as windows_sys::Win32::Foundation::HANDLE, INFINITE) };
        for slot in 0..MAX_SLOTS {
            if CRASHED[slot].swap(false, Ordering::AcqRel) {
                lost(slot);
            }
        }
    }
}

/// The worker at `slot` is gone with the pipeline it was making.
fn lost(slot: usize) {
    let job = {
        let mut jobs = ACTIVE_JOBS.lock().unwrap_or_else(|e| e.into_inner());
        jobs.get_mut(slot).and_then(Option::take)
    };
    let Some((p, screens_only)) = job else {
        return;
    };
    tracing::error!(
        key = p.key,
        shaders = ?p.shaders,
        rip = format_args!("{:#x}", FAULT_RIP[slot].load(Ordering::Relaxed)),
        address = format_args!("{:#x}", FAULT_ADDR[slot].load(Ordering::Relaxed)),
        "the driver's compiler crashed making a pipeline: it is left out, the worker is replaced"
    );
    let known = bad_keys(|keys| !keys.insert(p.key.clone()));
    if !known && let Some(dir) = super::library::folder() {
        use std::io::Write;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("warm-bad.txt"));
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{} ended the compiler in a run: {:?}", p.key, p.shaders);
        }
    }
    let _ = p.made.set(None);
    if p.was_slow.swap(false, Ordering::Relaxed) {
        let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        queue.slow_now = queue.slow_now.saturating_sub(1);
        AVAILABLE.notify_all();
    }
    {
        let (lock, done) = &DONE;
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        done.notify_all();
    }
    if WAITING.fetch_sub(1, Ordering::Relaxed) == 1 {
        drained();
    }
    if !spawn_worker(screens_only) {
        tracing::warn!("no worker to take the place of the one the compiler crashed under");
    }
}

/// Reports why a host pipeline was left out, is pending, or failed to build.
/// Returns `None` when the host pipeline is ready.
#[cfg(feature = "diagnostics")]
pub(crate) fn missing(p: &Pending) -> Option<String> {
    match p.made.get() {
        Some(Some(_)) => None,
        None => Some(p.left_out.unwrap_or("being made").into()),
        Some(None) => {
            let failed: Vec<String> = p
                .shaders
                .iter()
                .filter(|s| matches!(s.translated.get(), Some(None)))
                .map(|s| format!("{} {}", s.stage, s.hash))
                .collect();
            Some(if failed.is_empty() {
                format!("refused {:?}", p.shaders)
            } else {
                failed.join(" + ")
            })
        }
    }
}

/// The hashes of the shaders `p` is made of.
pub(crate) fn hashes(p: &Pending) -> String {
    let hashes: Vec<&str> = p.shaders.iter().map(|s| s.hash.as_str()).collect();
    hashes.join("+")
}

/// Signalled whenever a worker has made a pipeline.
static DONE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());

/// Title frames that get [`WARMUP_MS`] of waiting.
const WARMUP_FRAMES: u64 = 60;
const WARMUP_MS: u64 = 500;
/// Waiting per frame after the warm-up frames.
const STEADY_MS: u64 = 8;

/// Waiting allowed in title frame `frame`, over all threads;
/// `VICTORY_PIPELINE_WAIT_MS` sets it for every frame.
fn wait_budget(frame: u64) -> Duration {
    static BUDGET: OnceLock<Option<Duration>> = OnceLock::new();
    let set = *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_PIPELINE_WAIT_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok());
        tracing::info!(?millis, "host pipeline wait budget per frame (none: {WARMUP_MS} ms, then {STEADY_MS} ms)");
        millis.map(Duration::from_millis)
    });
    set.unwrap_or(Duration::from_millis(if frame < WARMUP_FRAMES { WARMUP_MS } else { STEADY_MS }))
}

/// As [`wait_budget`], for passes over the screen (`VICTORY_SCREEN_WAIT_MS`,
/// 10 s; 0: like the others). Below the title's 30 s watchdog.
fn screen_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_SCREEN_WAIT_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(10_000);
        Duration::from_millis(millis)
    })
}

/// Whether passes over the screen go before the others.
fn screens_first() -> bool {
    !screen_budget().is_zero()
}

/// Pipelines queued and not made yet.
static WAITING: AtomicU64 = AtomicU64::new(0);

/// Pipelines queued and not made yet (for the frame line).
pub(crate) fn waiting() -> u64 {
    WAITING.load(Ordering::Relaxed)
}

/// Pipelines made again for the formats bound at a draw, since the start.
static VARIANTS: AtomicU64 = AtomicU64::new(0);

/// Whether a draw into views of other formats than its pipeline declares
/// gets a host pipeline for those formats ([`for_formats`]); on unless
/// `VICTORY_TARGET_VARIANTS=0`.
fn variants_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_TARGET_VARIANTS").is_none_or(|v| v != "0"))
}

/// The host pipeline for a draw with `p` into views of the formats `bound`
/// that `p` does not declare: that of the same shaders and state for those
/// formats, queued and waited for like the title's own.
#[cold]
pub(crate) fn for_formats<'a>(p: &'a Arc<Pending>, bound: &TargetFormats) -> Option<&'a HostPipeline> {
    match p.graphics.as_deref() {
        Some(g) if variants_on() => ready(variant(p, g, &super::host::formats_for(&p.formats, bound))),
        _ => ready(p),
    }
}

/// The pipeline of `p`'s shaders and state `g` for the formats `bound`:
/// found in the chain from `p`, or added to it (logged once per set).
fn variant<'a>(p: &'a Arc<Pending>, g: &GraphicsState, bound: &TargetFormats) -> &'a Arc<Pending> {
    let mut at = p;
    for _ in 0..64 {
        let next = at.variant.get_or_init(|| {
            let so_far = VARIANTS.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::info!(
                pipeline = %hashes(p),
                declared = ?p.formats,
                bound = ?bound,
                so_far,
                "a pipeline is made again for the render target formats bound at its draw (VICTORY_TARGET_VARIANTS=0: the draw goes out with the title's pipeline)"
            );
            Arc::new(Pending::new(p.shaders.clone(), Some(Box::new(g.with_formats(bound)))))
        });
        if next.formats == *bound {
            return next;
        }
        at = next;
    }
    p
}

/// The host pipeline if it is made (or is made within the frame's budget);
/// queues it on first use.
pub(crate) fn ready(p: &Arc<Pending>) -> Option<&HostPipeline> {
    if p.left_out.is_some() {
        return None;
    }
    // A still-pending pipeline was requested by the title too. Prewarm it
    // on the next launch, even if this run never got to use it.
    if !p.used.load(Ordering::Relaxed) && !p.used.swap(true, Ordering::Relaxed) {
        super::library::note_used(&p.key);
    }
    if p.made.get().is_none() {
        start(p, REQUESTED);
        // A compute pipeline in the first 600 frames (the title's one-shot
        // world and lighting set-up passes) or with `compute_waits`: made
        // here, or waited for while a worker makes it, instead of dropping
        // its dispatch after the frame's budget. `make` keeps its panic guard.
        if p.graphics.is_none() && crate::frame::current() < 600 {
            p.make();
        } else if p.graphics.is_none() && compute_waits() {
            // One frame a pipeline waits (as the draws' wait below): a
            // pipeline still not made after the limit does not stall the
            // title's thread again at every dispatch of it.
            let frame = crate::frame::current();
            let first = match p.waited_frame.compare_exchange(u64::MAX, frame, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => frame,
                Err(f) => f,
            };
            if first == frame {
                wait_compute(p, frame);
            }
        } else {
            let frame = crate::frame::current();
            let first = match p.waited_frame.compare_exchange(
                u64::MAX,
                frame,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => frame,
                Err(f) => f,
            };
            if first == frame {
                wait(p, frame);
            }
        }
    }
    let made = p.made.get()?.as_ref();
    // The first time the made pipeline goes to a draw or dispatch: noted
    // for a device removal's report (one load a call after that).
    if let Some(host) = made
        && !p.handed.load(Ordering::Relaxed)
        && !p.handed.swap(true, Ordering::Relaxed)
    {
        crate::host::first_use(&host.name);
    }
    made
}

/// Whether a dispatch waits for its compute pipeline until it is made,
/// whatever the frame budget ([`wait_budget`]); a draw still waits within
/// the budget and is dropped while its pipeline is not there.
///
/// A dropped draw is drawn again in the next frame. A dropped dispatch may
/// not be: the title runs some compute passes once, when a scene changes
/// (a buffer reset or cleared), and what they leave is read for as long as
/// the scene lasts. In the prison corridor a compute program met for the
/// first time took 4.4 s to make on a 16 GB machine; its one dispatch was
/// dropped after the half second of budget, and the windows of the room
/// before stayed blended over the whole picture for minutes (run 195).
/// A wait of seconds once (the pipeline library has it on the next run)
/// against a picture wrong until the scene ends.
/// On by default, for at most [`compute_wait_limit`] (10 s) one frame of a
/// pipeline; `VICTORY_PIPELINE_COMPUTE_WAIT=0`: within the budget, as
/// before.
fn compute_waits() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_COMPUTE_WAIT").is_none_or(|v| v != "0"))
}
/// How long a dispatch waits for its compute pipeline at most
/// (`VICTORY_PIPELINE_COMPUTE_WAIT_MS`, default 10,000; 0 or unreadable:
/// the default). The title's watchdog thread faults the title (write to
/// address 0, `+0x23cf683`) when its main thread has not moved for about
/// 30 s. With the wait unbounded, a cold shader cache (a changed
/// translator) put the title's thread behind a queue of compute programs
/// that took 29 s and the title ended a minute into the world (fork issue
/// 1420; every run of our combined10 tree on a cache keyed for the one
/// before). Past the limit the dispatch is dropped as with
/// `VICTORY_PIPELINE_COMPUTE_WAIT=0`, and the pipeline is made meanwhile
/// (the library has it on the next run).
fn compute_wait_limit() -> Duration {
    static LIMIT: OnceLock<Duration> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        let ms = std::env::var("VICTORY_PIPELINE_COMPUTE_WAIT_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&ms| ms > 0)
            .unwrap_or(10_000);
        Duration::from_millis(ms)
    })
}

/// How long the dispatches of one frame wait for compute pipelines in all
/// (`VICTORY_PIPELINE_COMPUTE_WAIT_FRAME_MS`, default 8,000; 0 or unreadable:
/// the default). A frame that first dispatches dozens of compute programs
/// (the world's, on a cold shader cache) waits for each in turn: each is
/// within [`compute_wait_limit`], all of them together ran past the title's
/// 30 s watchdog (fork issue 1420, pure fork b1d777a8 at frame 1951 with
/// 12 of 16 processors busy making them). Past the frame's budget the
/// dispatches are dropped as with `VICTORY_PIPELINE_COMPUTE_WAIT=0`; the
/// pipelines are made meanwhile and wait again in the next frame, whose
/// budget starts again.
fn compute_frame_budget() -> Duration {
    static LIMIT: OnceLock<Duration> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        let ms = std::env::var("VICTORY_PIPELINE_COMPUTE_WAIT_FRAME_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&ms| ms > 0)
            .unwrap_or(8_000);
        Duration::from_millis(ms)
    })
}

/// The title's frame the budget is counted for, the time the dispatches of
/// it waited so far, and whether the frame's drop was logged.
static FRAME_WAITED: Mutex<(u64, Duration, bool)> = Mutex::new((u64::MAX, Duration::ZERO, false));

/// Waits for the worker making a compute pipeline (queued by the caller),
/// at most [`compute_wait_limit`] and within the frame's budget
/// ([`compute_frame_budget`]). The workers take requested compute pipelines
/// before the mesh and the early ones.
fn wait_compute(p: &Pending, frame: u64) {
    let left = {
        let mut w = FRAME_WAITED.lock().unwrap_or_else(|e| e.into_inner());
        if w.0 != frame {
            *w = (frame, Duration::ZERO, false);
        }
        compute_frame_budget().saturating_sub(w.1)
    };
    let limit = compute_wait_limit().min(left);
    if limit.is_zero() {
        // The frame's budget is spent: this dispatch is dropped, and the
        // pipeline may wait in a later frame.
        p.waited_frame.store(u64::MAX, Ordering::Relaxed);
        let mut w = FRAME_WAITED.lock().unwrap_or_else(|e| e.into_inner());
        if w.0 == frame && !w.2 {
            w.2 = true;
            tracing::warn!(
                frame,
                ms = compute_frame_budget().as_millis() as u64,
                "the compute pipelines a frame's dispatches wait for are over the frame's budget: its other dispatches are dropped, the pipelines are made meanwhile (VICTORY_PIPELINE_COMPUTE_WAIT_FRAME_MS)"
            );
        }
        return;
    }
    let waiting = crate::timeline::span("pipeline: a dispatch waits for its compute pipeline");
    let started = Instant::now();
    let (lock, done) = &DONE;
    let mut held = lock.lock().unwrap_or_else(|e| e.into_inner());
    while p.made.get().is_none() {
        let rest = limit.saturating_sub(started.elapsed());
        if rest.is_zero() {
            break;
        }
        // Short turns: a pipeline made by a thread that did not notify is seen too.
        held = done
            .wait_timeout(held, rest.min(Duration::from_millis(250)))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    drop(held);
    waiting.end();
    {
        let mut w = FRAME_WAITED.lock().unwrap_or_else(|e| e.into_inner());
        if w.0 == frame {
            w.1 += started.elapsed();
        }
    }
    if p.made.get().is_none() {
        tracing::warn!(
            shaders = ?p.shaders,
            ms = limit.as_millis() as u64,
            "a compute pipeline not made within the dispatch's wait: the dispatch is dropped, the pipeline is made meanwhile (VICTORY_PIPELINE_COMPUTE_WAIT_MS)"
        );
    }
}

/// Makes `p` on this thread if it is not made yet (waiting for a worker
/// that is making it); whether it has a host pipeline.
pub(crate) fn now(p: &Arc<Pending>) -> bool {
    if p.left_out.is_some() {
        return false;
    }
    p.make();
    p.made.get().is_some_and(|made| made.is_some())
}

/// At the title's creation of a pipeline: starts it at once if an earlier
/// run drew with it (during loading, so the world's first frames find it),
/// or if it is a confirmed opening-scene / bake producer, or if it is a
/// compute pipeline: its first dispatch waits for it, and a frame that
/// first dispatches hundreds of them (the world's) waited minutes.
pub(crate) fn early(p: &Arc<Pending>) {
    if p.left_out.is_some() {
        return;
    }
    if !p.shaders.is_empty()
        && (p.scene_input || p.compute() || super::library::wanted_early(&p.key))
    {
        super::library::EARLY.fetch_add(1, Ordering::Relaxed);
        start(p, if p.scene_input { REQUESTED } else { EARLY });
    }
}

/// Whether the frame's budget is one for all threads together: a thread
/// that waits takes what is left of it and gives back what it did not
/// use. Counted only when a wait had ended, the budget was one per thread:
/// each of the title's five recording threads began its first wait of a
/// frame with nothing counted yet and waited the whole half second, 2.5 s
/// of waiting in every frame in which pipelines were being made (runs
/// 34-49: "2504 ms waiting for pipelines" in each such frame).
/// On by default; `VICTORY_PIPELINE_WAIT_SHARED=0` counts as before.
fn shared_budget() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_WAIT_SHARED").is_none_or(|v| v != "0"))
}

/// LOCAL (the problem of fxceless issue #1181): whether one pipeline is
/// waited for at most one budget over the whole run, not one budget every
/// frame. A pipeline whose make takes longer than the budget (a driver
/// compile of tens of seconds, or one that never ends) otherwise cost the
/// whole budget in every frame in which a draw asked for it: with
/// `VICTORY_PIPELINE_WAIT_MS=10000` the picture stood still while the thing
/// drawn with it was in view, and moved again when the camera turned away.
/// After its budget its draws are dropped until it is made, as with any
/// pipeline past the frame's budget. On by default;
/// `VICTORY_PIPELINE_WAIT_ONCE=0` waits as before.
fn wait_once() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_WAIT_ONCE").is_none_or(|v| v != "0"))
}

/// Waits for `p` while frame `frame` has budget left.
fn wait(p: &Pending, frame: u64) {
    let screen = p.screen && screens_first();
    let budget = if screen { screen_budget() } else { wait_budget(frame) };
    if budget.is_zero() {
        return;
    }
    // LOCAL: what is left of this pipeline's own budget ([`wait_once`]).
    let own = if wait_once() {
        let own = budget.saturating_sub(Duration::from_micros(p.waited_us.load(Ordering::Relaxed)));
        if own.is_zero() {
            return;
        }
        own
    } else {
        Duration::MAX
    };
    // The frame the budget belongs to, and what was waited of it (or is
    // taken by a thread waiting now); of the passes over the screen too.
    static SPENT: [Mutex<(u64, Duration)>; 2] = [const { Mutex::new((u64::MAX, Duration::ZERO)) }; 2];
    let spent_of = &SPENT[usize::from(screen)];
    let shared = shared_budget();
    let left = {
        let mut spent = spent_of.lock().unwrap_or_else(|e| e.into_inner());
        if spent.0 != frame {
            *spent = (frame, Duration::ZERO);
        }
        let left = budget.saturating_sub(spent.1).min(own);
        if shared {
            spent.1 += left;
        }
        left
    };
    if left.is_zero() {
        return;
    }
    let waiting = crate::timeline::span("pipeline: first use waits for a host pipeline");
    let started = Instant::now();
    let (lock, done) = &DONE;
    let mut held = lock.lock().unwrap_or_else(|e| e.into_inner());
    while p.made.get().is_none() {
        let rest = left.saturating_sub(started.elapsed());
        if rest.is_zero() {
            break;
        }
        held = done.wait_timeout(held, rest).unwrap_or_else(|e| e.into_inner()).0;
    }
    drop(held);
    let waited = started.elapsed().min(left);
    waiting.end();
    if wait_once() && p.made.get().is_none() {
        let before = p.waited_us.fetch_add(waited.as_micros() as u64, Ordering::Relaxed);
        if Duration::from_micros(before) + waited >= budget {
            tracing::info!(
                shaders = ?p.shaders,
                ms = budget.as_millis() as u64,
                "a pipeline not made within its wait: its draws are dropped until it is (VICTORY_PIPELINE_WAIT_ONCE=0: waited for every frame)"
            );
        }
    }
    if screen && waited > Duration::from_millis(500) {
        tracing::info!(
            ms = waited.as_millis() as u64,
            made = p.made.get().is_some(),
            shaders = ?p.shaders,
            "a pass over the screen was waited for (VICTORY_SCREEN_WAIT_MS=0: dropped past half a second a frame)"
        );
    }
    {
        let mut spent = spent_of.lock().unwrap_or_else(|e| e.into_inner());
        // A frame that began meanwhile has its own budget.
        if spent.0 == frame {
            if shared {
                spent.1 = spent.1.saturating_sub(left - waited);
            } else {
                spent.1 += waited;
            }
        }
    }
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(
        crate::copystat::C::PipelineWaitUs,
        started.elapsed().as_micros() as u64,
    );
}

const NOT_QUEUED: u8 = 0;
const EARLY: u8 = 1;
const REQUESTED: u8 = 2;
const MAKING: u8 = 3;

/// How many workers may make pipelines at once that are known to take long
/// and that no draw has asked for: a quarter of them. On a fresh cache (a
/// change of the translator or the driver) the early list begins with
/// programs that take tens of seconds each: all 16 workers were in them
/// through the whole of the title's loading, 97 pipelines made in 20 s, and
/// the first minute and a half of the world ran at 0.5 to 1.6 s a frame,
/// dropping up to 2,900 draws a frame (run shaders-1).
/// `VICTORY_PIPELINE_SLOW=0`: in the order the title creates them, as before.
fn slow_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        if std::env::var_os("VICTORY_PIPELINE_SLOW").is_some_and(|v| v == "0") {
            usize::MAX
        } else {
            (worker_count(None, std::thread::available_parallelism().map_or(2, |n| n.get())) / 4).max(1)
        }
    })
}

struct Queue {
    /// Requested passes over the screen: before all others, and the only ones
    /// [`SCREEN_WORKERS`] take.
    screen: VecDeque<Arc<Pending>>,
    early: VecDeque<Arc<Pending>>,
    /// Those of the early ones known to take long, and how many of them are
    /// being made ([`slow_limit`]).
    slow: VecDeque<Arc<Pending>>,
    slow_now: usize,
    /// Depth-only / shadow-map prewarmed pipelines: rebuilt ahead of other
    /// early pipelines while no draw is actively requesting one.
    early_depth: VecDeque<Arc<Pending>>,
    /// Requested vertex/pixel graphics pipelines (including screen composition
    /// and tonemapping): compiled ahead of compute/mesh so the 3D image shows
    /// up quickly when the shader cache is cold.
    requested_graphics: VecDeque<Arc<Pending>>,
    /// Requested compute pipelines (lighting resolve, tonemap, post-processing):
    /// compiled ahead of mesh pipelines so the 3D screen lights up quickly.
    requested_compute: VecDeque<Arc<Pending>>,
    requested: VecDeque<Arc<Pending>>,
}

/// Whether `p` is a traditional vertex graphics pipeline (as opposed to mesh
/// or compute).
fn is_vertex_graphics(p: &Pending) -> bool {
    p.shaders.iter().any(|s| s.stage == "vertex")
}

/// Whether `p` is a compute pipeline.
fn is_compute(p: &Pending) -> bool {
    p.shaders.iter().any(|s| s.stage == "compute")
}

impl Queue {
    const fn new() -> Self {
        Self {
            screen: VecDeque::new(),
            early: VecDeque::new(),
            slow: VecDeque::new(),
            slow_now: 0,
            early_depth: VecDeque::new(),
            requested_graphics: VecDeque::new(),
            requested_compute: VecDeque::new(),
            requested: VecDeque::new(),
        }
    }

    /// Queues an unrequested pipeline or promotes one queued at creation.
    /// Returns whether it is new to the queue, or `None` if nothing changed.
    fn push(&mut self, p: &Arc<Pending>, priority: u8) -> Option<bool> {
        self.push_as(p, priority, slow_limit() != usize::MAX && super::library::slow(&p.key))
    }

    fn push_as(&mut self, p: &Arc<Pending>, priority: u8, slow: bool) -> Option<bool> {
        let prev = p.queue_state.load(Ordering::Relaxed);
        if prev >= priority {
            return None;
        }
        p.queue_state.store(priority, Ordering::Relaxed);
        if priority == REQUESTED && p.screen && screens_first() {
            self.screen.push_back(p.clone());
        } else if priority == REQUESTED {
            if is_vertex_graphics(p) {
                self.requested_graphics.push_back(p.clone());
            } else if is_compute(p) {
                self.requested_compute.push_back(p.clone());
            } else {
                self.requested.push_back(p.clone());
            }
        } else if slow {
            self.slow.push_back(p.clone());
        } else if p.graphics.as_ref().is_some_and(|state| state.writes_depth_only()) {
            self.early_depth.push_back(p.clone());
        } else {
            self.early.push_back(p.clone());
        }
        Some(prev == NOT_QUEUED)
    }

    fn pop(&mut self, screens_only: bool, slow_limit: usize) -> Option<Arc<Pending>> {
        // An early entry can have been promoted and claimed already.
        let unclaimed = |list: &mut VecDeque<Arc<Pending>>| {
            std::iter::from_fn(|| list.pop_front()).find(|p| p.queue_state.load(Ordering::Relaxed) == EARLY)
        };
        let mut p = self.screen.pop_front();
        if p.is_none() && !screens_only {
            p = self
                .requested_graphics
                .pop_front()
                .or_else(|| self.requested_compute.pop_front())
                .or_else(|| self.requested.pop_front());
            if p.is_none() {
                p = unclaimed(&mut self.early_depth);
            }
            if p.is_none() {
                p = unclaimed(&mut self.early);
            }
            if p.is_none() && self.slow_now < slow_limit {
                p = unclaimed(&mut self.slow);
                if let Some(p) = &p {
                    p.was_slow.store(true, Ordering::Relaxed);
                    self.slow_now += 1;
                }
            }
        }
        let p = p?;
        p.queue_state.store(MAKING, Ordering::Relaxed);
        Some(p)
    }
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());
static AVAILABLE: Condvar = Condvar::new();

/// Start or promote work for a draw without waiting behind the entire early
/// list. Queue changes and worker claims happen under the same lock.
fn start(p: &Arc<Pending>, priority: u8) {
    if p.queue_state.load(Ordering::Relaxed) >= priority {
        return;
    }
    if !workers() {
        if p.queue_state.swap(MAKING, Ordering::Relaxed) == NOT_QUEUED {
            p.make();
        }
        return;
    }
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(new) = queue.push(p, priority) {
        if new {
            PEAK.fetch_max(WAITING.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Relaxed);
        }
        // (One woken could be a worker that takes passes over the screen
        // alone: it would leave the pipeline to nobody.)
        AVAILABLE.notify_all();
    }
}

/// Workers that make passes over the screen alone, besides the others.
const SCREEN_WORKERS: usize = 2;

fn next(screens_only: bool) -> Arc<Pending> {
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(p) = queue.pop(screens_only, slow_limit()) {
            return p;
        }
        queue = AVAILABLE.wait(queue).unwrap_or_else(|e| e.into_inner());
    }
}

/// Stack of each pipeline worker: room for the driver's shader compiler,
/// which runs on it (reserved address space; committed only as used).
const PIPELINE_STACK: usize = 64 << 20;

/// How many workers make pipelines: `asked` (`VICTORY_PIPELINE_WORKERS`, 1 to
/// 64), else two thirds of the logical processors, 2 to 16. They run below the
/// title's own threads, and a cold cache queues hundreds at once.
fn worker_count(asked: Option<usize>, processors: usize) -> usize {
    worker_count_for(asked, processors, false)
}

fn worker_count_for(asked: Option<usize>, processors: usize, limited_ram: bool) -> usize {
    match asked {
        Some(n) => n.clamp(1, 64),
        None if limited_ram => (processors * 3 / 4).clamp(2, 8),
        None => (processors * 2 / 3).clamp(2, 16),
    }
}

/// Starts the workers ([`worker_count`]) on first use.
fn workers() -> bool {
    static RUNNING: OnceLock<bool> = OnceLock::new();
    *RUNNING.get_or_init(|| {
        let mut running = false;
        let workers = worker_count_for(
            std::env::var("VICTORY_PIPELINE_WORKERS")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            std::thread::available_parallelism().map_or(2, |n| n.get()),
            crate::residency::limited_host_ram(),
        );
        tracing::info!(workers, "pipeline workers");
        let screens = if screens_first() { SCREEN_WORKERS } else { 0 };
        guard_start();
        for i in 0..workers + screens {
            if spawn_worker(i >= workers) {
                running = true;
            }
        }
        running
    })
}

/// Starts one worker: one for passes over the screen alone, or one of the
/// others.
fn spawn_worker(screens_only: bool) -> bool {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name(format!("victory-pipelines-{slot}"))
        .stack_size(PIPELINE_STACK)
        .spawn(move || {
            WORKER_SLOT.set(slot);
            // Below the title's own threads (loading, recording),
            // and the streaming one, which is below normal itself
            // (`VICTORY_PIPELINE_LOWEST=0`: level with it, as before).
            let lowest = !screens_only && std::env::var_os("VICTORY_PIPELINE_LOWEST").is_none_or(|v| v != "0");
            // SAFETY: a plain call on our own thread.
            unsafe {
                use windows::Win32::System::Threading::*;
                let priority = if lowest { THREAD_PRIORITY_LOWEST } else { THREAD_PRIORITY_BELOW_NORMAL };
                let _ = SetThreadPriority(GetCurrentThread(), priority);
            }
            loop {
                let p = next(screens_only);
                {
                    // (If the compiler crashes in here this thread never
                    // returns: `lost` does what follows for it.)
                    let _compile = Compile::begin(slot, &p, screens_only);
                    p.make();
                }
                if p.was_slow.load(Ordering::Relaxed) {
                    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
                    queue.slow_now -= 1;
                    AVAILABLE.notify_all();
                }
                if WAITING.fetch_sub(1, Ordering::Relaxed) == 1 {
                    drained();
                } else {
                    super::library::save(false);
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "no pipeline worker");
        false
    } else {
        true
    }
}

/// The most pipelines waiting at once since the workers last ran out.
static PEAK: AtomicU64 = AtomicU64::new(0);

/// The workers ran out of work: logged after a batch of 10 or more, and
/// the cache saved (at most every 20 s).
fn drained() {
    use super::library::{EARLY, LOADED, MADE};
    if PEAK.swap(0, Ordering::Relaxed) >= 10 {
        tracing::info!(
            made = MADE.load(Ordering::Relaxed),
            loaded = LOADED.load(Ordering::Relaxed),
            started_early = EARLY.load(Ordering::Relaxed),
            "host pipelines all made"
        );
    }
    super::library::save(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panicking_pipeline_is_refused_and_the_worker_continues() {
        let failed = Arc::new(Pending::new(Vec::new(), None));
        let next = Arc::new(Pending::new(Vec::new(), None));
        let failed_job = Arc::clone(&failed);
        let next_job = Arc::clone(&next);
        std::thread::spawn(move || {
            failed_job.make_with(|| panic!("synthetic pipeline build panic"));
            next_job.make_with(|| None);
        })
        .join()
        .expect("the pipeline worker should survive");

        assert!(matches!(failed.made.get(), Some(None)));
        assert!(matches!(next.made.get(), Some(None)));
        let mut rebuilt = false;
        failed.make_with(|| {
            rebuilt = true;
            None
        });
        assert!(!rebuilt);
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn missing_distinguishes_omitted_and_pending_pipelines() {
        let reason = "ray activation shader without BVH, left out";
        let mut omitted = Pending::new(Vec::new(), None);
        omitted.left_out = Some(reason);
        assert_eq!(missing(&omitted).as_deref(), Some(reason));

        let pending = Pending::new(Vec::new(), None);
        pending.queue_state.store(MAKING, Ordering::Relaxed);
        assert_eq!(missing(&pending).as_deref(), Some("being made"));
    }

    #[test]
    fn worker_count_follows_the_processors_and_the_switch() {
        assert_eq!(worker_count(None, 1), 2);
        assert_eq!(worker_count(None, 24), 16);
        assert_eq!(worker_count(None, 64), 16);
        assert_eq!(worker_count(Some(0), 24), 1);
        assert_eq!(worker_count(Some(20), 24), 20);
        assert_eq!(worker_count_for(None, 8, true), 6);
        assert_eq!(worker_count_for(None, 16, true), 8);
        assert_eq!(worker_count_for(None, 24, true), 8);
        assert_eq!(worker_count_for(Some(12), 8, true), 12);
    }

    /// A first use queues the pipeline and, within the budget, gets the
    /// worker's answer (here: no host pipeline for no shaders).
    #[test]
    fn first_use_is_made_by_a_worker() {
        let p = Arc::new(Pending::new(Vec::new(), None));
        assert!(ready(&p).is_none());
        assert_ne!(p.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        let started = Instant::now();
        while p.made.get().is_none() && started.elapsed() < Duration::from_secs(10) {
            std::thread::yield_now();
        }
        assert!(matches!(p.made.get(), Some(None)));
    }

    /// A draw into other formats than its pipeline declares finds the
    /// pipeline for them, which exists once per set of formats and is
    /// queued by its first draw like any other.
    #[test]
    fn a_pipeline_for_bound_formats_is_made_once() {
        // The title's pipeline declares one render target, format 24.
        let p = Arc::new(Pending::new(Vec::new(), Some(Box::new(GraphicsState::declaring(24)))));
        assert_eq!(p.formats, [24, 0, 0, 0, 0, 0, 0, 0, 0]);
        let g = p.graphics.as_deref().unwrap();
        let (a, b) = ([26, 0, 0, 0, 0, 0, 0, 0, 0], [28, 0, 0, 0, 0, 0, 0, 0, 0]);
        let first = variant(&p, g, &a);
        assert_eq!(first.formats, a);
        assert_ne!(first.key, p.key);
        assert!(Arc::ptr_eq(first, variant(&p, g, &a)));
        let second = variant(&p, g, &b);
        assert_eq!(second.formats, b);
        assert!(!Arc::ptr_eq(first, second));
        assert_ne!(second.key, first.key);
        assert!(Arc::ptr_eq(first, variant(&p, g, &a)));
        assert!(Arc::ptr_eq(second, variant(&p, g, &b)));
        assert_eq!(first.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        assert_eq!(p.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
    }

    /// A pipeline the compiler crashed on is not made again: no compile.
    #[test]
    fn a_pipeline_on_the_bad_list_is_not_compiled() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "bad-pipeline-test".to_owned();
        bad_keys(|keys| keys.insert(p.key.clone()));
        let before = SKIPPED_BAD.load(Ordering::Relaxed);
        p.make_guarded();
        assert!(matches!(p.made.get(), Some(None)));
        assert!(SKIPPED_BAD.load(Ordering::Relaxed) > before);
    }

    #[test]
    fn pending_pipeline_is_recorded_for_the_next_launch() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "pending-pipeline-test".to_owned();
        // Simulate a worker that has taken the job but cannot finish it yet.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let p = Arc::new(p);

        assert!(!super::super::library::wanted_early(&p.key));
        // A compute pipeline is made at once when dispatches wait for theirs
        // (#1312, the default; it has no shaders here, so nothing comes of
        // it); either way it is noted for the next launch.
        assert!(ready(&p).is_none());
        assert!(super::super::library::wanted_early(&p.key));
    }

    #[test]
    fn draws_promote_early_pipelines_without_claiming_them_twice() {
        let earlier = Arc::new(Pending::new(Vec::new(), None));
        let promoted = Arc::new(Pending::new(Vec::new(), None));
        let requested = Arc::new(Pending::new(Vec::new(), None));
        let mut queue = Queue::new();

        assert_eq!(queue.push(&earlier, EARLY), Some(true));
        assert_eq!(queue.push(&promoted, EARLY), Some(true));
        assert_eq!(queue.push(&requested, REQUESTED), Some(true));
        assert_eq!(queue.push(&promoted, REQUESTED), Some(false));
        assert_eq!(queue.push(&promoted, REQUESTED), None);

        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &requested));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &promoted));
        assert_eq!(queue.push(&promoted, REQUESTED), None);
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &earlier));
        assert!(queue.pop(false, 1).is_none());
    }

    #[test]
    fn pipelines_known_to_take_long_leave_workers_for_the_others() {
        let new = || Arc::new(Pending::new(Vec::new(), None));
        let (long, longer, asked, quick) = (new(), new(), new(), new());
        let mut queue = Queue::new();
        for p in [&long, &longer, &asked] {
            assert_eq!(queue.push_as(p, EARLY, true), Some(true));
        }
        assert_eq!(queue.push_as(&quick, EARLY, false), Some(true));
        // The quick one first, then one long one at a time; one a draw asks
        // for waits for nobody.
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &quick));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &long));
        assert!(queue.pop(false, 1).is_none());
        assert_eq!(queue.push_as(&asked, REQUESTED, true), Some(false));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &asked));
        assert!(!asked.was_slow.load(Ordering::Relaxed));
        assert!(queue.pop(false, 1).is_none());
        queue.slow_now -= 1;
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &longer));
        assert!(queue.pop(false, 1).is_none());
    }

    /// Passes over the screen go before all others, and workers of their own
    /// take nothing else.
    #[test]
    fn passes_over_the_screen_go_first() {
        let of = |screen| {
            let mut p = Pending::new(Vec::new(), None);
            p.screen = screen;
            Arc::new(p)
        };
        let (world, screen) = (of(false), of(true));
        let mut queue = Queue::new();
        assert_eq!(queue.push(&world, REQUESTED), Some(true));
        assert_eq!(queue.push(&screen, REQUESTED), Some(true));
        assert!(Arc::ptr_eq(&queue.pop(true, 1).unwrap(), &screen));
        assert!(queue.pop(true, 1).is_none());
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &world));
    }

    #[test]
    fn slow_pipeline_waits_at_most_one_frame() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "slow-pipeline-one-frame-test".to_owned();
        // Simulate a pipeline already claimed by a worker that has not finished.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let first_frame = crate::frame::current();
        // Pretend it already exhausted its wait on another frame (avoid u64::MAX sentinel).
        let earlier_frame = if first_frame == 0 { 1 } else { first_frame - 1 };
        p.waited_frame.store(earlier_frame, Ordering::Relaxed);
        let p = Arc::new(p);
        let started = Instant::now();
        assert!(ready(&p).is_none());
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "subsequent frames must not block for the pipeline wait budget"
        );
    }


    #[test]
    fn skipped_hash_leaves_pipeline_unqueued_and_unmatched_queues_normally() {
        let shader = |hash: &str| {
            Arc::new(Shader {
                stage: "compute",
                hash: hash.to_owned(),
                container: Vec::new(),
                #[cfg(windows)]
                ambient: None,
                #[cfg(windows)]
                probe: None,
                #[cfg(windows)]
                bakes: false,
                #[cfg(windows)]
                translated: OnceLock::from(None),
                #[cfg(windows)]
                resolves_light: false,
                #[cfg(windows)]
                ends_reflections: false,
                #[cfg(windows)]
                draws_sky: false,
            })
        };
        let skips = super::super::parse_skip_hashes("32ed072b9f845048");

        let skipped = Arc::new(Pending::new_with_skip_hashes(
            vec![shader("32ed072b9f845048")],
            None,
            &skips,
        ));
        assert_eq!(skipped.left_out, Some("skipped by VICTORY_SKIP_PIPELINES"));
        early(&skipped);
        assert!(ready(&skipped).is_none());
        assert_eq!(skipped.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        assert!(skipped.made.get().is_none());
        #[cfg(feature = "diagnostics")]
        assert_eq!(
            missing(&skipped).as_deref(),
            Some("skipped by VICTORY_SKIP_PIPELINES")
        );

        let normal = Arc::new(Pending::new_with_skip_hashes(
            vec![shader("0011223344556677")],
            None,
            &skips,
        ));
        assert_eq!(normal.left_out, None);
        assert!(ready(&normal).is_none());
        assert_ne!(normal.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        let started = Instant::now();
        while normal.made.get().is_none() && started.elapsed() < Duration::from_secs(10) {
            std::thread::yield_now();
        }
        assert!(matches!(normal.made.get(), Some(None)));
    }

}
