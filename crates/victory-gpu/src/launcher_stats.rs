//! Publishes the live statistics block for the launcher (`victory-stats`, docs/launcher-contract.md).
//!
//! The frame path ([`Publisher::frame`]) is a few atomic stores: no lock, no allocation, no system
//! call. The slower values are written from the once-a-second branch of the frame meter
//! ([`Publisher::slow`]); that is the only place that asks the system anything. Off with
//! `VICTORY_STATS=0`; if the block cannot be made nothing happens and the engine goes on.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use victory_stats::{Slow, Writer, qpc_now};

/// Card memory in use and its budget, set where the engine already looks at them
/// ([`crate::host::Host::video_memory`]), so this module asks nothing new.
pub(crate) static VRAM: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

pub(crate) struct Publisher {
    writer: Writer,
    /// `SUBMISSION_US` as seen at the last frame: its change is this frame's CPU and GPU time.
    seen: [u64; 2],
}

impl Publisher {
    /// The block of this process, once: `None` when switched off, already made, or refused.
    pub(crate) fn new() -> Option<Publisher> {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        static TAKEN: AtomicBool = AtomicBool::new(false);
        if !*ON.get_or_init(|| std::env::var("VICTORY_STATS").as_deref() != Ok("0")) {
            return None;
        }
        if TAKEN.swap(true, Ordering::Relaxed) {
            return None;
        }
        let build = format!(
            "{} {}",
            option_env!("VICTORY_BUILD_ID").unwrap_or(env!("CARGO_PKG_VERSION")),
            if cfg!(feature = "diagnostics") { "diagnostics" } else { "release" }
        );
        let writer = Writer::create(std::process::id(), &build)?;
        tracing::info!("live statistics block published for the launcher");
        Some(Publisher { writer, seen: [0; 2] })
    }

    /// One presented frame. `submission_us` is the engine's running totals of CPU preparation and
    /// GPU wait (they are reset once in a while by the meter's lines, a smaller value is a restart).
    #[inline]
    pub(crate) fn frame(&mut self, submission_us: [u64; 2]) {
        let mut delta = |i: usize| {
            let (now, was) = (submission_us[i], self.seen[i]);
            self.seen[i] = now;
            (if now >= was { now - was } else { now }).min(u64::from(u32::MAX)) as u32
        };
        let (cpu, gpu) = (delta(0), delta(1));
        self.writer.frame(qpc_now(), cpu, gpu);
    }

    /// The values that change slowly, about once a second.
    pub(crate) fn slow(&mut self, draws: u64, dispatches: u64) {
        let w = &self.writer;
        w.set(Slow::VramUsed, VRAM[0].load(Ordering::Relaxed));
        w.set(Slow::VramBudget, VRAM[1].load(Ordering::Relaxed));
        w.set(Slow::RamResident, process_working_set());
        w.set(Slow::Draws, draws);
        w.set(Slow::Dispatches, dispatches);
        // Pipelines being built and the shader warm-up are not counted by the engine yet: 0.
        w.slow_done(qpc_now());
    }
}

/// Our working set in bytes (once a second).
fn process_working_set() -> u64 {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    // SAFETY: our own process and a sized structure.
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.WorkingSetSize as u64
}
