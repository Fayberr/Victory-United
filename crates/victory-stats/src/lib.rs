//! Live statistics block: the engine writes it once per frame, the launcher reads it once per second.
//!
//! Contract (see docs/launcher-contract.md): a named shared-memory block called
//! `Local\VictoryUnited.Stats.<pid of the engine process>`, fixed layout, versioned header, little
//! endian. This crate is the single source of truth for both sides.
//!
//! # Layout (version 1, 8 KiB ring plus 512 bytes of header, 8704 bytes in all)
//!
//! | offset | field | meaning |
//! |---|---|---|
//! | 0 | `magic` | `"VUSTATS1"`, stored last when the block is created |
//! | 8 | `version` | layout version, 1 |
//! | 12 | `size` | size of the block in bytes |
//! | 16 | `qpc_freq` | QueryPerformanceCounter ticks per second |
//! | 24 | `pid` | engine process id |
//! | 32 | `build` | engine build string, 64 bytes, zero padded UTF-8 |
//! | 128 | `seq` | seqlock counter for the frame section, odd while a frame is being written |
//! | 136 | `frames` | frames presented so far |
//! | 144 | `last_qpc` | QPC of the latest presented frame |
//! | 152 | `slow_qpc` | QPC of the latest write of the slow values |
//! | 160.. | slow values | one atomic `u64` each, see [`Slow`] |
//! | 512 | `ring` | 512 entries of 16 bytes: `qpc: u64`, `cpu_us: u32`, `gpu_us: u32` |
//!
//! # Consistency
//!
//! The frame section (`frames`, `last_qpc`, the ring) has exactly one writer, the presenting thread,
//! and is protected by a seqlock: the writer makes `seq` odd, stores, makes it even again. The reader
//! copies the section and accepts it only if `seq` was even and unchanged, otherwise it retries a
//! few times and gives up with `None` (a torn read is never returned). Every field is an atomic, so
//! there is no data race in the Rust sense either. Slow values are independent atomics, each one is
//! always a whole value; they may be from slightly different moments, which is fine for a display.
//!
//! The frame time of frame `n` is `ring[n].qpc - ring[n-1].qpc` (present to present). `cpu_us` and
//! `gpu_us` are what the engine knows about that frame (0 when unknown).
//!
//! # Cost
//!
//! [`Writer::frame`] is a handful of relaxed and release stores: no lock, no allocation, no system
//! call (the caller passes the QPC value, [`qpc_now`] is user mode only).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

mod os;

pub use os::qpc_now;

pub const MAGIC: u64 = u64::from_le_bytes(*b"VUSTATS1");
pub const VERSION: u32 = 1;
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
pub struct Block {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSample {
    pub qpc: u64,
    pub cpu_us: u32,
    pub gpu_us: u32,
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
    assert!(std::mem::size_of::<Block>() == 512 + RING * 16);
    assert!(std::mem::offset_of!(Block, version) == 8);
    assert!(std::mem::offset_of!(Block, size) == 12);
    assert!(std::mem::offset_of!(Block, qpc_freq) == 16);
    assert!(std::mem::offset_of!(Block, pid) == 24);
    assert!(std::mem::offset_of!(Block, build) == 32);
    assert!(std::mem::offset_of!(Block, seq) == 128);
    assert!(std::mem::offset_of!(Block, frames) == 136);
    assert!(std::mem::offset_of!(Block, last_qpc) == 144);
    assert!(std::mem::offset_of!(Block, slow_qpc) == 152);
    assert!(std::mem::offset_of!(Block, slow) == 160);
    assert!(std::mem::offset_of!(Block, ring) == 512);
};

impl Block {
    /// Copies the block if it is valid and consistent. `None` for a wrong magic or version, or when
    /// the writer kept changing the frame section for every attempt.
    pub fn snapshot(&self) -> Option<Snapshot> {
        if self.magic.load(Ordering::Acquire) != MAGIC
            || self.version.load(Ordering::Relaxed) != VERSION
            || self.size.load(Ordering::Relaxed) as usize != SIZE
        {
            return None;
        }
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
                    }
                })
                .collect();
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) != s1 {
                continue;
            }
            let slow = |s: Slow| self.slow[s as usize].load(Ordering::Relaxed);
            let build: Vec<u8> = self.build.iter().map(|b| b.load(Ordering::Relaxed)).take_while(|&b| b != 0).collect();
            return Some(Snapshot {
                pid: self.pid.load(Ordering::Relaxed) as u32,
                build: String::from_utf8_lossy(&build).into_owned(),
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
            });
        }
        None
    }

    fn init(&self, pid: u32, build: &str) {
        self.qpc_freq.store(os::qpc_freq(), Ordering::Relaxed);
        self.pid.store(pid as u64, Ordering::Relaxed);
        for (slot, b) in self.build.iter().zip(build.bytes().take(BUILD_LEN - 1)) {
            slot.store(b, Ordering::Relaxed);
        }
        self.version.store(VERSION, Ordering::Relaxed);
        self.size.store(SIZE as u32, Ordering::Relaxed);
        // Last, so a reader that sees the magic sees everything above.
        self.magic.store(MAGIC, Ordering::Release);
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
        let (owner, block) = os::create(&name(pid))?;
        // SAFETY: `block` points to `SIZE` zeroed bytes aligned to 64 that live as long as `owner`.
        unsafe { &*block }.init(pid, build);
        Some(Writer { block, _owner: owner, frames: 0, seq: 0 })
    }

    /// A block on the heap, for tests and benchmarks.
    pub fn in_memory(pid: u32, build: &str) -> Writer {
        let (owner, block) = os::heap();
        unsafe { &*block }.init(pid, build);
        Writer { block, _owner: owner, frames: 0, seq: 0 }
    }

    fn block(&self) -> &Block {
        // SAFETY: see `create`.
        unsafe { &*self.block }
    }

    /// A reader on this writer's own block (tests).
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.block().snapshot()
    }

    /// Records one presented frame. `qpc` is [`qpc_now`] taken at the present call.
    #[inline]
    pub fn frame(&mut self, qpc: u64, cpu_us: u32, gpu_us: u32) {
        // SAFETY: see `create`; a separate borrow from `self` so the counters can change.
        let b = unsafe { &*self.block };
        self.seq += 1;
        b.seq.store(self.seq, Ordering::Relaxed); // odd: writing
        fence(Ordering::Release);
        let e = &b.ring[self.frames as usize % RING];
        e.qpc.store(qpc, Ordering::Relaxed);
        e.cpu_us.store(cpu_us, Ordering::Relaxed);
        e.gpu_us.store(gpu_us, Ordering::Relaxed);
        self.frames += 1;
        b.frames.store(self.frames, Ordering::Relaxed);
        b.last_qpc.store(qpc, Ordering::Relaxed);
        self.seq += 1;
        b.seq.store(self.seq, Ordering::Release); // even: done
    }

    /// Writes one slow value. Any thread that holds the writer behind a reference may call it.
    pub fn set(&self, what: Slow, value: u64) {
        self.block().slow[what as usize].store(value, Ordering::Relaxed);
    }

    /// Marks the moment the slow values were last written.
    pub fn slow_done(&self, qpc: u64) {
        self.block().slow_qpc.store(qpc, Ordering::Release);
    }
}

/// Launcher side: maps the block of an engine process read-only.
pub struct Reader {
    block: *const Block,
    _owner: os::Owner,
}
unsafe impl Send for Reader {}

impl Reader {
    /// `None` when the process publishes no block (older engine, `VICTORY_STATS=0`, not started).
    pub fn open(pid: u32) -> Option<Reader> {
        let (owner, block) = os::open(&name(pid))?;
        Some(Reader { block, _owner: owner })
    }

    /// A consistent copy, or `None` when the block is not valid (yet).
    pub fn snapshot(&self) -> Option<Snapshot> {
        // SAFETY: the mapping is `SIZE` bytes and lives as long as `_owner`.
        unsafe { &*self.block }.snapshot()
    }
}

/// The block name for an engine process.
pub fn name(pid: u32) -> String {
    format!("Local\\VictoryUnited.Stats.{pid}")
}

#[cfg(test)]
mod tests;
