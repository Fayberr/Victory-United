use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

#[test]
fn layout_is_stable() {
    assert_eq!(SIZE, 8704);
    assert_eq!(std::mem::align_of::<Block>(), 64);
    assert_eq!(MAGIC.to_le_bytes(), *b"VUSTATS1");
    assert_eq!(name(42), "Local\\VictoryUnited.Stats.42");
}

#[test]
fn round_trip() {
    let mut w = Writer::in_memory(1234, "abcd1234-release");
    w.set(Slow::VramUsed, 5 << 30);
    w.set(Slow::VramBudget, 12 << 30);
    w.set(Slow::Draws, 99);
    w.slow_done(77);
    for i in 0..10u64 {
        w.frame(1000 + i * 16, i as u32, 2 * i as u32);
    }
    let s = w.snapshot().unwrap();
    assert_eq!((s.pid, s.build.as_str()), (1234, "abcd1234-release"));
    assert_eq!(s.frames, 10);
    assert_eq!(s.last_qpc, 1000 + 9 * 16);
    assert_eq!((s.vram_used, s.vram_budget, s.draws, s.slow_qpc), (5 << 30, 12 << 30, 99, 77));
    assert_eq!(s.frames_recent.len(), 10);
    assert_eq!(s.frames_recent[3], FrameSample { qpc: 1048, cpu_us: 3, gpu_us: 6 });
    assert_eq!(s.frame_times_ms().len(), 9);
}

#[test]
fn ring_wraps_and_keeps_order() {
    let mut w = Writer::in_memory(1, "");
    for i in 0..(RING as u64 * 2 + 5) {
        w.frame(i, 0, 0);
    }
    let s = w.snapshot().unwrap();
    assert_eq!(s.frames_recent.len(), RING);
    assert!(s.frames_recent.windows(2).all(|p| p[1].qpc == p[0].qpc + 1));
    assert_eq!(s.frames_recent.last().unwrap().qpc, RING as u64 * 2 + 4);
}

#[test]
fn missing_or_foreign_block_is_none() {
    assert!(Reader::open(u32::MAX - 3).is_none());
    // A zeroed block (magic not stored yet) is not valid.
    let (_owner, p) = os::heap();
    assert!(unsafe { &*p }.snapshot().is_none());
    // A wrong version is rejected.
    let w = Writer::in_memory(1, "x");
    w.block().version.store(2, Ordering::Relaxed);
    assert!(w.snapshot().is_none());
}

#[test]
fn a_torn_read_is_never_valid() {
    // The writer keeps every field of one frame equal to the frame number; any mix is a torn read.
    let mut w = Writer::in_memory(1, "t");
    let stop = Arc::new(AtomicBool::new(false));
    let block = w.block as usize;
    let s2 = stop.clone();
    let seen = Arc::new(AtomicU64::new(0));
    let seen2 = seen.clone();
    let reader = std::thread::spawn(move || {
        let b = unsafe { &*(block as *const Block) };
        let (mut ok, mut none) = (0u32, 0u32);
        while !s2.load(Ordering::Relaxed) {
            match b.snapshot() {
                Some(s) => {
                    ok += 1;
                    seen2.store(ok as u64, Ordering::Relaxed);
                    for (i, f) in s.frames_recent.iter().enumerate() {
                        let n = s.frames - s.frames_recent.len() as u64 + i as u64;
                        assert_eq!(f.qpc, n + 1, "torn sample");
                        assert_eq!(f.cpu_us as u64, (n + 1) & 0xffff_ffff);
                        assert_eq!(f.gpu_us as u64, (n + 1) & 0xffff_ffff);
                    }
                    assert_eq!(s.last_qpc, s.frames);
                }
                None => none += 1,
            }
        }
        (ok, none)
    });
    // Keep writing until the reader has validated plenty of snapshots (or 5 s passed).
    let t = std::time::Instant::now();
    let mut n = 0u64;
    while (seen.load(Ordering::Relaxed) < 2000 || n < 300_000) && t.elapsed().as_secs() < 5 {
        n += 1;
        w.frame(n, n as u32, n as u32);
    }
    stop.store(true, Ordering::Relaxed);
    let (ok, _none) = reader.join().unwrap();
    assert!(ok > 0, "reader never got a snapshot");
}

#[test]
fn frame_cost_is_small() {
    let mut w = Writer::in_memory(1, "");
    let t = std::time::Instant::now();
    let n = 2_000_000u64;
    for i in 0..n {
        w.frame(std::hint::black_box(i), 1, 2);
    }
    std::hint::black_box(w.snapshot());
    let per = t.elapsed().as_nanos() as f64 / n as f64;
    eprintln!("Writer::frame: {per:.1} ns per frame");
    // Generous bound so a loaded CI machine does not flake; the target is under 1000 ns.
    assert!(per < 1000.0, "{per} ns");
}
