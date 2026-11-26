<<<<<<< HEAD
//! `victory warm`: host pipelines made ahead of a run (`known_pipelines`), in
//! processes of their own: a compiler that crashes or hangs costs one pipeline.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use victory_hle::known_pipelines as known;

/// Pipelines a process makes before another takes its place: the compiler's
/// and the translations' memory goes with it.
const AT_ONCE: usize = 40;
/// A pipeline that takes longer is taken for one the compiler hangs on.
const LONGEST: Duration = Duration::from_secs(180);

/// As the runs count one that takes long.
const SLOW_MS: f64 = 1000.0;

fn folder() -> Result<PathBuf> {
    let cfg = victory_core::Config::load()?;
    Ok(cfg.cache_subdir("shaders")?.with_file_name("pipelines"))
}

pub fn run(jobs: Option<usize>, limit: Option<usize>, again: bool, used: bool, these: Option<String>) -> Result<()> {
    let dir = folder()?;
    if let Some(these) = these {
        return make(&dir, &these);
    }
    let began = Instant::now();
    victory_hle::known_pipelines::use_translations(dir.with_file_name("shaders"));
    let fingerprint = known::fingerprint();
    let done_file = dir.join("warm-done.txt");
    let bad_file = dir.join("warm-bad.txt");
    let keys = known::keys(&dir);
    let listed = std::fs::read_to_string(&done_file).unwrap_or_default();
    let mut done: HashSet<&str> = HashSet::new();
    for line in listed.lines() {
        if let Some((of, key)) = line.split_once(' ')
            && of == fingerprint
        {
            done.insert(key);
        }
    }
    let bad = std::fs::read_to_string(&bad_file).unwrap_or_default();
    let bad: HashSet<&str> = bad.lines().filter_map(|l| l.split(' ').next()).collect();
    let early = std::fs::read_to_string(dir.join("early-pipelines.txt")).unwrap_or_default();
    let early: HashSet<&str> = early.lines().collect();
    // Those earlier runs drew with first, the others in no order (by key).
    let mut all: Vec<usize> = (0..keys.len())
        .filter(|&n| {
            let key = keys[n].as_str();
            (again || !done.contains(key)) && !bad.contains(key) && (!used || early.contains(key))
        })
        .collect();
    all.sort_by_key(|&n| (!early.contains(keys[n].as_str()), &keys[n]));
    all.truncate(limit.unwrap_or(usize::MAX));
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = jobs.unwrap_or(cores * 2 / 3).max(1);
    println!(
        "{} pipelines known, {} made before ({fingerprint}), {} bad; {} to make, {jobs} processes",
        keys.len(),
        done.len(),
        bad.len(),
        all.len()
    );
    let chunks: Vec<&[usize]> = all.chunks(AT_ONCE).collect();
    let next = AtomicUsize::new(0);
    let state = Mutex::new((Vec::<f64>::new(), 0usize, 0usize));
    let done_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&done_file)?);
    // (Read back by the runs: these are made beside the others, not before.)
    let slow_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("slow-pipelines.txt"))?);
    let bad_out = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&bad_file)?);
    std::thread::scope(|s| {
        for _ in 0..jobs.min(chunks.len()) {
            s.spawn(|| {
                while let Some(&chunk) = chunks.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let mut rest = chunk;
                    while !rest.is_empty() {
                        let (made, ended) = child(rest, |n, ms, there| {
                            if ms >= SLOW_MS {
                                let _ = writeln!(lock(&slow_out), "\n{} {ms:.0}", keys[n]);
                            }
                            let mut state = lock(&state);
                            state.0.push(ms);
                            state.1 += usize::from(!there);
                            let count = state.0.len();
                            if count % 1000 == 0 {
                                println!("{count} of {}, {:.0} s", all.len(), began.elapsed().as_secs_f64());
                            }
                            drop(state);
                            let _ = writeln!(lock(&done_out), "{fingerprint} {}", keys[n]);
                        });
                        rest = &rest[made.min(rest.len())..];
                        match (ended, rest.split_first()) {
                            (Some(why), Some((&n, after))) => {
                                // The one it was at.
                                println!("pipeline {n} {}: {why}", keys[n]);
                                let _ = writeln!(lock(&bad_out), "{} {why}", keys[n]);
                                lock(&state).2 += 1;
                                rest = after;
                            }
                            _ => break,
                        }
                    }
                }
            });
        }
    });
    let (mut times, refused, crashed) = state.into_inner().unwrap_or_else(|e| e.into_inner());
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times.get((times.len() as f64 * q) as usize).copied().unwrap_or(0.0);
    println!(
        "{} made in {:.0} s ({refused} have no host pipeline, {crashed} ended their process); ms each: median {:.1}, p90 {:.1}, p99 {:.1}, most {:.0}, sum {:.0} s",
        times.len(),
        began.elapsed().as_secs_f64(),
        at(0.5),
        at(0.9),
        at(0.99),
        times.last().copied().unwrap_or(0.0),
        times.iter().sum::<f64>() / 1e3
    );
    Ok(())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Has a process make `these`; how many it made, and how it ended if not well.
fn child(these: &[usize], mut each: impl FnMut(usize, f64, bool)) -> (usize, Option<String>) {
    let list: Vec<String> = these.iter().map(usize::to_string).collect();
    let spawned = std::env::current_exe().and_then(|exe| {
        use std::os::windows::process::CommandExt;
        const IDLE_PRIORITY_CLASS: u32 = 0x40;
        Command::new(exe)
            .args(["warm", "--these", &list.join(",")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(IDLE_PRIORITY_CLASS)
            .spawn()
    });
    let Ok(mut process) = spawned else { return (these.len(), None) };
    let Some(out) = process.stdout.take() else { return (these.len(), None) };
    let process = Mutex::new(process);
    let (heard, over) = (Mutex::new(Instant::now()), AtomicBool::new(false));
    let hung = AtomicBool::new(false);
    let mut made = 0;
    std::thread::scope(|s| {
        s.spawn(|| {
            while !over.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                if lock(&heard).elapsed() > LONGEST {
                    hung.store(true, Ordering::Relaxed);
                    let _ = lock(&process).kill();
                    return;
                }
            }
        });
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            *lock(&heard) = Instant::now();
            let mut words = line.split(' ');
            if words.next() == Some("made")
                && let (Some(n), Some(ms), Some(there)) = (words.next(), words.next(), words.next())
                && let (Ok(n), Ok(ms)) = (n.parse(), ms.parse())
            {
                made += 1;
                each(n, ms, there == "1");
            }
        }
        over.store(true, Ordering::Relaxed);
    });
    let status = lock(&process).wait();
    let well = status.as_ref().is_ok_and(|s| s.success());
    if well {
        return (these.len(), None);
    }
    let why = if hung.load(Ordering::Relaxed) {
        format!("hung for {} s", LONGEST.as_secs())
    } else {
        status.map_or_else(|e| e.to_string(), |s| format!("{s}"))
    };
    (made, Some(why))
}

/// What such a process does.
fn make(dir: &Path, these: &str) -> Result<()> {
    let these: Vec<usize> = these.split(',').map(|n| n.parse().context("--these")).collect::<Result<_>>()?;
    let cfg = victory_core::Config::load()?;
    victory_hle::known_pipelines::use_translations(cfg.cache_subdir("shaders")?);
    let mut out = std::io::stdout();
    known::make(dir, &these, |n, came| {
        let told = match came {
            None => writeln!(out, "at {n}"),
            Some((ms, there)) => writeln!(out, "made {n} {ms:.2} {}", u8::from(there)),
        };
        if told.and_then(|()| out.flush()).is_err() {
            // Whoever asked is gone.
            std::process::exit(1);
        }
    });
    // Without what a run's end does (nothing to save, the host's threads).
    let _ = out.flush();
    std::process::exit(0)
}
||||||| 085ee7c4
=======
//! `victory warm`: the title's shaders translated into the shader cache
//! before a run (see docs/precompilacao-shaders.md).
//!
//! A run translates each shader when the title creates a pipeline with it,
//! on the pipeline workers, while the title waits up to its budget per
//! frame (past it the draw is dropped). The translation is a function of
//! the container's bytes and of this build alone (the cache key:
//! `victory_gpu::shader_cache`), so every container found in the title's
//! files can be translated ahead of time into the same entries. The run
//! then reads them back instead.
//!
//! Not covered: shaders a title makes or loads only in memory (still
//! translated during the run), and the host driver's compile of each
//! pipeline, which needs the graphics state only the title gives (the
//! pipeline library keeps those from the first run on).

use std::path::PathBuf;

use anyhow::Result;

#[cfg(windows)]
pub fn run(paths: Vec<PathBuf>, list: bool, jobs: Option<usize>) -> Result<()> {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use std::time::Instant;

    use victory_hle::modules::d3d12x;
    use victory_shader::Container;

    use crate::shader::{MAX_FILE, sources};

    let paths = crate::default_paths(paths)?;
    let cfg = victory_core::Config::load()?;
    let dir = cfg.cache_subdir("shaders")?;
    println!("shader cache {}", dir.display());
    println!(
        "note         entries are only valid for this victory.exe and its VICTORY_* settings;"
    );
    println!("             warm with the same build and settings you run the title with");
    d3d12x::use_shader_cache(dir);

    let threads =
        jobs.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
    let seen = Mutex::new(HashSet::new());
    let failures: Mutex<BTreeMap<String, (usize, String)>> = Mutex::new(BTreeMap::new());
    let (found, cached, translated, failed) = (
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    );
    let started = Instant::now();
    for root in &paths {
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() || entry.metadata().map_or(0, |m| m.len()) > MAX_FILE {
                continue;
            }
            let Ok(data) = std::fs::read(entry.path()) else {
                continue;
            };
            let jobs = sources(&data);
            if jobs.is_empty() {
                continue;
            }
            let path = entry.path();
            let next = AtomicUsize::new(0);
            std::thread::scope(|s| {
                for _ in 0..threads.min(jobs.len()) {
                    s.spawn(|| {
                        while let Some(job) = jobs.get(next.fetch_add(1, Relaxed)) {
                            let Some(bytes) = job.bytes() else {
                                continue;
                            };
                            // The bytes a title passes: this container only,
                            // not the rest of the file after it.
                            let Ok(container) = Container::bytes_of(&bytes) else {
                                continue;
                            };
                            let new = seen
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(blake3::hash(container));
                            if !new {
                                continue;
                            }
                            found.fetch_add(1, Relaxed);
                            match d3d12x::warm_shader(container) {
                                Ok(false) => {
                                    cached.fetch_add(1, Relaxed);
                                }
                                Ok(true) => {
                                    translated.fetch_add(1, Relaxed);
                                }
                                Err(e) => {
                                    failed.fetch_add(1, Relaxed);
                                    let example = format!("{}{}", path.display(), job.label());
                                    if list {
                                        println!("does not translate: {example}: {e}");
                                    }
                                    let mut f = failures.lock().unwrap_or_else(|e| e.into_inner());
                                    f.entry(e).or_insert((0, example)).0 += 1;
                                }
                            }
                        }
                    });
                }
            });
        }
    }
    let (found, cached, translated, failed) = (
        found.into_inner(),
        cached.into_inner(),
        translated.into_inner(),
        failed.into_inner(),
    );
    println!(
        "containers   {found} distinct: {translated} translated now, {cached} already cached, \
         {failed} do not translate ({:.1} s)",
        started.elapsed().as_secs_f64()
    );
    let mut failures: Vec<_> = failures
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .collect();
    failures.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (reason, (n, example)) in failures.iter().take(20) {
        println!("  {n:>7}  {reason}  (e.g. {example})");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run(_: Vec<PathBuf>, _: bool, _: Option<usize>) -> Result<()> {
    anyhow::bail!("warming the shader cache requires Windows (the host D3D12 device)")
}
>>>>>>> vitoriabr/main
