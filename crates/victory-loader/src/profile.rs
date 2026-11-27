//! Where the process's time goes: `VICTORY_PROFILE=<from>,<to>,<ms>,<file>`
//! samples the stacks of the busiest threads every `<ms>` milliseconds
//! between two times (seconds, as `VICTORY_STACKS`), and writes them counted
//! to `<file>` at the end (several of them, one after another, with `;`
//! between). A thread that waits is sampled like one that
//! runs: its top frame says which (`ntdll`'s system calls).
//!
//! The file: `thread <id> <start> <cpu ms> <samples> <name>`, then
//! `stack <thread> <phase> <count> <frame>;<frame>;...` (innermost first, as
//! `module+rva`), where the phase is what the caller's function says of the
//! frame at that moment.

use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows_sys::Win32::System::Threading::{
    GetThreadTimes, OpenThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
};

use crate::crash::locate;
use crate::stacks::{frames_of_depth, name_of, other_threads, start_of};

/// The threads sampled: those that ran most since they were last chosen.
const BUSIEST: usize = 28;
/// How often they are chosen anew.
const CHOOSE: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq)]
struct Options {
    threads: usize,
    filter: String,
    depth: usize,
    symbolize: bool,
}

impl Options {
    fn parse(threads: &str, filter: &str, depth: &str, symbolize: &str) -> Self {
        Self {
            threads: threads
                .trim()
                .parse::<usize>()
                .map_or(BUSIEST, |n| n.clamp(4, BUSIEST)),
            filter: filter.trim().to_lowercase(),
            depth: depth
                .trim()
                .parse::<usize>()
                .map_or(crate::crash::MAX_FRAMES, |n| {
                    n.clamp(1, crate::crash::MAX_FRAMES)
                }),
            symbolize: symbolize.trim() == "1",
        }
    }
}

/// What `VICTORY_PROFILE` says: from, to, every, the file.
fn asked(list: &str) -> Option<(Duration, Duration, Duration, String)> {
    let mut parts = list.splitn(4, ',');
    let mut seconds = || parts.next()?.trim().parse::<f64>().ok();
    let (from, to, ms) = (seconds()?, seconds()?, seconds()?);
    let of = |s: f64| Duration::try_from_secs_f64(s).ok();
    let file = parts.next()?.trim().to_string();
    Some((of(from)?, of(to)?, of(ms / 1000.0)?, file))
        .filter(|(a, b, ms, f)| a < b && !ms.is_zero() && !f.is_empty())
}

struct Thread {
    handle: HANDLE,
    start: String,
    name: String,
    /// Its processor time, 100 ns: at first, when last chosen, now.
    first: u64,
    last: u64,
    now: u64,
    samples: u64,
}

impl Drop for Thread {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: owned handle from OpenThread.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

struct Symbols(HANDLE);

impl Symbols {
    fn open() -> Option<Self> {
        use windows_sys::Win32::System::Diagnostics::Debug::SymInitializeW;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        };
        let exe = std::env::current_exe().ok()?;
        use std::os::windows::ffi::OsStrExt;
        let search: Vec<_> = exe
            .parent()?
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: a unique owned handle to this process. The profiler calls DbgHelp from this
        // thread only, with a local search path. A missing PDB leaves module offsets usable.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                0,
                GetCurrentProcessId(),
            )
        };
        if handle.is_null() {
            return None;
        }
        if unsafe { SymInitializeW(handle, search.as_ptr(), 1) } == 0 {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self(handle))
    }

    fn locate(&self, address: u64) -> String {
        use windows_sys::Win32::System::Diagnostics::Debug::{SYMBOL_INFO, SymFromAddr};
        #[repr(C)]
        struct Buffer {
            info: SYMBOL_INFO,
            name: [u8; 1024],
        }
        // SAFETY: all fields are integers. DbgHelp writes the symbol into the trailing buffer.
        let mut buffer: Buffer = unsafe { std::mem::zeroed() };
        buffer.info.SizeOfStruct = size_of::<SYMBOL_INFO>() as u32;
        buffer.info.MaxNameLen = buffer.name.len() as u32;
        let fallback = locate(address as usize);
        if unsafe { SymFromAddr(self.0, address, std::ptr::null_mut(), &mut buffer.info) } == 0 {
            return fallback;
        }
        let module = fallback
            .split_once('+')
            .map_or(fallback.as_str(), |(module, _)| module);
        let size = (buffer.info.NameLen as usize).min(buffer.name.len());
        // SAFETY: the slice starts at Name and stays within the enclosing buffer.
        let name =
            unsafe { std::slice::from_raw_parts(buffer.info.Name.as_ptr().cast::<u8>(), size) };
        format!(
            "{module}!{}",
            String::from_utf8_lossy(name).replace([';', '\n', '\r'], ",")
        )
    }
}

impl Drop for Symbols {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Diagnostics::Debug::SymCleanup;
        // SAFETY: this thread owns both the DbgHelp session and process handle.
        unsafe {
            SymCleanup(self.0);
            CloseHandle(self.0);
        }
    }
}

fn cpu(thread: HANDLE) -> u64 {
    // SAFETY: zeroed is a valid FILETIME, and the calls write them.
    let mut t: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [a, b, c, d] = &mut t;
    // SAFETY: an open thread; the pointers are to the structures above.
    unsafe { GetThreadTimes(thread, a, b, c, d) };
    let of = |f: &FILETIME| u64::from(f.dwHighDateTime) << 32 | u64::from(f.dwLowDateTime);
    of(&t[2]) + of(&t[3])
}

/// Reads `VICTORY_PROFILE` and, if set, starts the thread that samples.
/// `phase` is asked at each round.
pub fn profile_on_schedule(phase: fn() -> u32) {
    let Ok(list) = std::env::var("VICTORY_PROFILE") else {
        return;
    };
    let options = Options::parse(
        &std::env::var("VICTORY_PROFILE_THREADS").unwrap_or_default(),
        &std::env::var("VICTORY_PROFILE_FILTER").unwrap_or_default(),
        &std::env::var("VICTORY_PROFILE_DEPTH").unwrap_or_default(),
        &std::env::var("VICTORY_PROFILE_SYMBOLIZE").unwrap_or_default(),
    );
    let start = Instant::now();
    let all: Vec<_> = list.split(';').filter_map(asked).collect();
    std::thread::spawn(move || {
        for one in all {
            profile(one, start, phase, &options);
        }
    });
}

fn profile(
    (from, to, every, file): (Duration, Duration, Duration, String),
    start: Instant,
    phase: fn() -> u32,
    options: &Options,
) {
    std::thread::sleep(from.saturating_sub(start.elapsed()));
    eprintln!("VIctory: the profile begins");
    let mut threads: HashMap<u32, Thread> = HashMap::new();
    let mut chosen: Vec<u32> = Vec::new();
    let mut counts: HashMap<(u32, u32, Vec<u64>), u64> = HashMap::new();
    let mut phases: HashMap<u32, u64> = HashMap::new();
    let mut choose_at = Instant::now();
    let mut rounds = 0u64;
    while start.elapsed() < to {
        let round = Instant::now();
        if round >= choose_at {
            choose_at = round + CHOOSE;
            choose(&mut threads, &mut chosen, options);
        }
        let phase = phase();
        *phases.entry(phase).or_default() += 1;
        for id in &chosen {
            if let Some(t) = threads.get_mut(id) {
                let (frames, _) = frames_of_depth(t.handle, options.depth);
                t.samples += 1;
                if !frames.is_empty() {
                    *counts.entry((*id, phase, frames)).or_default() += 1;
                }
            }
        }
        rounds += 1;
        std::thread::sleep(every.saturating_sub(round.elapsed()));
    }
    choose(&mut threads, &mut chosen, options);
    if let Err(e) = write(&file, &threads, &counts, &phases, rounds, options.symbolize) {
        eprintln!("VIctory: the profile was not written: {e}");
    } else {
        eprintln!("VIctory: the profile is in {file}");
    }
}

/// Opens the threads that are new, and chooses the ones to sample.
fn choose(threads: &mut HashMap<u32, Thread>, chosen: &mut Vec<u32>, options: &Options) {
    for id in other_threads() {
        threads.entry(id).or_insert_with(|| {
            // SAFETY: opens a thread of our own process; kept open for the
            // profile's life, so the id stays its own.
            let handle = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                    0,
                    id,
                )
            };
            let first = if handle.is_null() { 0 } else { cpu(handle) };
            Thread {
                handle,
                start: if handle.is_null() {
                    String::new()
                } else {
                    locate(start_of(handle))
                },
                name: if handle.is_null() {
                    String::new()
                } else {
                    name_of(handle)
                },
                first,
                last: first,
                now: first,
                samples: 0,
            }
        });
    }
    let mut ran: Vec<(u64, u32)> = Vec::new();
    for (&id, t) in threads.iter_mut().filter(|(_, t)| !t.handle.is_null()) {
        t.now = cpu(t.handle);
        if !options.filter.is_empty() {
            t.name = name_of(t.handle);
        }
        if options.filter.is_empty() || t.name.to_lowercase().contains(&options.filter) {
            ran.push((t.now.saturating_sub(t.last), id));
        }
        t.last = t.now;
    }
    ran.sort_unstable_by(|a, b| b.cmp(a));
    *chosen = ran
        .into_iter()
        .take(options.threads)
        .map(|(_, id)| id)
        .collect();
}

fn write(
    file: &str,
    threads: &HashMap<u32, Thread>,
    counts: &HashMap<(u32, u32, Vec<u64>), u64>,
    phases: &HashMap<u32, u64>,
    rounds: u64,
    symbolize: bool,
) -> std::io::Result<()> {
    let path = std::path::Path::new(file);
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&partial)?);
    writeln!(out, "rounds {rounds}")?;
    for (phase, n) in phases {
        writeln!(out, "phase {phase} {n}")?;
    }
    let symbols = symbolize.then(Symbols::open).flatten();
    let mut names: HashMap<u64, String> = HashMap::new();
    for (id, t) in threads {
        writeln!(
            out,
            "thread {id} {} {} {} {}",
            t.start,
            t.now.saturating_sub(t.first) / 10_000,
            t.samples,
            t.name.replace(['\n', '\r'], " ")
        )?;
    }
    for ((id, phase, frames), n) in counts {
        write!(out, "stack {id} {phase} {n} ")?;
        for f in frames {
            let name = names.entry(*f).or_insert_with(|| {
                symbols
                    .as_ref()
                    .map_or_else(|| locate(*f as usize), |symbols| symbols.locate(*f))
            });
            write!(out, "{name};")?;
        }
        writeln!(out)?;
    }
    out.flush()?;
    drop(out);
    std::fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiler_tuning_defaults_and_bounds() {
        assert_eq!(
            Options::parse("", "", "", ""),
            Options {
                threads: 28,
                filter: String::new(),
                depth: 40,
                symbolize: false,
            }
        );
        assert_eq!(
            Options::parse("1", " Main ", "77", "1"),
            Options {
                threads: 4,
                filter: "main".into(),
                depth: 40,
                symbolize: true,
            }
        );
        assert_eq!(Options::parse("9", "", "8", "0").depth, 8);
    }

    #[test]
    fn what_is_asked_is_read_whole_or_not_at_all() {
        assert_eq!(
            asked("150, 160.5,5,D:\\a b,c.txt"),
            Some((
                Duration::from_secs(150),
                Duration::from_millis(160_500),
                Duration::from_millis(5),
                "D:\\a b,c.txt".to_string()
            ))
        );
        assert_eq!(asked("150,160,5"), None);
        assert_eq!(asked("160,150,5,x"), None);
        assert_eq!(asked("a,160,5,x"), None);
        assert_eq!(asked("-1,160,5,x"), None);
        assert_eq!(asked("1,160,0,x"), None);
    }
}
