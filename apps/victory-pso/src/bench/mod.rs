//! `bench`: what the host's D3D12 driver does with work of our own, timed.
//!
//! Nothing of a title is loaded or drawn: the shaders are in these files and
//! the data is counted up from zero. Each row separates the CPU's time
//! (recording, submitting, waiting) from the GPU's own (timestamps), because
//! the two run beside each other and a wall time alone says neither. Every
//! group also checks its results, so a driver that is fast and wrong fails.

mod draws;
mod gpu;
mod pipelines;
mod present;
mod resources;
mod sync;
mod upload;

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use serde::Serialize;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::ProcessStatus::K32EnumProcessModules;
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::Interface;

use crate::adapter::{self, Adapter, Want};
use gpu::{Queue, Timed};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Group {
    /// Draws with each kind of state change between them, and submissions.
    Draws,
    /// Copies between the CPU and the card, on each queue.
    Upload,
    /// Fences and waits.
    Sync,
    /// Pipeline state creation: cold, again, from a blob, on threads.
    Pipelines,
    /// Resource, view and barrier costs.
    Resources,
    /// A swap chain. Opens a small window for a few seconds, so it only
    /// runs when asked for by name.
    Present,
}

#[derive(Args)]
pub struct Options {
    #[arg(long, value_enum, default_value_t = Want::Nvidia)]
    adapter: Want,
    /// The groups to run; all but `present` unless named.
    #[arg(long, value_enum, value_delimiter = ',')]
    only: Vec<Group>,
    /// Measurements of each row, after two that are thrown away.
    #[arg(long, default_value_t = 15)]
    rounds: usize,
    /// A fraction of the work: for checking the program, not the card.
    #[arg(long)]
    quick: bool,
    /// Run under D3D12's debug layer, and fail on what it reports. Its
    /// times say nothing about the driver.
    #[arg(long)]
    debug_layer: bool,
    /// Also write the report (TOML) here, outside the source tree.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// What a group runs with.
pub struct Bench {
    adapter: Adapter,
    direct: Queue,
    compute: Queue,
    copy: Queue,
    rounds: usize,
    quick: bool,
    rows: Vec<Row>,
}

/// The middle of some measurements, and their spread.
#[derive(Clone, Copy, Serialize, Debug, PartialEq)]
pub struct Stat {
    pub min: f64,
    pub median: f64,
    pub p95: f64,
}

impl Stat {
    pub fn of(values: &[f64]) -> Option<Stat> {
        let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
        v.sort_by(f64::total_cmp);
        let round = |x: f64| (x * 1000.0).round() / 1000.0;
        // Nearest rank: the smallest value at least 95% of them do not pass.
        let p95 = (v.len() * 95).div_ceil(100).max(1) - 1;
        Some(Stat {
            min: round(*v.first()?),
            median: round(if v.len() % 2 == 1 {
                v[v.len() / 2]
            } else {
                (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
            }),
            p95: round(v[p95]),
        })
    }
}

/// One measurement: times in microseconds.
#[derive(Serialize, Default)]
pub struct Row {
    pub group: String,
    pub case: String,
    /// What the row counts (draws, copies, pipelines), and how many.
    pub items: String,
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submit_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_busy_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_idle_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_delay_us: Option<Stat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wake_delay_us: Option<Stat>,
    /// Bytes moved in one measurement, for the rows that move data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// `bytes` over the GPU's time where it has one, else over the CPU's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gib_per_s: Option<f64>,
    /// What was compared, if anything.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub checked: String,
}

impl Row {
    pub fn new(group: &str, case: &str, items: &str, count: usize) -> Row {
        Row {
            group: group.to_owned(),
            case: case.to_owned(),
            items: items.to_owned(),
            count: count as u64,
            ..Row::default()
        }
    }

    pub fn timed(mut self, all: &[Timed]) -> Row {
        let some =
            |f: fn(&Timed) -> Option<f64>| Stat::of(&all.iter().filter_map(f).collect::<Vec<_>>());
        self.wall_us = some(|t| Some(t.wall));
        self.record_us = some(|t| Some(t.record));
        self.submit_us = some(|t| Some(t.submit));
        self.wait_us = some(|t| Some(t.wait));
        self.gpu_busy_us = some(|t| t.gpu_busy);
        self.gpu_idle_us = some(|t| t.gpu_idle);
        self.start_delay_us = some(|t| t.start_delay);
        self.wake_delay_us = some(|t| t.wake_delay);
        self
    }

    /// A row whose only time is the CPU's.
    pub fn cpu(mut self, us: &[f64]) -> Row {
        self.wall_us = Stat::of(us);
        self
    }

    pub fn moving(mut self, bytes: u64) -> Row {
        let time = self.gpu_busy_us.or(self.wall_us).map(|s| s.median);
        self.bytes = Some(bytes);
        self.gib_per_s = time
            .filter(|&t| t > 0.0)
            .map(|t| (bytes as f64 / t * 1e6 / f64::from(1 << 30) * 100.0).round() / 100.0);
        self
    }

    pub fn checked(mut self, what: &str) -> Row {
        self.checked = what.to_owned();
        self
    }
}

impl Bench {
    /// `full`, or a small part of it under `--quick`.
    fn size(&self, full: usize) -> usize {
        if self.quick { (full / 50).max(8) } else { full }
    }

    /// Two measurements to warm up and the ones that count.
    fn rounds<T>(&self, mut one: impl FnMut() -> Result<T>) -> Result<Vec<T>> {
        for _ in 0..2 {
            one()?;
        }
        (0..self.rounds).map(|_| one()).collect()
    }

    fn row(&mut self, row: Row) {
        println!("{}", line(&row));
        self.rows.push(row);
    }
}

fn line(r: &Row) -> String {
    let time = |s: Option<Stat>| match s {
        Some(s) => format!("{:>11.1}", s.median),
        None => format!("{:>11}", "-"),
    };
    let each = r.wall_us.map_or(0.0, |s| s.median) / r.count.max(1) as f64;
    let mut out = format!(
        "  {:<44}{:>7}{}{}{}{}{}{}{each:>10.3}",
        r.case,
        r.count,
        time(r.wall_us),
        time(r.record_us),
        time(r.submit_us),
        time(r.wait_us),
        time(r.gpu_busy_us),
        time(r.gpu_idle_us),
    );
    if let Some(speed) = r.gib_per_s {
        out.push_str(&format!("  {speed:.2} GiB/s"));
    }
    // Where other work on the machine may have got into a row.
    if r.wall_us.is_some_and(|s| s.p95 > 2.0 * s.median) {
        out.push_str("  (unsteady)");
    }
    out
}

#[derive(Serialize)]
struct Report {
    adapter: String,
    vendor: String,
    driver: String,
    software: bool,
    rounds: usize,
    quick: bool,
    debug_layer: bool,
    /// Video memory the system lets this process use.
    video_budget_mib: u64,
    /// Modules of others inside the process.
    hooks: Vec<String>,
    features: std::collections::BTreeMap<&'static str, String>,
    row: Vec<Row>,
}

/// The user-mode driver's version, as DXGI gives it.
fn driver(dxgi: &IDXGIAdapter) -> String {
    // SAFETY: a query on a live adapter.
    match unsafe { dxgi.CheckInterfaceSupport(&IDXGIDevice::IID) } {
        Ok(v) => format!(
            "{}.{}.{}.{}",
            v >> 48 & 0xffff,
            v >> 32 & 0xffff,
            v >> 16 & 0xffff,
            v & 0xffff
        ),
        Err(e) => format!("not given: {}", e.message()),
    }
}

/// Modules in this process that came from neither Windows (its folder holds
/// the drivers too) nor this program: overlays and capture tools hook D3D12
/// from inside, and what they cost would read as the driver's.
fn hooks() -> Vec<String> {
    let path = |module| {
        let mut name = [0u16; 1024];
        // SAFETY: a buffer of the length given.
        let len = unsafe { GetModuleFileNameW(module, &mut name) } as usize;
        String::from_utf16_lossy(&name[..len]).to_lowercase()
    };
    let mut windows = [0u16; 260];
    let mut modules = [HMODULE::default(); 1024];
    let mut bytes = 0;
    // SAFETY: buffers of the lengths given.
    let (len, listed) = unsafe {
        (
            GetWindowsDirectoryW(Some(&mut windows)) as usize,
            K32EnumProcessModules(
                GetCurrentProcess(),
                modules.as_mut_ptr(),
                size_of_val(&modules) as u32,
                &mut bytes,
            ),
        )
    };
    if !listed.as_bool() || len == 0 || len > windows.len() {
        return vec!["the list of modules could not be read".to_owned()];
    }
    let windows = format!(
        "{}\\",
        String::from_utf16_lossy(&windows[..len]).to_lowercase()
    );
    let own = path(None);
    let count = (bytes as usize / size_of::<HMODULE>()).min(modules.len());
    modules[..count]
        .iter()
        .map(|&m| path(Some(m)))
        .filter(|p| !p.starts_with(&windows) && *p != own)
        .collect()
}

/// What the rows depend on.
fn features(device: &ID3D12Device) -> std::collections::BTreeMap<&'static str, String> {
    fn ask<T: Default>(device: &ID3D12Device, what: D3D12_FEATURE) -> Option<T> {
        let mut data = T::default();
        // SAFETY: a feature query with its own structure.
        unsafe { device.CheckFeatureSupport(what, (&raw mut data).cast(), size_of::<T>() as u32) }
            .ok()
            .map(|()| data)
    }
    let mut out = std::collections::BTreeMap::new();
    let mut add = |name, value: Option<String>| {
        out.insert(name, value.unwrap_or_else(|| "unknown".to_owned()));
    };
    let o: Option<D3D12_FEATURE_DATA_D3D12_OPTIONS> = ask(device, D3D12_FEATURE_D3D12_OPTIONS);
    add(
        "resource_binding_tier",
        o.map(|o| o.ResourceBindingTier.0.to_string()),
    );
    add(
        "resource_heap_tier",
        o.map(|o| o.ResourceHeapTier.0.to_string()),
    );
    add(
        "tiled_resources_tier",
        o.map(|o| o.TiledResourcesTier.0.to_string()),
    );
    let o: Option<D3D12_FEATURE_DATA_D3D12_OPTIONS12> = ask(device, D3D12_FEATURE_D3D12_OPTIONS12);
    add(
        "enhanced_barriers",
        o.map(|o| o.EnhancedBarriersSupported.as_bool().to_string()),
    );
    let o: Option<D3D12_FEATURE_DATA_D3D12_OPTIONS16> = ask(device, D3D12_FEATURE_D3D12_OPTIONS16);
    add(
        "gpu_upload_heaps",
        o.map(|o| o.GPUUploadHeapSupported.as_bool().to_string()),
    );
    let o: Option<D3D12_FEATURE_DATA_D3D12_OPTIONS3> = ask(device, D3D12_FEATURE_D3D12_OPTIONS3);
    add(
        "copy_queue_timestamps",
        o.map(|o| o.CopyQueueTimestampQueriesSupported.as_bool().to_string()),
    );
    out
}

/// What the debug layer has to say: an error if any of it is one.
fn debug_messages(device: &ID3D12Device) -> Result<()> {
    let queue: ID3D12InfoQueue = device.cast().context("no debug layer messages")?;
    let mut bad = Vec::new();
    // SAFETY: each message is read into a buffer of the size it asked for.
    unsafe {
        for n in 0..queue.GetNumStoredMessages() {
            let mut size = 0;
            queue.GetMessage(n, None, &mut size)?;
            // Eight-byte units, for the structure's alignment.
            let mut space = vec![0u64; size.div_ceil(8)];
            let message = space.as_mut_ptr().cast::<D3D12_MESSAGE>();
            queue.GetMessage(n, Some(message), &mut size)?;
            let m = &*message;
            // Corruption, error and warning are 0, 1 and 2.
            if m.Severity.0 <= D3D12_MESSAGE_SEVERITY_WARNING.0 {
                let text = std::ffi::CStr::from_ptr(m.pDescription.cast());
                bad.push(text.to_string_lossy().into_owned());
            }
        }
    }
    let total = bad.len();
    bad.sort();
    bad.dedup();
    for b in bad.iter().take(20) {
        println!("  debug layer: {b}");
    }
    ensure!(
        total == 0,
        "the debug layer reported {total} errors or warnings"
    );
    println!("debug layer   : no errors or warnings");
    Ok(())
}

pub fn run(o: &Options) -> Result<()> {
    ensure!(o.rounds > 0, "--rounds must be at least 1");
    if let Some(out) = &o.out {
        victory_core::ensure_outside_repo(out)?;
    }
    if o.debug_layer {
        let mut debug: Option<ID3D12Debug> = None;
        // SAFETY: before any device exists.
        unsafe {
            D3D12GetDebugInterface(&mut debug)
                .context("no debug layer (Windows' optional feature `Graphics Tools`)")?;
            debug.context("no debug layer")?.EnableDebugLayer();
        }
    }
    let a = adapter::pick(o.adapter)?;
    // SAFETY: plain DXGI queries.
    let (dxgi, budget) = unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let dxgi: IDXGIAdapter3 = factory.EnumAdapterByLuid(a.device.GetAdapterLuid())?;
        let mut memory = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        dxgi.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut memory)?;
        (dxgi, memory.Budget >> 20)
    };
    let mut report = Report {
        adapter: a.name.clone(),
        vendor: format!("{:#06x}", a.vendor),
        driver: driver(&dxgi.cast()?),
        software: a.software,
        rounds: o.rounds,
        quick: o.quick,
        debug_layer: o.debug_layer,
        video_budget_mib: budget,
        hooks: Vec::new(),
        features: features(&a.device),
        row: Vec::new(),
    };
    println!("adapter       : {} ({})", report.adapter, report.vendor);
    println!("driver        : {}", report.driver);
    println!("video budget  : {budget} MiB");
    for (name, value) in &report.features {
        println!("{name:<22}: {value}");
    }
    let mut bench = Bench {
        direct: Queue::new(&a.device, D3D12_COMMAND_LIST_TYPE_DIRECT)?,
        compute: Queue::new(&a.device, D3D12_COMMAND_LIST_TYPE_COMPUTE)?,
        copy: Queue::new(&a.device, D3D12_COMMAND_LIST_TYPE_COPY)?,
        adapter: a,
        rounds: o.rounds,
        quick: o.quick,
        rows: Vec::new(),
    };
    println!(
        "\n  {:<44}{:>7}{:>11}{:>11}{:>11}{:>11}{:>11}{:>11}{:>10}",
        "median us",
        "count",
        "wall",
        "record",
        "submit",
        "wait",
        "gpu busy",
        "gpu idle",
        "wall/each"
    );
    let groups: [(Group, &str, fn(&mut Bench) -> Result<()>); 6] = [
        (Group::Draws, "draws", draws::run),
        (Group::Upload, "upload", upload::run),
        (Group::Sync, "sync", sync::run),
        (Group::Pipelines, "pipelines", pipelines::run),
        (Group::Resources, "resources", resources::run),
        (Group::Present, "present", present::run),
    ];
    let mut failed = None;
    for (group, name, run) in groups {
        let wanted = if o.only.is_empty() {
            group != Group::Present
        } else {
            o.only.contains(&group)
        };
        if !wanted {
            continue;
        }
        println!("{name}");
        let done = run(&mut bench).and_then(|()| gpu::alive(&bench.adapter.device));
        if let Err(e) = done {
            failed = Some(e.context(format!("in group `{name}`")));
            break;
        }
    }
    // Looked for last: one that waits for a swap chain is in by now.
    report.hooks = hooks();
    if report.hooks.is_empty() {
        println!("hooks         : none");
    }
    for h in &report.hooks {
        println!("hooks         : {h} (the times are with it, not the driver's alone)");
    }
    // The rows that were measured before a failure say where it happened.
    report.row = std::mem::take(&mut bench.rows);
    if let Some(out) = &o.out {
        std::fs::write(out, toml::to_string(&report)?)
            .with_context(|| format!("writing {}", out.display()))?;
        println!("report        : {}", out.display());
    }
    if o.debug_layer {
        let messages = debug_messages(&bench.adapter.device);
        if failed.is_none() {
            messages?;
        }
    }
    match failed {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_are_the_middle_and_the_nearest_rank() {
        let s = Stat::of(&[5.0, 1.0, 3.0]).unwrap();
        assert_eq!((s.min, s.median, s.p95), (1.0, 3.0, 5.0));
        let s = Stat::of(&[4.0, 1.0, 2.0, 3.0]).unwrap();
        assert_eq!((s.min, s.median, s.p95), (1.0, 2.5, 4.0));
        let many: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(Stat::of(&many).unwrap().p95, 95.0);
        assert_eq!(Stat::of(&[]), None);
        // A time that could not be worked out is not a measurement.
        assert_eq!(Stat::of(&[f64::NAN, 2.0]).unwrap().median, 2.0);
    }

    #[test]
    fn a_speed_is_over_the_gpus_time_where_there_is_one() {
        let gib = 1u64 << 30;
        let cpu = Row::new("g", "c", "bytes", 1).cpu(&[1e6]).moving(gib);
        assert_eq!(cpu.gib_per_s, Some(1.0));
        let t = Timed {
            wall: 1e6,
            gpu_busy: Some(0.5e6),
            ..Timed::default()
        };
        let gpu = Row::new("g", "c", "bytes", 1).timed(&[t]).moving(gib);
        assert_eq!(gpu.gib_per_s, Some(2.0));
    }
}
