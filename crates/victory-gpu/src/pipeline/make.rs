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
//! The default budget is 12 s per title frame and kind, a window the
//! recording threads wait in together ([`DEFAULT_WAIT_MS`]); it was half
//! a second. A pass the title runs only once and whose pipeline takes
//! longer is lost: while a world loads the title dispatches the passes
//! that unpack its models once. With a budget set below the defaults, a
//! frame may wait longer for a pipeline while a run's loading allowance
//! lasts ([`frame_budget`]), and never for one the driver has held for
//! long ([`held_too_long`]).
//! Ported from the sibling fork `joshuu/victory-amd`'s 4436f8f1 (its
//! loading waits only).
//! Passes over the screen (no vertex input, no depth) have workers and a
//! longer budget of their own ([`screen_budget`]): the title fills its
//! lighting tables with such draws once, at the world's first frame, and
//! behind a world's worth of pipelines they were dropped; that run had no
//! lamps and no diffuse light to its end (run textures-d2, the first after a
//! new translation; Belobug 18e57db8 found the same).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Shader;
use super::host::{GraphicsState, HostPipeline, TargetFormats};

/// What a host pipeline is made from, and the host pipeline once made.
pub(crate) struct Pending {
    shaders: Vec<Arc<Shader>>,
    pub(crate) graphics: Option<Box<GraphicsState>>,
    queue_state: AtomicU8,
    /// Taken by a worker from [`Queue::slow`].
    was_slow: AtomicBool,
    /// Draws and dispatches that asked for it and found it not made: the
    /// workers make the most wanted one first.
    asked: AtomicU64,
    made: OnceLock<Option<HostPipeline>>,
    /// What identifies it across runs (shader containers and state), and
    /// whether a draw or dispatch has requested it yet.
    pub(super) key: String,
    used: AtomicBool,
<<<<<<< HEAD
    /// A pass over the screen ([`GraphicsState::screen_pass`]).
    screen: bool,
    /// When a worker began it ([`slow`]).
    begun: OnceLock<Instant>,
    /// Why it is not made, if it is not ([`left_out`]).
    left_out: Option<&'static str>,
}

/// Why a pipeline with these shaders is not made, if it is not: what
/// `VICTORY_RT_RESOLVE=0` and `VICTORY_ACTIVATE_RAYS=0` leave out (both made
/// by default).
fn left_out(shaders: &[Arc<Shader>]) -> Option<&'static str> {
    if without_structures(shaders) {
        Some("shades ray-tracing hits, left out (VICTORY_RT_RESOLVE=0)")
    } else if too_dear(shaders) {
        Some("too dear to compile, left out (VICTORY_ACTIVATE_RAYS=0)")
    } else {
        None
    }
}

/// Whether some compute shader of `shaders` holds `name`.
fn named(shaders: &[Arc<Shader>], name: &[u8]) -> bool {
    shaders
        .iter()
        .any(|s| s.stage == "compute" && s.container.windows(name.len()).any(|w| w == name))
}

/// Whether a shader is left out for what it costs the host's driver:
/// `CS_ScreenProbes_ActivateRays` (39f0e9aeb156a61c, 66,501 instructions,
/// 9,948 branches). As translated before the clean-up passes
/// (`emit::shader_opt`), AMD's took a core and 0.8 GB more every 10 s for it
/// and did not finish (RX 7800 XT: 47 GB after 8 minutes); cleaned up, it
/// takes 85 s and 1.3 GB, once. `VICTORY_ACTIVATE_RAYS=0` leaves it out.
fn too_dear(shaders: &[Arc<Shader>]) -> bool {
    static OUT: OnceLock<bool> = OnceLock::new();
    let out = *OUT.get_or_init(|| std::env::var_os("VICTORY_ACTIVATE_RAYS").is_some_and(|v| v == "0"));
    out && named(shaders, b"CS_ScreenProbes_ActivateRays")
}

/// Whether a shader only shades what the console's ray-tracing structures
/// were hit by: about 1,000 compute programs of the title
/// (`CS_RaytracingResolve`, the `raytracingresolve_dispatch` pass of each
/// material), all bound at the world's first frame, each 5 to 9 s of AMD's
/// compiler. They stand in the queue before the passes that make the
/// picture, and with the structure builder off there is no hit to shade. `VICTORY_RT_RESOLVE=0` leaves them out, unless the
/// builder is on (off here by default; the fork this is from,
/// myokex/victory-amd, leaves them out unless told otherwise).
fn without_structures(shaders: &[Arc<Shader>]) -> bool {
    static OUT: OnceLock<bool> = OnceLock::new();
    let is = |name, value| std::env::var_os(name).is_some_and(|v| v == value);
    let out = *OUT.get_or_init(|| is("VICTORY_RT_RESOLVE", "0") && !is("VICTORY_BVH_BUILDER", "1"));
    out && named(shaders, b"CS_RaytracingResolve")
||||||| 9c184b35
=======
    /// The formats its state declares for what a draw draws into (zeros
    /// without a graphics state).
    pub(crate) formats: TargetFormats,
    /// The same shaders and state for other formats, bound at a draw
    /// ([`for_formats`]): a chain from the title's pipeline, one link per
    /// set of formats met, never shortened.
    variant: OnceLock<Arc<Pending>>,
>>>>>>> fork/main
}

// SAFETY: D3D12 objects are free-threaded; the graphics state (whose input
// elements point into its own names) is only read after creation.
unsafe impl Send for Pending {}
// SAFETY: as above.
unsafe impl Sync for Pending {}

impl Pending {
    pub(crate) fn new(shaders: Vec<Arc<Shader>>, graphics: Option<Box<GraphicsState>>) -> Pending {
        let key = key(&shaders, graphics.as_deref());
        let left_out = left_out(&shaders);
        Pending {
<<<<<<< HEAD
            screen: graphics.as_ref().is_some_and(|g| g.screen_pass()),
||||||| 9c184b35
=======
            formats: graphics.as_ref().map_or([0; 9], |g| g.formats()),
            variant: OnceLock::new(),
>>>>>>> fork/main
            shaders,
            graphics,
            left_out,
            queue_state: AtomicU8::new(NOT_QUEUED),
            was_slow: AtomicBool::new(false),
            asked: AtomicU64::new(0),
            made: OnceLock::new(),
            key,
            used: AtomicBool::new(false),
            begun: OnceLock::new(),
        }
    }

    /// Whether it is a compute pipeline.
    fn computes(&self) -> bool {
        matches!(self.shaders.as_slice(), [only] if only.stage == "compute")
    }

    /// Whether the host's driver has been at it for longer than a frame
    /// waits for ([`held_too_long`]).
    fn slow(&self) -> bool {
        self.begun.get().is_some_and(|at| held_too_long(at.elapsed(), self.computes(), wait_budget()))
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }

    fn make(&self) {
        let _ = self.begun.set(Instant::now());
        self.make_with(|| HostPipeline::new(&self.shaders, self.graphics.as_deref()));
    }

    /// A panic on the way is a pipeline the host does not have: let out, it
    /// would end the worker for good and leave `made` unset, so that every
    /// later frame waited for it (and, on a title's thread, end the run).
    fn make_with(&self, new: impl FnOnce() -> Option<HostPipeline>) {
        self.made.get_or_init(|| {
            let began = Instant::now();
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(new)).unwrap_or_else(|_| {
                tracing::error!(
                    key = %self.key,
                    shaders = ?self.shaders,
                    "making a host pipeline panicked; its draws are dropped"
                );
                None
            });
            super::library::note_time(&self.key, began.elapsed().as_millis() as u64);
            made
        });
        let (lock, done) = &DONE;
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        done.notify_all();
    }
}

/// Identifies the title pipeline by its shader containers and graphics state.
pub(crate) fn key(shaders: &[Arc<Shader>], graphics: Option<&GraphicsState>) -> String {
    let mut key = blake3::Hasher::new();
    for shader in shaders {
        key.update(shader.stage.as_bytes());
        key.update(shader.hash.as_bytes());
    }
    key.update(graphics.map(GraphicsState::describe).unwrap_or_default().as_bytes());
    key.finalize().to_hex()[..32].to_owned()
}

/// Why a pipeline has no host pipeline (diagnostics): its shaders that do
/// not translate, else "refused" or "being made"; `None` if it has one.
pub(crate) fn missing(p: &Pending) -> Option<String> {
    match p.made.get() {
        Some(Some(_)) => None,
        None => Some(p.left_out.map_or_else(
            || format!("being made {:?}", p.shaders),
            str::to_owned,
        )),
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

/// Waiting allowed per title frame, over all threads.
fn wait_budget() -> Duration {
    #[cfg(test)]
    if let Some(budget) = TEST_BUDGET.get() {
        return budget;
    }
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_PIPELINE_WAIT_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_WAIT_MS);
        tracing::info!(millis, "host pipeline wait budget per frame, per kind");
        Duration::from_millis(millis)
    })
}

/// The default of [`wait_budget`]: the picture before frame rate. A frame
/// waits for pipelines still being made rather than drop their draws and
/// dispatches, up to 12 s for each kind (graphics, compute), the threads
/// together ([`Sharing::Window`]). At worst a frame then waits 24 s, both
/// kinds' windows used up one after the other, under the title's GPU
/// watchdog, which ends a run whose frame takes more than 30 s (decision
/// 0007). Only a pipeline still not made after its frame's window loses
/// that frame's draws. `VICTORY_PIPELINE_WAIT_MS=500` waits as before.
const DEFAULT_WAIT_MS: u64 = 12_000;

/// The title's GPU watchdog: one frame of more than this ends the run.
const TITLE_WATCHDOG: Duration = Duration::from_secs(30);

// Both kinds' windows, one after the other, stay under the watchdog.
const _: () = assert!(2 * DEFAULT_WAIT_MS < TITLE_WATCHDOG.as_millis() as u64);

#[cfg(test)]
thread_local! {
    /// A test's own frame budget, on its thread alone.
    static TEST_BUDGET: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// What a frame may wait for one kind of pipeline while a world loads: they
/// are asked for by the hundred then, and a pass the title runs once is
/// lost if its pipeline is not there. Compute pipelines get more, apart:
/// the passes that unpack a world's models are dispatched once (the
/// sibling fork counted 130 compute pipelines dropped while a world loaded,
/// and walls, floors, ceilings and faces missing for the session).
const LOADING_FRAME: Duration = Duration::from_secs(4);
const LOADING_FRAME_COMPUTE: Duration = Duration::from_secs(12);

/// `VICTORY_PIPELINE_GATE_S` (120): the most a run waits past
/// [`wait_budget`] for each kind, in seconds, all frames together; 0 waits
/// as before. The title's GPU watchdog bounds one frame (30 s).
fn loading_allowance() -> Duration {
    static TOTAL: OnceLock<Duration> = OnceLock::new();
    *TOTAL.get_or_init(|| {
        let seconds = std::env::var("VICTORY_PIPELINE_GATE_S")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(120);
        tracing::info!(seconds, "host pipeline loading allowance per kind");
        Duration::from_secs(seconds)
    })
}

/// A frame's budget for a pipeline of one kind: the frame budget, or while
/// the run's allowance for that kind lasts (`spent` of it gone) up to
/// [`LOADING_FRAME`] or [`LOADING_FRAME_COMPUTE`].
fn frame_budget(base: Duration, compute: bool, allowance: Duration, spent: Duration) -> Duration {
    let loading = if compute { LOADING_FRAME_COMPUTE } else { LOADING_FRAME };
    base.max(loading.min(allowance.saturating_sub(spent)))
}

/// Whether the driver has held a pipeline for longer than most take, so no
/// frame waits for it (one compute program of the title takes 85 s): 10 s
/// for a compute pipeline, 3 s for others. Only while the frame budget
/// `base` (`VICTORY_PIPELINE_WAIT_MS`) is shorter than that: a budget set
/// at or above it is the bound alone, so a pipeline still being made is
/// waited for as long as the frame's budget lasts.
fn held_too_long(held: Duration, compute: bool, base: Duration) -> bool {
    let most = Duration::from_secs(if compute { 10 } else { 3 });
    base < most && held > most
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
        p.asked.fetch_add(1, Ordering::Relaxed);
        start(p, REQUESTED);
        wait(p, crate::frame::current());
    }
    p.made.get()?.as_ref()
}

<<<<<<< HEAD
/// Starts `p` on the workers, before anything asks for it.
pub(crate) fn begin(p: &Arc<Pending>) {
    if p.left_out.is_none() {
        start(p, REQUESTED);
    }
}

/// Why each of `pipelines` that is made and is none the host has is not
/// there, for the log.
pub(crate) fn failures(pipelines: &[Arc<Pending>]) -> Vec<String> {
    pipelines
        .iter()
        .filter(|p| matches!(p.made.get(), Some(None)))
        .map(|p| {
            let untranslated: Vec<String> = p
                .shaders
                .iter()
                .filter(|s| matches!(s.translated.get(), Some(None)))
                .map(|s| format!("{} {} does not translate", s.stage, s.hash))
                .collect();
            if untranslated.is_empty() {
                format!("the host refuses {:?}", p.shaders)
            } else {
                untranslated.join(", ")
            }
        })
        .collect()
}

||||||| 9c184b35
=======
/// Pipelines made again for the formats bound at a draw, since the start.
static VARIANTS: AtomicU64 = AtomicU64::new(0);

/// Whether a draw into views of other formats than its pipeline declares
/// gets a host pipeline for those formats ([`for_formats`]). On by
/// default; `VICTORY_TARGET_VARIANTS=0` sends such a draw to the host with
/// the title's pipeline, as before.
fn variants_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_TARGET_VARIANTS").is_none_or(|v| v != "0"))
}

/// The host pipeline for a draw with `p` into views of the formats
/// `bound`, which `p` does not declare (`host::formats_differ`): that of
/// the same shaders and state for those formats. It is a pipeline like the
/// title's own: queued at its first draw, made by a worker, waited for
/// within the frame's budget, and the draw is dropped while it is not
/// there ([`ready`]).
#[cold]
pub(crate) fn for_formats<'a>(p: &'a Arc<Pending>, bound: &TargetFormats) -> Option<&'a HostPipeline> {
    match p.graphics.as_deref() {
        Some(g) if variants_on() => ready(variant(p, g, &super::host::formats_for(&p.formats, bound))),
        _ => ready(p),
    }
}

/// The pipeline of `p`'s shaders and its state `g` for the formats
/// `bound`: found in the chain that starts at `p`, or added to it (logged
/// then: once per pipeline and set of formats).
fn variant<'a>(p: &'a Arc<Pending>, g: &GraphicsState, bound: &TargetFormats) -> &'a Arc<Pending> {
    let mut at = p;
    // (`bound` is what `host::formats_for` gives, and a state made for it
    // declares exactly it, so the walk ends at the link made for it; the
    // count is a bound all the same: past it the title's own pipeline.)
    for _ in 0..64 {
        let next = at.variant.get_or_init(|| {
            let so_far = VARIANTS.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::info!(
                pipeline = ?p.shaders,
                declared = ?p.formats,
                bound = ?bound,
                so_far,
                "a pipeline is made again for the render target formats bound at its draw (eight render targets, then depth; VICTORY_TARGET_VARIANTS=0: the draw goes out with the title's pipeline)"
            );
            Arc::new(Pending::new(p.shaders.clone(), Some(Box::new(g.with_formats(bound)))))
        });
        // (A state made for `bound` declares exactly `bound`.)
        if next.formats == *bound {
            return next;
        }
        at = next;
    }
    p
}

>>>>>>> fork/main
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
/// run drew with it (during loading, so the world's first frames find it).
pub(crate) fn early(p: &Arc<Pending>) {
    // Turning off speculative starts keeps first-use scheduling and the disk caches available.
    static PREWARM: OnceLock<bool> = OnceLock::new();
    let enabled = *PREWARM.get_or_init(|| std::env::var_os("VICTORY_EARLY_PIPELINES").is_none_or(|v| v != "0"));
    if enabled && p.left_out.is_none() && !p.shaders.is_empty() && super::library::wanted_early(&p.key) {
        super::library::EARLY.fetch_add(1, Ordering::Relaxed);
        start(p, EARLY);
    }
}

/// How the threads that wait in one frame share its budget
/// (`VICTORY_PIPELINE_WAIT_SHARED`): by default a window of waiting time
/// ([`Sharing::Window`]); `reserve` the way before it, `0` the way before
/// that. Counted only when a wait had ended, the budget was one per
/// thread: each of the title's five recording threads began its first
/// wait of a frame with nothing counted yet and waited the whole half
/// second, 2.5 s of waiting in every frame in which pipelines were being
/// made (runs 34-49: "2504 ms waiting for pipelines" in each such frame).
/// Reserved, a thread that waited took the rest of the budget, and every
/// other thread's new pipeline in that frame was dropped at once: a run
/// of about 835 drops as "being made" whose frame lines show at most 9 ms
/// waited (run fork-essentials).
fn shared_budget() -> Sharing {
    static ON: OnceLock<Sharing> = OnceLock::new();
    *ON.get_or_init(|| sharing_from(std::env::var("VICTORY_PIPELINE_WAIT_SHARED").ok().as_deref()))
}

/// How the threads that wait in one frame count its budget for one kind
/// of pipeline (`VICTORY_PIPELINE_WAIT_SHARED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sharing {
    /// The budget is a window of waiting time: every thread may wait while
    /// the time in which some thread has waited in this frame is shorter,
    /// and nothing is reserved, so no thread drops a pipeline because
    /// another is waiting. The threads' waits together take at most the
    /// budget.
    Window,
    /// `reserve`: a thread that waits takes what is left of the budget and
    /// gives back what it did not use; another thread asking meanwhile
    /// gets nothing.
    Reserve,
    /// `0`: each thread counts its waits once they have ended.
    PerThread,
}

/// The way of counting the switch's value asks for.
fn sharing_from(value: Option<&str>) -> Sharing {
    match value.map(str::trim) {
        Some("0") => Sharing::PerThread,
        Some("reserve") => Sharing::Reserve,
        _ => Sharing::Window,
    }
}

/// One frame's waiting for one kind of pipeline.
#[derive(Debug)]
struct FrameWait {
    frame: u64,
    /// The waiting counted against the budget so far (the window: time in
    /// which some thread waited, up to `since`).
    spent: Duration,
    /// The window: threads waiting now, and since when one has been.
    waiting: u32,
    since: Option<Instant>,
}

impl FrameWait {
    const fn new() -> FrameWait {
        FrameWait { frame: u64::MAX, spent: Duration::ZERO, waiting: 0, since: None }
    }

    /// A thread is to wait in `frame` at `now`: how long it may.
    fn begin(&mut self, sharing: Sharing, frame: u64, budget: Duration, now: Instant) -> Duration {
        if self.frame != frame {
            *self = FrameWait { frame, ..FrameWait::new() };
        }
        if sharing == Sharing::Window {
            let open = self.since.map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
            let left = budget.saturating_sub(self.spent + open);
            if !left.is_zero() {
                if self.waiting == 0 {
                    self.since = Some(now);
                }
                self.waiting += 1;
            }
            return left;
        }
        let left = budget.saturating_sub(self.spent);
        if sharing == Sharing::Reserve {
            self.spent += left;
        }
        left
    }

    /// The thread that [`begin`](Self::begin) gave `left` (more than
    /// nothing) in `frame` has waited `waited`, and stops at `now`. A frame
    /// that began meanwhile has its own budget.
    fn end(&mut self, sharing: Sharing, frame: u64, left: Duration, waited: Duration, now: Instant) {
        if self.frame != frame {
            return;
        }
        match sharing {
            Sharing::Window => {
                self.waiting = self.waiting.saturating_sub(1);
                if self.waiting == 0
                    && let Some(at) = self.since.take()
                {
                    self.spent += now.saturating_duration_since(at);
                }
            }
            Sharing::Reserve => self.spent = self.spent.saturating_sub(left - waited),
            Sharing::PerThread => self.spent += waited,
        }
    }
}

/// What a frame's budget was spent on: the time in which any thread waited.
/// (The first thread to wait took all that was left of the budget and gave
/// back the rest afterwards: the others waited for nothing meanwhile, not
/// even for a pipeline that loads in a millisecond, and their draws were
/// dropped.)
struct Spent {
    frame: u64,
    time: Duration,
    /// Threads waiting now, and since when there were any (or since the
    /// frame began, if later).
    waiting: u32,
    since: Option<Instant>,
}

impl Spent {
    const fn new() -> Self {
        Self {
            frame: u64::MAX,
            time: Duration::ZERO,
            waiting: 0,
            since: None,
        }
    }

    /// A thread wants to wait in `frame` at `now`: how long it may, of
    /// `budget`. [`Spent::end`] follows if that is anything.
    fn begin(&mut self, frame: u64, budget: Duration, now: Instant) -> Duration {
        if self.frame != frame {
            (self.frame, self.time) = (frame, Duration::ZERO);
            self.since = (self.waiting > 0).then_some(now);
        }
        let running = self.since.map_or(Duration::ZERO, |s| now.saturating_duration_since(s));
        let left = budget.saturating_sub(self.time + running);
        if !left.is_zero() {
            self.waiting += 1;
            self.since.get_or_insert(now);
        }
        left
    }

    fn end(&mut self, now: Instant) {
        self.waiting -= 1;
        if self.waiting == 0
            && let Some(since) = self.since.take()
        {
            self.time += now.saturating_duration_since(since);
        }
    }
}

/// Waits for `p` while frame `frame` has budget left. Dispatches have a
/// budget of their own, as large: what a dropped draw leaves out is drawn
/// in the next frame, but what a pass that runs once would have written is
/// lost, and draws (far more of them) came first and spent it all.
fn wait(p: &Pending, frame: u64) {
    let screen = p.screen && screens_first();
    let base = if screen { screen_budget() } else { wait_budget() };
    if base.is_zero() || p.slow() {
        return;
    }
    // What was waited past the base budget since the start, in
    // milliseconds, by kind (others, compute).
    static PAST: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
    let compute = p.computes();
    let past = &PAST[usize::from(compute)];
    let budget = frame_budget(base, compute, loading_allowance(), Duration::from_millis(past.load(Ordering::Relaxed)));
    // The frame the budget belongs to, and what was waited of it (or is
    // taken by a thread waiting now), by kind; of the passes over the screen
    // too.
    static FRAMES: [Mutex<FrameWait>; 4] = [const { Mutex::new(FrameWait::new()) }; 4];
    let kind = usize::from(compute) * 2 + usize::from(screen);
    let frame_wait = &FRAMES[kind];
    let sharing = shared_budget();
    let started = Instant::now();
    let left = frame_wait.lock().unwrap_or_else(|e| e.into_inner()).begin(sharing, frame, budget, started);
    if left.is_zero() {
        return;
    }
    let (lock, done) = &DONE;
    let mut held = lock.lock().unwrap_or_else(|e| e.into_inner());
    while p.made.get().is_none() && !p.slow() {
        let rest = left.saturating_sub(started.elapsed());
        if rest.is_zero() {
            break;
        }
        // Woken now and then to see whether it has become a slow one.
        let rest = rest.min(Duration::from_millis(250));
        held = done.wait_timeout(held, rest).unwrap_or_else(|e| e.into_inner()).0;
    }
    drop(held);
    let waited = started.elapsed().min(left);
    if screen && waited > Duration::from_millis(500) {
        tracing::info!(
            ms = waited.as_millis() as u64,
            made = p.made.get().is_some(),
            shaders = ?p.shaders,
            "a pass over the screen was waited for (VICTORY_SCREEN_WAIT_MS=0: dropped past half a second a frame)"
        );
    }
    past.fetch_add(waited.saturating_sub(base).as_millis() as u64, Ordering::Relaxed);
    frame_wait.lock().unwrap_or_else(|e| e.into_inner()).end(sharing, frame, left, waited, Instant::now());
    crate::framelog::add(crate::framelog::T::PipelineWaitUs, started.elapsed().as_micros() as u64);
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
    requested: VecDeque<Arc<Pending>>,
}

impl Queue {
    const fn new() -> Self {
        Self {
            screen: VecDeque::new(),
            early: VecDeque::new(),
            slow: VecDeque::new(),
            slow_now: 0,
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
            self.requested.push_back(p.clone());
        } else if slow {
            self.slow.push_back(p.clone());
        } else {
            self.early.push_back(p.clone());
        }
        Some(prev == NOT_QUEUED)
    }

    /// The passes over the screen first, then the most asked for of the
    /// requested ones (the first of equals), then the early ones in order,
    /// then one long one if there is room for it. In the order of the first
    /// request, one that thousands of draws a frame want waited behind
    /// hundreds wanted once.
    fn pop(&mut self, screens_only: bool, slow_limit: usize) -> Option<Arc<Pending>> {
        // An early entry can have been promoted and claimed already.
        let unclaimed = |list: &mut VecDeque<Arc<Pending>>| {
            std::iter::from_fn(|| list.pop_front()).find(|p| p.queue_state.load(Ordering::Relaxed) == EARLY)
        };
        let most_asked = |requested: &mut VecDeque<Arc<Pending>>| {
            let mut most: Option<(usize, u64)> = None;
            for (at, p) in requested.iter().enumerate() {
                let asked = p.asked.load(Ordering::Relaxed);
                if most.is_none_or(|(_, m)| asked > m) {
                    most = Some((at, asked));
                }
            }
            most.and_then(|(at, _)| requested.remove(at))
        };
        let mut p = self.screen.pop_front();
        if p.is_none() && !screens_only {
            p = most_asked(&mut self.requested).or_else(|| unclaimed(&mut self.early));
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

/// Workers at work until the title draws a world.
const MENU_WORKERS: usize = 2;
static WORLD: AtomicBool = AtomicBool::new(false);

/// At every present, with the draws run on the host so far: a frame of many
/// draws is the world's (menus run about ten).
pub(crate) fn frame_drew(drawn: u64) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if drawn.saturating_sub(LAST.swap(drawn, Ordering::Relaxed)) >= 60 {
        WORLD.store(true, Ordering::Relaxed);
    }
}

/// How many workers make pipelines: `asked` (`VICTORY_PIPELINE_WORKERS`, 1 to
/// 64), else two thirds of the logical processors, 2 to 16. They run below the
/// title's own threads, and a cold cache queues hundreds at once.
fn worker_count(asked: Option<usize>, processors: usize) -> usize {
    match asked {
        Some(n) => n.clamp(1, 64),
        None => (processors * 2 / 3).clamp(2, 16),
    }
}

/// Starts the workers ([`worker_count`]) on first use.
fn workers() -> bool {
    static RUNNING: OnceLock<bool> = OnceLock::new();
    *RUNNING.get_or_init(|| {
        let mut running = false;
        let workers = worker_count(
            std::env::var("VICTORY_PIPELINE_WORKERS")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            std::thread::available_parallelism().map_or(2, |n| n.get()),
        );
        tracing::info!(workers, "pipeline workers");
        let screens = if screens_first() { SCREEN_WORKERS } else { 0 };
        for i in 0..workers + screens {
            let screens_only = i >= workers;
            let spawned = std::thread::Builder::new()
                .name(format!("victory-pipelines-{i}"))
                .spawn(move || {
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
                        // All of them during the intro and the menus starve
                        // the title's threads: frames come unevenly.
                        while i >= MENU_WORKERS && !WORLD.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        let p = next(screens_only);
                        p.make();
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
            } else {
                running = true;
            }
        }
        running
    })
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
mod loading_tests {
    use super::{FrameWait, LOADING_FRAME, LOADING_FRAME_COMPUTE, Sharing, frame_budget, held_too_long, sharing_from};
    use std::time::{Duration, Instant};

    /// Two threads ask for pipelines still being made in one frame, the
    /// second 1 ms after the first: by default both may wait, inside one
    /// window of the frame's budget, and neither drops its pipeline
    /// because the other is waiting. The window bounds their waiting
    /// together: once some thread has waited its whole length, a thread
    /// that asks gets nothing. A new frame has a window of its own.
    #[test]
    fn two_threads_in_one_frame_both_wait_inside_the_window() {
        let sharing = sharing_from(None);
        let (budget, t0, ms) = (Duration::from_secs(12), Instant::now(), Duration::from_millis(1));
        let mut w = FrameWait::new();
        let a = w.begin(sharing, 7, budget, t0);
        let b = w.begin(sharing, 7, budget, t0 + ms);
        assert_eq!(a, budget);
        assert_eq!(b, budget - ms, "the second thread drops its pipeline at once: {sharing:?} gives it {b:?}");
        // Both made after 2 s: 2 s of the window gone.
        w.end(sharing, 7, a, Duration::from_secs(2), t0 + Duration::from_secs(2));
        w.end(sharing, 7, b, Duration::from_secs(2) - ms, t0 + Duration::from_secs(2));
        assert_eq!(w.begin(sharing, 7, budget, t0 + Duration::from_secs(5)), Duration::from_secs(10));
        // That thread waits out the rest: the window is used up.
        w.end(sharing, 7, Duration::from_secs(10), Duration::from_secs(10), t0 + Duration::from_secs(15));
        assert_eq!(w.begin(sharing, 7, budget, t0 + Duration::from_secs(16)), Duration::ZERO);
        // The next frame.
        assert_eq!(w.begin(sharing, 8, budget, t0 + Duration::from_secs(17)), budget);
    }

    /// `VICTORY_PIPELINE_WAIT_SHARED=reserve`, the default before the
    /// window: the second thread gets nothing while the first waits. `0`:
    /// each thread counts only waits that have ended.
    #[test]
    fn the_old_ways_of_counting_stay_behind_the_switch() {
        let (budget, t0) = (Duration::from_secs(12), Instant::now());
        let mut w = FrameWait::new();
        assert_eq!(w.begin(Sharing::Reserve, 1, budget, t0), budget);
        assert_eq!(w.begin(Sharing::Reserve, 1, budget, t0), Duration::ZERO);
        let mut w = FrameWait::new();
        assert_eq!(w.begin(Sharing::PerThread, 1, budget, t0), budget);
        assert_eq!(w.begin(Sharing::PerThread, 1, budget, t0), budget);
        w.end(Sharing::PerThread, 1, budget, Duration::from_secs(3), t0);
        assert_eq!(w.begin(Sharing::PerThread, 1, budget, t0), Duration::from_secs(9));
        assert_eq!(sharing_from(Some("0")), Sharing::PerThread);
        assert_eq!(sharing_from(Some("reserve")), Sharing::Reserve);
    }

    /// While a run's allowance lasts a frame may wait 4 s for a graphics
    /// pipeline and 12 s for a compute one, then only the frame budget; a
    /// budget set above them stays.
    #[test]
    fn a_frame_waits_longer_while_the_loading_allowance_lasts() {
        let (base, allowance) = (Duration::from_millis(500), Duration::from_secs(120));
        assert_eq!(frame_budget(base, false, allowance, Duration::ZERO), LOADING_FRAME);
        assert_eq!(frame_budget(base, true, allowance, Duration::ZERO), LOADING_FRAME_COMPUTE);
        // 1 s of the allowance left: 1 s.
        assert_eq!(frame_budget(base, true, allowance, Duration::from_secs(119)), Duration::from_secs(1));
        // Spent: the frame budget, as before.
        assert_eq!(frame_budget(base, true, allowance, Duration::from_secs(130)), base);
        assert_eq!(frame_budget(base, false, Duration::ZERO, Duration::ZERO), base, "VICTORY_PIPELINE_GATE_S=0");
        assert_eq!(frame_budget(Duration::from_secs(20), true, allowance, Duration::ZERO), Duration::from_secs(20));
    }

    /// No frame waits for a pipeline the driver has held past 10 s
    /// (compute) or 3 s (others), while the frame budget is shorter; a
    /// budget at or above that is the bound alone.
    #[test]
    fn a_pipeline_held_too_long_is_not_waited_for() {
        let base = Duration::from_millis(500);
        assert!(!held_too_long(Duration::from_secs(10), true, base));
        assert!(held_too_long(Duration::from_millis(10_001), true, base));
        assert!(!held_too_long(Duration::from_secs(3), false, base));
        assert!(held_too_long(Duration::from_millis(3_001), false, base));
        // VICTORY_PIPELINE_WAIT_MS=12000: waited for however long it is held.
        let long = Duration::from_secs(12);
        assert!(!held_too_long(Duration::from_secs(60), true, long));
        assert!(!held_too_long(Duration::from_secs(60), false, long));
        // 5 s: past it for compute pipelines only.
        let between = Duration::from_secs(5);
        assert!(held_too_long(Duration::from_secs(60), true, between));
        assert!(!held_too_long(Duration::from_secs(60), false, between));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_count_follows_the_processors_and_the_switch() {
        assert_eq!(worker_count(None, 1), 2);
        assert_eq!(worker_count(None, 24), 16);
        assert_eq!(worker_count(None, 64), 16);
        assert_eq!(worker_count(Some(0), 24), 1);
        assert_eq!(worker_count(Some(20), 24), 20);
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

<<<<<<< HEAD
    /// Threads waiting at the same time share the time they wait, none
    /// takes it from the others, and a new frame starts anew.
    #[test]
    fn waits_at_the_same_time_count_once() {
        let ms = Duration::from_millis;
        let (t, mut s) = (Instant::now(), Spent::new());
        assert_eq!(s.begin(1, ms(500), t), ms(500));
        assert_eq!(s.begin(1, ms(500), t + ms(100)), ms(400), "the second waits too");
        s.end(t + ms(150));
        s.end(t + ms(200));
        assert_eq!(s.begin(1, ms(500), t + ms(900)), ms(300), "200 ms were waited");
        s.end(t + ms(1200));
        assert_eq!(s.begin(1, ms(500), t + ms(1300)), ms(0));
        // A wait that reaches into the next frame counts there from its start.
        assert_eq!(s.begin(2, ms(500), t + ms(1400)), ms(500));
        assert_eq!(s.begin(3, ms(500), t + ms(1500)), ms(500));
        s.end(t + ms(1600));
        s.end(t + ms(1700));
        assert_eq!(s.begin(3, ms(500), t + ms(1800)), ms(300));
    }

    /// It is answered, waited for by no one, and the worker goes on.
    #[test]
    fn a_panic_while_making_is_a_pipeline_the_host_does_not_have() {
        let p = Arc::new(Pending::new(Vec::new(), None));
        p.make_with(|| panic!("a fault of the translator's"));
        assert!(matches!(p.made.get(), Some(None)));
        p.make_with(|| unreachable!("tried again"));
        let started = Instant::now();
        assert!(ready(&p).is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        let next = Arc::new(Pending::new(Vec::new(), None));
        next.make();
        assert!(matches!(next.made.get(), Some(None)));
    }

||||||| 9c184b35
=======
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

>>>>>>> fork/main
    #[test]
    fn pending_pipeline_is_recorded_for_the_next_launch() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "pending-pipeline-test".to_owned();
        // Simulate a worker that has taken the job but cannot finish it yet.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let p = Arc::new(p);
        // No frame budget on this thread (`VICTORY_PIPELINE_WAIT_MS=0`):
        // the request is not waited for, where the default would wait out
        // a 12 s window for a pipeline that is never made.
        super::TEST_BUDGET.set(Some(std::time::Duration::ZERO));

        assert!(!super::super::library::wanted_early(&p.key));
        assert!(ready(&p).is_none());
        assert!(p.made.get().is_none());
        assert!(super::super::library::wanted_early(&p.key));
    }

    #[test]
    fn the_most_wanted_pipeline_is_made_first() {
        let new = |asked: u64| {
            let p = Arc::new(Pending::new(Vec::new(), None));
            p.asked.store(asked, Ordering::Relaxed);
            p
        };
        let (once, often, also_once, unasked) = (new(1), new(900), new(1), new(0));
        let mut queue = Queue::new();
        for p in [&unasked, &once, &also_once, &often] {
            assert_eq!(queue.push(p, REQUESTED), Some(true));
        }
        // More draws ask while it waits.
        also_once.asked.fetch_add(5, Ordering::Relaxed);
        for p in [&often, &also_once, &once, &unasked] {
            assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), p));
        }
        assert!(queue.pop(false, 1).is_none());
    }

    #[test]
    fn only_pipelines_the_host_does_not_have_are_failures() {
        let (refused, waiting) = (
            Arc::new(Pending::new(Vec::new(), None)),
            Arc::new(Pending::new(Vec::new(), None)),
        );
        refused.make_with(|| None);
        assert_eq!(failures(&[refused, waiting]).len(), 1);
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
}
