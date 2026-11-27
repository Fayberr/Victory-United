//! Live statistics block: the engine writes it once per frame, the launcher reads it once per second.
//!
//! Contract (see docs/launcher-contract.md): a named shared-memory block called
//! `Local\VictoryUnited.Stats.<pid of the engine process>`, fixed layout, versioned header, little
//! endian. This crate is the single source of truth for both sides.
//!
//! # Layout
//!
//! Version 1 is 8704 bytes: a 512-byte header and 512 16-byte frame records. Version 2 keeps
//! that complete prefix and appends a parallel frame ring, present status, optional counters
//! and named stages. The magic stays `VUSTATS1`: check version and size before reading a tail.
//! Existing header offsets are asserted below. A zero in the v1 prefix can mean unknown.
//! Optional v2 fields use the maximum unsigned value for unavailable, so zero remains a
//! measurement. Stage names are immutable, UTF-8 and zero padded. Stage values are elapsed
//! microseconds over the last meter interval, normalized per frame by the reader.
//!
//! The presenting thread writes both rings and present status under one seqlock. A reader
//! accepts a copy only if the sequence was even and unchanged. Slow fields are independent
//! atomics, published about once a second. The frame path allocates nothing and makes no syscalls.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

mod os;

pub use os::qpc_now;

pub const MAGIC: u64 = u64::from_le_bytes(*b"VUSTATS1");
pub const VERSION: u32 = 2;
pub const V1_SIZE: usize = std::mem::size_of::<LegacyBlock>();
pub const STAGES: usize = 8;
const STAGE_NAME_LEN: usize = 32;
pub const UNKNOWN_US: u32 = u32::MAX;
const UNKNOWN: u64 = u64::MAX;
pub const RING: usize = 512;
pub const BUILD_LEN: usize = 64;
/// Total size of the block in bytes.
pub const SIZE: usize = std::mem::size_of::<Block>();

/// One slow value, written about once a second by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Slow {
    VramUsed,
    VramBudget,
    RamResident,
    Draws,
    Dispatches,
    PipelinesBuilding,
    WarmupDone,
    WarmupTotal,
}
const SLOW: usize = 8;

#[repr(C)]
struct Entry {
    qpc: AtomicU64,
    cpu_us: AtomicU32,
    gpu_us: AtomicU32,
}

/// The shared block. Never construct it by hand; use [`Writer`].
#[repr(C, align(64))]
struct LegacyBlock {
    magic: AtomicU64,
    version: AtomicU32,
    size: AtomicU32,
    qpc_freq: AtomicU64,
    pid: AtomicU64,
    build: [std::sync::atomic::AtomicU8; BUILD_LEN],
    _pad0: [u8; 128 - 32 - BUILD_LEN],
    seq: AtomicU64,
    frames: AtomicU64,
    last_qpc: AtomicU64,
    slow_qpc: AtomicU64,
    slow: [AtomicU64; SLOW],
    _pad1: [u8; 512 - 160 - SLOW * 8],
    ring: [Entry; RING],
}

/// Per-present measurements in the v2 ring. Unknown values are `UNKNOWN_US`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentTiming {
    pub call_us: u32,
    pub gpu_idle_us: u32,
    pub gpu_idle_cpu_us: u32,
}

impl Default for PresentTiming {
    fn default() -> Self {
        Self {
            call_us: UNKNOWN_US,
            gpu_idle_us: UNKNOWN_US,
            gpu_idle_cpu_us: UNKNOWN_US,
        }
    }
}

#[repr(C)]
struct PresentEntry {
    call_us: AtomicU32,
    gpu_idle_us: AtomicU32,
    gpu_idle_cpu_us: AtomicU32,
    _reserved: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentState {
    Waiting,
    Drawing,
    Occluded,
    Other(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Metric {
    CopiedToGpu,
    CopiedFromGpu,
    InPlaceBytes,
    InPlacePages,
    DrawsIssued,
    DispatchesIssued,
    PipelineDrops,
    IndirectDrops,
    PipelineWaitUs,
    TexturesWithoutRoom,
    TextureUploadBytes,
    WriteWatchQueries,
    PipelinesBuilding,
    VramUsed,
    VramBudget,
    RamResident,
    DrawsExecuted,
    DispatchesExecuted,
    AdapterLuid,
}

const METRICS: usize = Metric::AdapterLuid as usize + 1;

#[repr(C, align(64))]
pub struct Block {
    legacy: LegacyBlock,
    timing: [PresentEntry; RING],
    // Low word is HRESULT; the high word is a PresentState discriminator.
    present_state: AtomicU64,
    occluded: AtomicU64,
    rejected: AtomicU64,
    stages_frames: AtomicU64,
    metrics: [AtomicU64; METRICS],
    stage_us: [AtomicU64; STAGES],
    stage_names: [[std::sync::atomic::AtomicU8; STAGE_NAME_LEN]; STAGES],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interval {
    pub stage_us: Vec<(String, u64)>,
    pub frames: u64,
    metrics: [u64; METRICS],
}

impl Interval {
    pub fn metric(&self, which: Metric) -> Option<u64> {
        let value = self.metrics[which as usize];
        (value != UNKNOWN).then_some(value)
    }
}

/// What the reader gets: a consistent copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub pid: u32,
    pub build: String,
    pub qpc_freq: u64,
    /// Frames presented so far.
    pub frames: u64,
    pub last_qpc: u64,
    pub slow_qpc: u64,
    pub vram_used: u64,
    pub vram_budget: u64,
    pub ram_resident: u64,
    pub draws: u64,
    pub dispatches: u64,
    pub pipelines_building: u64,
    pub warmup_done: u64,
    pub warmup_total: u64,
    /// The latest frames, oldest first (at most [`RING`]).
    pub frames_recent: Vec<FrameSample>,
    pub present_state: Option<PresentState>,
    pub occluded: Option<u64>,
    pub rejected: Option<u64>,
    pub interval: Option<Interval>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSample {
    pub qpc: u64,
    pub cpu_us: u32,
    pub gpu_us: u32,
    pub timing: PresentTiming,
}

impl Snapshot {
    /// Present-to-present frame times in milliseconds, oldest first (one fewer than the samples).
    pub fn frame_times_ms(&self) -> Vec<f32> {
        let f = self.qpc_freq.max(1) as f64;
        self.frames_recent
            .windows(2)
            .map(|w| (w[1].qpc.saturating_sub(w[0].qpc) as f64 * 1000.0 / f) as f32)
            .collect()
    }
}

const _: () = {
    assert!(V1_SIZE == 512 + RING * 16);
    assert!(std::mem::offset_of!(Block, timing) == V1_SIZE);
    assert!(std::mem::offset_of!(LegacyBlock, version) == 8);
    assert!(std::mem::offset_of!(LegacyBlock, size) == 12);
    assert!(std::mem::offset_of!(LegacyBlock, qpc_freq) == 16);
    assert!(std::mem::offset_of!(LegacyBlock, pid) == 24);
    assert!(std::mem::offset_of!(LegacyBlock, build) == 32);
    assert!(std::mem::offset_of!(LegacyBlock, seq) == 128);
    assert!(std::mem::offset_of!(LegacyBlock, frames) == 136);
    assert!(std::mem::offset_of!(LegacyBlock, last_qpc) == 144);
    assert!(std::mem::offset_of!(LegacyBlock, slow_qpc) == 152);
    assert!(std::mem::offset_of!(LegacyBlock, slow) == 160);
    assert!(std::mem::offset_of!(LegacyBlock, ring) == 512);
};

impl LegacyBlock {
    fn snapshot(&self, v2: Option<&Block>) -> Option<Snapshot> {
        for _ in 0..8 {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let frames = self.frames.load(Ordering::Relaxed);
            let last_qpc = self.last_qpc.load(Ordering::Relaxed);
            let n = (frames as usize).min(RING);
            let first = frames as usize - n;
            let samples: Vec<FrameSample> = (first..frames as usize)
                .map(|i| {
                    let e = &self.ring[i % RING];
                    FrameSample {
                        qpc: e.qpc.load(Ordering::Relaxed),
                        cpu_us: e.cpu_us.load(Ordering::Relaxed),
                        gpu_us: e.gpu_us.load(Ordering::Relaxed),
                        timing: v2.map_or_else(PresentTiming::default, |b| {
                            let e = &b.timing[i % RING];
                            PresentTiming {
                                call_us: e.call_us.load(Ordering::Relaxed),
                                gpu_idle_us: e.gpu_idle_us.load(Ordering::Relaxed),
                                gpu_idle_cpu_us: e.gpu_idle_cpu_us.load(Ordering::Relaxed),
                            }
                        }),
                    }
                })
                .collect();
            let status = v2.map(|b| {
                let state = b.present_state.load(Ordering::Relaxed);
                let state = match state >> 32 {
                    0 => PresentState::Waiting,
                    1 => PresentState::Drawing,
                    2 => PresentState::Occluded,
                    _ => PresentState::Other(state as i32),
                };
                (
                    state,
                    b.occluded.load(Ordering::Relaxed),
                    b.rejected.load(Ordering::Relaxed),
                )
            });
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) != s1 {
                continue;
            }
            let slow = |s: Slow| self.slow[s as usize].load(Ordering::Relaxed);

            return Some(Snapshot {
                pid: self.pid.load(Ordering::Relaxed) as u32,
                build: atomic_text(&self.build),
                qpc_freq: self.qpc_freq.load(Ordering::Relaxed),
                frames,
                last_qpc,
                slow_qpc: self.slow_qpc.load(Ordering::Relaxed),
                vram_used: slow(Slow::VramUsed),
                vram_budget: slow(Slow::VramBudget),
                ram_resident: slow(Slow::RamResident),
                draws: slow(Slow::Draws),
                dispatches: slow(Slow::Dispatches),
                pipelines_building: slow(Slow::PipelinesBuilding),
                warmup_done: slow(Slow::WarmupDone),
                warmup_total: slow(Slow::WarmupTotal),
                frames_recent: samples,
                present_state: status.map(|s| s.0),
                occluded: status.map(|s| s.1),
                rejected: status.map(|s| s.2),
                interval: v2.map(Block::interval),
            });
        }
        None
    }
}

fn atomic_text(bytes: &[std::sync::atomic::AtomicU8]) -> String {
    let bytes: Vec<_> = bytes
        .iter()
        .map(|b| b.load(Ordering::Relaxed))
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Block {
    fn interval(&self) -> Interval {
        Interval {
            frames: self.stages_frames.load(Ordering::Relaxed),
            metrics: self.metrics.each_ref().map(|m| m.load(Ordering::Relaxed)),
            stage_us: self
                .stage_names
                .iter()
                .zip(&self.stage_us)
                .filter_map(|(name, value)| {
                    let value = value.load(Ordering::Relaxed);
                    (value != UNKNOWN).then(|| (atomic_text(name), value))
                })
                .collect(),
        }
    }

    fn init(&self, pid: u32, build: &str, stages: &[&str]) {
        for metric in self.metrics.iter().chain(&self.stage_us) {
            metric.store(UNKNOWN, Ordering::Relaxed);
        }
        for (name, value) in self.stage_names.iter().zip(stages) {
            for (slot, b) in name.iter().zip(value.bytes().take(STAGE_NAME_LEN - 1)) {
                slot.store(b, Ordering::Relaxed);
            }
        }
        let b = &self.legacy;
        b.qpc_freq.store(os::qpc_freq(), Ordering::Relaxed);
        b.pid.store(pid as u64, Ordering::Relaxed);
        for (slot, b) in b.build.iter().zip(build.bytes().take(BUILD_LEN - 1)) {
            slot.store(b, Ordering::Relaxed);
        }
        b.version.store(VERSION, Ordering::Relaxed);
        b.size.store(SIZE as u32, Ordering::Relaxed);
        // Last, so a reader that sees the magic sees everything above.
        b.magic.store(MAGIC, Ordering::Release);
    }
}

/// Engine side. Owns the mapping (or, for tests, a heap block).
pub struct Writer {
    block: *const Block,
    _owner: os::Owner,
    /// Mirror of the frame counter, so the frame path needs no load from shared memory.
    frames: u64,
    seq: u64,
}

// The block is only touched through atomics and the pointer stays valid while `_owner` lives.
unsafe impl Send for Writer {}

impl Writer {
    /// Creates `Local\VictoryUnited.Stats.<pid>`. `None` when the system refuses: the engine goes on.
    pub fn create(pid: u32, build: &str) -> Option<Writer> {
        Self::create_named(pid, build, &[])
    }

    pub fn create_named(pid: u32, build: &str, stages: &[&str]) -> Option<Writer> {
        let (owner, block) = os::create(&name(pid))?;
        // SAFETY: `block` points to `SIZE` zeroed bytes aligned to 64 that live as long as `owner`.
        unsafe { &*block }.init(pid, build, stages);
        Some(Writer {
            block,
            _owner: owner,
            frames: 0,
            seq: 0,
        })
    }

    /// A block on the heap, for tests and benchmarks.
    pub fn in_memory(pid: u32, build: &str) -> Writer {
        Self::in_memory_named(pid, build, &[])
    }

    pub fn in_memory_named(pid: u32, build: &str, stages: &[&str]) -> Writer {
        let (owner, block) = os::heap();
        unsafe { &*block }.init(pid, build, stages);
        Writer {
            block,
            _owner: owner,
            frames: 0,
            seq: 0,
        }
    }

    fn block(&self) -> &Block {
        // SAFETY: see `create`.
        unsafe { &*self.block }
    }

    /// A reader on this writer's own block (tests).
    pub fn snapshot(&self) -> Option<Snapshot> {
        unsafe {
            snapshot_view(
                self.block.cast(),
                SIZE,
                self.block().legacy.pid.load(Ordering::Relaxed) as u32,
            )
        }
    }

    /// Records one presented frame. `qpc` is [`qpc_now`] taken at the present call.
    #[inline]
    pub fn frame(&mut self, qpc: u64, cpu_us: u32, gpu_us: u32) {
        self.frame_with_timing(qpc, cpu_us, gpu_us, PresentTiming::default());
    }

    #[inline]
    pub fn frame_with_timing(&mut self, qpc: u64, cpu_us: u32, gpu_us: u32, timing: PresentTiming) {
        // SAFETY: see `create`; a separate borrow from `self` so the counters can change.
        let b = unsafe { &*self.block };
        self.begin(&b.legacy);
        let index = self.frames as usize % RING;
        let e = &b.legacy.ring[index];
        let t = &b.timing[index];
        t.call_us.store(timing.call_us, Ordering::Relaxed);
        t.gpu_idle_us.store(timing.gpu_idle_us, Ordering::Relaxed);
        t.gpu_idle_cpu_us
            .store(timing.gpu_idle_cpu_us, Ordering::Relaxed);
        b.present_state.store(1 << 32, Ordering::Relaxed);
        e.qpc.store(qpc, Ordering::Relaxed);
        e.cpu_us.store(cpu_us, Ordering::Relaxed);
        e.gpu_us.store(gpu_us, Ordering::Relaxed);
        self.frames += 1;
        b.legacy.frames.store(self.frames, Ordering::Relaxed);
        b.legacy.last_qpc.store(qpc, Ordering::Relaxed);
        self.end(&b.legacy);
    }

    /// Records a failed or occluded Present without advancing the frame ring.
    #[inline]
    pub fn present_rejected(&mut self, status: i32, occluded: bool) {
        let b = unsafe { &*self.block };
        self.begin(&b.legacy);
        let (kind, count) = if occluded {
            (2, &b.occluded)
        } else {
            (3, &b.rejected)
        };
        count.store(
            count.load(Ordering::Relaxed).saturating_add(1),
            Ordering::Relaxed,
        );
        b.present_state
            .store((kind << 32) | u64::from(status as u32), Ordering::Relaxed);
        self.end(&b.legacy);
    }

    #[inline]
    fn begin(&mut self, b: &LegacyBlock) {
        self.seq += 1;
        b.seq.store(self.seq, Ordering::Relaxed);
        fence(Ordering::Release);
    }

    #[inline]
    fn end(&mut self, b: &LegacyBlock) {
        self.seq += 1;
        b.seq.store(self.seq, Ordering::Release);
    }

    pub fn set_metric(&self, metric: Metric, value: Option<u64>) {
        self.block().metrics[metric as usize].store(value.unwrap_or(UNKNOWN), Ordering::Relaxed);
    }

    pub fn set_stages(&self, frames: u64, totals_us: &[u64]) {
        let b = self.block();
        for (slot, &value) in b.stage_us.iter().zip(totals_us) {
            slot.store(value, Ordering::Relaxed);
        }
        b.stages_frames.store(frames, Ordering::Relaxed);
    }

    /// Writes one slow value. Any thread that holds the writer behind a reference may call it.
    pub fn set(&self, what: Slow, value: u64) {
        self.block().legacy.slow[what as usize].store(value, Ordering::Relaxed);
    }

    /// Marks the moment the slow values were last written.
    pub fn slow_done(&self, qpc: u64) {
        self.block().legacy.slow_qpc.store(qpc, Ordering::Release);
    }
}

/// Launcher side: maps the block of an engine process read-only.
pub struct Reader {
    view: *const u8,
    len: usize,
    pid: u32,
    _owner: os::Owner,
}
unsafe impl Send for Reader {}

impl Reader {
    /// `None` when the process publishes no block (older engine, `VICTORY_STATS=0`, not started).
    pub fn open(pid: u32) -> Option<Reader> {
        let (owner, view, len) = os::open(&name(pid))?;
        Some(Reader {
            view,
            len,
            pid,
            _owner: owner,
        })
    }

    /// A consistent copy, or `None` when the block is not valid (yet).
    pub fn snapshot(&self) -> Option<Snapshot> {
        // SAFETY: the read-only mapped view is valid for `len` bytes and lives with `_owner`.
        unsafe { snapshot_view(self.view, self.len, self.pid) }
    }
}

/// The aligned view must be readable for `len` bytes. Validate the header before borrowing the
/// version-specific tail, since a v1 mapping can be shorter than `Block`.
unsafe fn snapshot_view(view: *const u8, len: usize, pid: u32) -> Option<Snapshot> {
    if len < V1_SIZE {
        return None;
    }
    let legacy = unsafe { &*view.cast::<LegacyBlock>() };
    if legacy.magic.load(Ordering::Acquire) != MAGIC
        || legacy.pid.load(Ordering::Relaxed) != u64::from(pid)
    {
        return None;
    }
    let declared = legacy.size.load(Ordering::Relaxed) as usize;
    let version = legacy.version.load(Ordering::Relaxed);
    match (version, declared) {
        (1, V1_SIZE) => legacy.snapshot(None),
        (VERSION, SIZE) if len >= SIZE => legacy.snapshot(Some(unsafe { &*view.cast::<Block>() })),
        _ => None,
    }
}

/// The block name for an engine process.
pub fn name(pid: u32) -> String {
    format!("Local\\VictoryUnited.Stats.{pid}")
}

#[cfg(test)]
mod tests;
