use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

#[test]
fn layout_is_stable() {
    assert_eq!(V1_SIZE, 8704);
    assert_eq!(SIZE, 17472);
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
    assert_eq!(
        (s.vram_used, s.vram_budget, s.draws, s.slow_qpc),
        (5 << 30, 12 << 30, 99, 77)
    );
    assert_eq!(s.frames_recent.len(), 10);
    assert_eq!(
        s.frames_recent[3],
        FrameSample {
            qpc: 1048,
            cpu_us: 3,
            gpu_us: 6,
            timing: PresentTiming::default()
        }
    );
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
    assert!(unsafe { snapshot_view(p.cast(), SIZE, 1) }.is_none());
    // A wrong version is rejected.
    let w = Writer::in_memory(1, "x");
    w.block().legacy.version.store(3, Ordering::Relaxed);
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
        let b = block as *const u8;
        let (mut ok, mut none) = (0u32, 0u32);
        while !s2.load(Ordering::Relaxed) {
            match unsafe { snapshot_view(b, SIZE, 1) } {
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
        w.frame_with_timing(
            std::hint::black_box(i),
            1,
            2,
            PresentTiming {
                call_us: 3,
                gpu_idle_us: 4,
                gpu_idle_cpu_us: 5,
            },
        );
    }
    std::hint::black_box(w.snapshot());
    let per = t.elapsed().as_nanos() as f64 / n as f64;
    eprintln!("Writer::frame: {per:.1} ns per frame");
    // Generous bound so a loaded CI machine does not flake; the target is under 1000 ns.
    assert!(per < 1000.0, "{per} ns");
}

#[test]
fn version_one_uses_only_the_legacy_prefix() {
    let mut w = Writer::in_memory(42, "legacy");
    w.frame(1234, 10, 12);
    assert!(unsafe { snapshot_view(w.block.cast(), V1_SIZE, 42) }.is_none());
    w.block().legacy.version.store(1, Ordering::Relaxed);
    w.block()
        .legacy
        .size
        .store(V1_SIZE as u32, Ordering::Relaxed);
    assert!(unsafe { snapshot_view(w.block.cast(), V1_SIZE, 43) }.is_none());
    let snapshot = unsafe { snapshot_view(w.block.cast(), V1_SIZE, 42) }.unwrap();
    assert_eq!(snapshot.frames_recent[0].qpc, 1234);
    assert_eq!(snapshot.frames_recent[0].timing, PresentTiming::default());
    assert_eq!(snapshot.present_state, None);
    assert_eq!(snapshot.interval, None);
}

#[test]
fn occlusion_counts_without_a_frame_and_clears_on_success() {
    let mut w = Writer::in_memory_named(42, "modern", &["Buffer refresh"]);
    assert_eq!(
        w.snapshot().unwrap().present_state,
        Some(PresentState::Waiting)
    );
    w.present_rejected(123, true);
    w.present_rejected(123, true);
    let snapshot = w.snapshot().unwrap();
    assert_eq!(snapshot.present_state, Some(PresentState::Occluded));
    assert_eq!(snapshot.occluded, Some(2));
    assert_eq!(snapshot.frames, 0);
    w.present_rejected(-12, false);
    assert_eq!(
        w.snapshot().unwrap().present_state,
        Some(PresentState::Other(-12))
    );
    let timing = PresentTiming {
        call_us: 20,
        gpu_idle_us: 0,
        gpu_idle_cpu_us: 0,
    };
    w.frame_with_timing(1200, 30, 40, timing);
    w.set_metric(Metric::DrawsIssued, Some(0));
    w.set_stages(1, &[25]);
    let snapshot = w.snapshot().unwrap();
    assert_eq!(snapshot.present_state, Some(PresentState::Drawing));
    assert_eq!(snapshot.rejected, Some(1));
    assert_eq!(snapshot.frames, 1);
    assert_eq!(snapshot.frames_recent[0].timing, timing);
    let interval = snapshot.interval.unwrap();
    assert_eq!(interval.metric(Metric::DrawsIssued), Some(0));
    assert_eq!(interval.metric(Metric::PipelineDrops), None);
    assert_eq!(interval.stage_us, vec![("Buffer refresh".into(), 25)]);
}
