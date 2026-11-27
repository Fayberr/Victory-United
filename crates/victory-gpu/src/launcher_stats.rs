//! Live launcher statistics. The present path uses only QPC and atomic stores.
//! The existing meter publishes slower counters once per interval.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use victory_stats::{Metric, PresentTiming, Slow, UNKNOWN_US, Writer, qpc_now};

/// Updated by the existing DXGI video-memory query.
pub(crate) static VRAM: [AtomicU64; 2] = [const { AtomicU64::new(u64::MAX) }; 2];
/// Cumulative copies of the logged timers, which are reset by a different thread.
pub(crate) static SUBMISSION: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
pub(crate) static CARD_IDLE: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
pub(crate) static CARD_CLOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static PIPELINE_WAIT_US: AtomicU64 = AtomicU64::new(0);
pub(crate) static TEXTURE_UPLOAD_BYTES: AtomicU64 = AtomicU64::new(0);

const STAGES: &[&str] = &["Buffer refresh", "Texture refresh", "Readback"];

pub(crate) struct Publisher {
    writer: Writer,
    seen: [u64; 4],
    qpc_freq: u64,
    interrupted: bool,
}

impl Publisher {
    pub(crate) fn new() -> Option<Self> {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        static TAKEN: AtomicBool = AtomicBool::new(false);
        if !*ON.get_or_init(|| std::env::var("VICTORY_STATS").as_deref() != Ok("0"))
            || TAKEN.swap(true, Relaxed)
        {
            return None;
        }
        let build = format!(
            "{} {}",
            option_env!("VICTORY_BUILD_ID").unwrap_or(env!("CARGO_PKG_VERSION")),
            if cfg!(feature = "diagnostics") {
                "diagnostics"
            } else {
                "release"
            }
        );
        let writer = Writer::create_named(std::process::id(), &build, STAGES)?;
        let qpc_freq = writer.qpc_freq();
        tracing::info!("live statistics block published for the launcher");
        Some(Self {
            writer,
            seen: totals(),
            qpc_freq,
            interrupted: true,
        })
    }

    pub(crate) fn adapter(&self, luid: windows::Win32::Foundation::LUID) {
        let bits = (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart);
        self.writer.set_metric(Metric::AdapterLuid, Some(bits));
    }

    #[inline]
    pub(crate) fn frame(&mut self, (start, end): (u64, u64)) {
        let now = totals();
        let [cpu, wait, mut idle, mut idle_cpu] = std::array::from_fn(|i| {
            now[i]
                .saturating_sub(self.seen[i])
                .min(u64::from(UNKNOWN_US - 1)) as u32
        });
        self.seen = now;
        let interrupted = std::mem::take(&mut self.interrupted);
        if interrupted || !CARD_CLOCK_ACTIVE.load(Relaxed) {
            idle = UNKNOWN_US;
            idle_cpu = UNKNOWN_US;
        }
        let call_us = end.saturating_sub(start).saturating_mul(1_000_000) / self.qpc_freq;
        self.writer.frame_with_timing(
            end,
            if interrupted { UNKNOWN_US } else { cpu },
            if interrupted { UNKNOWN_US } else { wait },
            PresentTiming {
                call_us: call_us.min(u64::from(UNKNOWN_US - 1)) as u32,
                gpu_idle_us: idle,
                gpu_idle_cpu_us: idle_cpu,
            },
        );
    }

    #[inline]
    pub(crate) fn rejected(&mut self, status: windows::core::HRESULT) {
        use windows::Win32::Foundation::DXGI_STATUS_OCCLUDED;
        self.interrupted = true;
        self.writer
            .present_rejected(status.0, status == DXGI_STATUS_OCCLUDED);
    }

    pub(crate) fn slow(
        &self,
        frames: u32,
        preparing: [u64; 4],
        readback: [u64; 2],
        [drops, indirect, budget]: [u64; 3],
    ) {
        let w = &self.writer;
        let [used, budget_bytes] = VRAM.each_ref().map(|v| {
            let bytes = v.load(Relaxed);
            (bytes != u64::MAX).then_some(bytes)
        });
        w.set(Slow::VramUsed, used.unwrap_or_default());
        w.set(Slow::VramBudget, budget_bytes.unwrap_or_default());
        let ram = process_working_set();
        let draws = crate::draw::drawn();
        let dispatches = crate::draw::dispatched();
        w.set(Slow::RamResident, ram.unwrap_or_default());
        w.set(Slow::Draws, draws);
        w.set(Slow::Dispatches, dispatches);
        let issued = crate::list::totals();
        let in_place = crate::shadow::in_place_bytes();
        for (metric, value) in [
            (Metric::VramUsed, used),
            (Metric::VramBudget, budget_bytes),
            (Metric::RamResident, ram),
            (Metric::DrawsIssued, Some(issued[0])),
            (Metric::DispatchesIssued, Some(issued[1])),
            (Metric::DrawsExecuted, Some(draws)),
            (Metric::DispatchesExecuted, Some(dispatches)),
            (Metric::PipelineDrops, Some(drops)),
            (Metric::IndirectDrops, Some(indirect)),
            (Metric::IndirectBudgetDrops, Some(budget)),
            (Metric::PipelinesBuilding, Some(crate::pipeline::making())),
            (Metric::PipelineWaitUs, Some(PIPELINE_WAIT_US.load(Relaxed))),
            (
                Metric::TexturesWithoutRoom,
                Some(crate::resource::NOT_MADE_FOR_NOW.load(Relaxed)),
            ),
            (
                Metric::TextureLevelsWithoutTiles,
                Some(crate::tiles::STATS[9].load(Relaxed)),
            ),
            (
                Metric::TextureUploadBytes,
                Some(TEXTURE_UPLOAD_BYTES.load(Relaxed)),
            ),
            (Metric::WriteWatchQueries, Some(crate::watch::queries())),
            (Metric::InPlaceBytes, Some(in_place)),
            (Metric::InPlacePages, Some(in_place / 65_536)),
            (Metric::CopiedToGpu, Some(preparing[1])),
            (Metric::CopiedFromGpu, Some(readback[1])),
        ] {
            w.set_metric(metric, value);
        }
        w.set_stages(
            u64::from(frames),
            &[preparing[0], preparing[2], readback[0]],
        );
        w.slow_done(qpc_now());
    }
}

#[inline]
fn totals() -> [u64; 4] {
    [
        SUBMISSION[0].load(Relaxed),
        SUBMISSION[1].load(Relaxed),
        CARD_IDLE[0].load(Relaxed),
        CARD_IDLE[1].load(Relaxed),
    ]
}

fn process_working_set() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: our own process and a sized structure.
    (unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0)
        .then_some(c.WorkingSetSize as u64)
}
