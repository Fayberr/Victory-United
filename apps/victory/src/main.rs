//! `victory`: the command-line front end.
//!
//! Every command reads the user's own files and writes only to stdout or to
//! the user's cache directory (outside the repository).

mod agility;
mod census;
#[cfg(windows)]
mod doctor;
mod inspect;
<<<<<<< HEAD
#[cfg(windows)]
mod prebuild;
mod precompile;
#[cfg(all(windows, feature = "diagnostics"))]
mod replay;
||||||| a66ac212
=======
#[cfg(windows)]
mod prebuild;
>>>>>>> klbf/beta-2
mod run;
mod shader;
mod translate;
#[cfg(windows)]
mod warm;
mod work;

/// Counts the Rust heap's live bytes and their peak for the resident line. Not in diagnostics
/// builds, where `victory-gpu`'s allocator does the counting.
#[cfg(not(feature = "diagnostics"))]
struct Counting;

// SAFETY: every call goes to the system's allocator unchanged; only counters are kept beside it.
#[cfg(not(feature = "diagnostics"))]
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let at = unsafe { std::alloc::System.alloc(layout) };
        if !at.is_null() {
            victory_core::heapcount::grow(layout.size());
        }
        at
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let at = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if !at.is_null() {
            victory_core::heapcount::grow(layout.size());
        }
        at
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: std::alloc::Layout) {
        victory_core::heapcount::shrink(layout.size());
        // SAFETY: the caller's contract, passed on.
        unsafe { std::alloc::System.dealloc(at, layout) };
    }

    unsafe fn realloc(&self, at: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let moved = unsafe { std::alloc::System.realloc(at, layout, size) };
        if !moved.is_null() {
            if size > layout.size() {
                victory_core::heapcount::grow(size - layout.size());
            } else {
                victory_core::heapcount::shrink(layout.size() - size);
            }
        }
        moved
    }
}

#[cfg(not(feature = "diagnostics"))]
#[global_allocator]
static HEAP: Counting = Counting;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "VItoriaBr",
    version,
    about = "Run an Xbox Series title natively on Windows"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Summarise one PE image: headers, sections, imports, TLS, build id.
    Inspect {
        file: PathBuf,
        /// List every imported function.
        #[arg(long)]
        imports: bool,
        /// List every exported function.
        #[arg(long)]
        exports: bool,
    },
    /// Aggregate the imports of every PE image under the given paths and
    /// show which ones our runtime covers. Defaults to the configured
    /// game and test-title directories.
    Census {
        paths: Vec<PathBuf>,
        /// Do not count host pass-through as covered.
        #[arg(long)]
        strict: bool,
        /// List every missing `dll!function`, most used first.
        #[arg(long)]
        missing: bool,
    },
    /// List the `dll!export` pairs an import library describes.
    LibExports {
        lib: PathBuf,
        /// List every export, not just per-DLL counts.
        #[arg(long)]
        list: bool,
    },
    /// Find shader containers in files or directories (also inside engine
    /// archives, if uncompressed) and report their layout.
    ShaderInfo {
        paths: Vec<PathBuf>,
        /// Print one line per container found.
        #[arg(long)]
        verbose: bool,
        /// Also list the decoded native instructions (implies --verbose).
        #[arg(long)]
        disasm: bool,
        /// Show the effects, techniques, and passes in shader databases.
        #[arg(long)]
        effects: bool,
    },
    /// Run every shader found (containers, SGD5 files) through the
    /// translator and report what it cannot handle yet.
    Translate {
        paths: Vec<PathBuf>,
        /// Check emitted DXIL with dxil.dll's validator (`VICTORY_DXIL`,
        /// else the newest Windows SDK's).
        #[arg(long)]
        validate: bool,
        /// Check host pipelines, or prebuild the shader pack's pipelines with --cache.
        #[arg(long)]
        pipelines: bool,
        /// Prebuild only shader-pack pipelines requested by earlier runs in this cache.
        #[arg(long, requires_all = ["cache", "pipelines"])]
        used_pipelines: bool,
        /// Print one line per program and outcome.
        #[arg(long)]
        list: bool,
        /// Worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
<<<<<<< HEAD
        /// Fill the shader cache that `run` reads.
        #[arg(long, conflicts_with_all = ["validate", "list"])]
        cache: bool,
    },
    /// Translate shaders into the runtime cache ahead of time (resumable).
    /// With --pipelines, also compile compute pipelines into the host
    /// pipeline library and GPU driver cache.
    Precompile {
        paths: Vec<PathBuf>,
        /// Also compile compute shaders into `host-pipelines.bin` and the GPU
        /// driver cache.
        #[arg(long)]
        pipelines: bool,
        /// Restrict precompilation to one shader stage.
        #[arg(long, value_enum)]
        only: Option<precompile::StageFilter>,
        /// Stop after this many unique shader containers.
        #[arg(long)]
        limit: Option<usize>,
        /// Translation worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
        /// Concurrent GPU driver compute pipeline compilations (default: 4).
        #[arg(long)]
        pipeline_jobs: Option<usize>,
    },
    /// Translate every shader found (as `translate` finds them) into the
    /// shader cache, so that no run has to: a few minutes, once per change
    /// of the translator.
    Shaders {
        paths: Vec<PathBuf>,
        /// Worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
    },
    /// Make the host pipelines of every pipeline the title ever created
    /// (runs with `VICTORY_PIPELINE_BOOK=1` write them down), so that the
    /// driver's cache on disk has them before a run needs one: once per change
    /// of the translator or the driver. Goes on where it was stopped.
    #[cfg(windows)]
    Warm {
        /// Processes at once (default: two thirds of the logical cores).
        #[arg(long)]
        jobs: Option<usize>,
        /// At most this many pipelines.
        #[arg(long)]
        limit: Option<usize>,
        /// Those made before too.
        #[arg(long)]
        again: bool,
        /// Only the pipelines earlier runs drew with.
        #[arg(long)]
        used: bool,
        /// (What the processes it starts are given.)
        #[arg(long, hide = true)]
        these: Option<String>,
||||||| a66ac212
=======
        /// Warm the run's shader cache: a file of shader hashes (the 16
        /// hex digits a run's log names a shader by, anywhere in a line);
        /// the containers with those hashes are translated into the
        /// cache and nothing else is done. Set `VICTORY_HOST_WAVE` and
        /// `VICTORY_VIEW_LIMIT` to what the run's log says of the host.
        #[arg(long)]
        warm: Option<PathBuf>,
        /// With --pipelines: fill the shader cache and host pipeline
        /// library `run` reads with every pipeline of the title's shader
        /// pack (resumable; honours VICTORY_SKIP_PIPELINES).
        #[arg(long, requires = "pipelines", conflicts_with_all = ["validate", "list"])]
        cache: bool,
    },
    /// Translate the shaders of a title's archives into the cache and make
    /// their compute pipelines, so that a run finds them there.
    Warm {
        paths: Vec<PathBuf>,
        /// Worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
        /// Only translate: no host pipelines.
        #[arg(long)]
        no_pipelines: bool,
>>>>>>> klbf/beta-2
    },
    /// Prepare, map and bind a title executable; with --execute, run it.
    Run {
        exe: PathBuf,
        /// What unimplemented imports do when called.
        #[arg(long, value_enum, default_value_t = Unimplemented::Trap)]
        unimplemented: Unimplemented,
        /// Disable host pass-through (everything must be implemented by us).
        #[arg(long)]
        strict: bool,
        /// Actually start the title. Without it, stop after binding.
        #[arg(long)]
        execute: bool,
        /// The console model the title sees (device type, memory).
        #[arg(long, value_enum, default_value_t = Console::SeriesX)]
        console: Console,
        /// Where work is done that the processor and the graphics card can
        /// both do. Settings set in the environment already stay.
        #[arg(long, value_enum, default_value_t = work::Work::Balanced)]
        work: work::Work,
        /// Arguments for the title, after `--` (it reads the whole command
        /// line; a title that relaunches itself passes some).
        #[arg(last = true)]
        title_args: Vec<String>,
    },
    /// Run a captured frame's GPU work again (diagnostics builds; see
    /// docs/replay.md).
    #[cfg(all(windows, feature = "diagnostics"))]
    Replay(replay::Replay),
    /// Show the resolved configuration and check it.
    Config,
    /// Check what this PC has of what a run needs (graphics, system DLLs,
    /// memory, folders). Loads no title.
    #[cfg(windows)]
    Doctor,
}

#[derive(Clone, Copy, ValueEnum)]
enum Unimplemented {
    Trap,
    Zero,
}

#[derive(Clone, Copy, ValueEnum)]
enum Console {
    SeriesX,
    SeriesS,
}

/// (Neptune) Diagnostics builds: the redistributable D3D12 runtime and debug layer in `D3D12\`
/// beside the executable are used if they are there (`build.rs` exports these two; the
/// version is the middle number of the package's, 1.620.2).
#[cfg(all(windows, feature = "diagnostics"))]
#[unsafe(no_mangle)]
#[used]
pub static D3D12SDKVersion: u32 = 620;

#[cfg(all(windows, feature = "diagnostics"))]
#[repr(transparent)]
pub struct SdkPath(*const u8);
// SAFETY: a pointer to a constant string.
#[cfg(all(windows, feature = "diagnostics"))]
unsafe impl Sync for SdkPath {}

#[cfg(all(windows, feature = "diagnostics"))]
#[unsafe(no_mangle)]
#[used]
pub static D3D12SDKPath: SdkPath = SdkPath(b".\\D3D12\\\0".as_ptr());

fn main() -> Result<()> {
<<<<<<< HEAD
    // LOCAL-ONLY (branch local/win10-agility): keep the Agility exports.
    std::hint::black_box(agility::D3D12SDKVersion);
    std::hint::black_box(&agility::D3D12SDKPath);
||||||| a66ac212
=======
    // `VICTORY_HEAP_COUNT=1`: the Rust heap's live bytes for the resident line (off: it slowed loading).
    victory_core::heapcount::enable(std::env::var_os("VICTORY_HEAP_COUNT").is_some_and(|v| v == "1"));
>>>>>>> klbf/beta-2
    victory_core::logging::init("info");
    // The audio recorder `run` starts for `VICTORY_AUDIO_CAPTURE`.
    #[cfg(all(windows, feature = "diagnostics"))]
    if victory_hle::modules::audio::capture::child() {
        return Ok(());
    }
    let cli = Cli::parse();
    if let Command::Run { work, .. } = &cli.command {
        // SAFETY: nothing so far has started a thread.
        unsafe { work.apply() };
    }
    match cli.command {
        Command::Inspect {
            file,
            imports,
            exports,
        } => inspect::run(&file, imports, exports),
        Command::Census {
            paths,
            strict,
            missing,
        } => census::run(paths, strict, missing),
        Command::LibExports { lib, list } => inspect::lib_exports(&lib, list),
        Command::ShaderInfo {
            paths,
            verbose,
            disasm,
<<<<<<< HEAD
            effects,
        } => shader::run(paths, verbose || disasm, disasm, effects),
        Command::Translate {
            paths,
            jobs,
            pipelines,
            used_pipelines,
            cache: true,
            ..
        } => translate::warm(paths, jobs, pipelines, used_pipelines),
||||||| a66ac212
        } => shader::run(paths, verbose || disasm, disasm),
=======
        } => shader::run(paths, verbose || disasm, disasm),
        #[cfg(windows)]
        Command::Translate {
            paths,
            jobs,
            cache: true,
            ..
        } => prebuild::run(paths, jobs),
>>>>>>> klbf/beta-2
        Command::Translate {
            paths,
            validate,
            pipelines,
            list,
            jobs,
<<<<<<< HEAD
            cache: false,
            ..
||||||| a66ac212
=======
            warm,
            ..
>>>>>>> klbf/beta-2
        } => {
<<<<<<< HEAD
            apply_host_shader_overrides_from_env();
||||||| a66ac212
            // `VICTORY_VIEW_LIMIT=<null slot>`: translate as on a host whose view heap is capped
            if let Some(n) = std::env::var("VICTORY_VIEW_LIMIT").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_view_heap_limit(n);
            }
            // `VICTORY_HOST_WAVE=32`: translate as for a GPU whose waves are at most 32 lanes
            if let Some(n) = std::env::var("VICTORY_HOST_WAVE").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_host_max_wave(n);
            }
=======
            // `VICTORY_VIEW_LIMIT=<null slot>`: translate as on a host whose view heap is capped
            if let Some(n) = std::env::var("VICTORY_VIEW_LIMIT").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_view_heap_limit(n);
            }
            // `VICTORY_HOST_WAVE=32`: translate as for a GPU whose waves are at most 32 lanes
            if let Some(n) = std::env::var("VICTORY_HOST_WAVE").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_host_max_wave(n);
            }
            if let Some(file) = warm {
                return translate::warm_translations(paths, &file, jobs);
            }
>>>>>>> klbf/beta-2
            translate::run(paths, validate, pipelines, list, jobs)
        }
<<<<<<< HEAD
        Command::Precompile {
            paths,
            pipelines,
            only,
            limit,
            jobs,
            pipeline_jobs,
        } => precompile::run(paths, pipelines, only, limit, jobs, pipeline_jobs),
        Command::Shaders { paths, jobs } => {
            apply_host_shader_overrides_from_env();
            translate::precompile(paths, jobs)
        }
        #[cfg(windows)]
        Command::Warm { jobs, limit, again, used, these } => warm::run(jobs, limit, again, used, these),
||||||| a66ac212
=======
        Command::Warm { paths, jobs, no_pipelines } => translate::warm(paths, jobs, !no_pipelines),
>>>>>>> klbf/beta-2
        Command::Run {
            exe,
            unimplemented,
            strict,
            execute,
            console,
<<<<<<< HEAD
            work: _,
            title_args,
        } => {
            victory_hle::runtime::set_title_args(title_args);
            let code = run::run(&exe, unimplemented, strict, execute, console)?;
            if code != 0 {
                // Preserve all 32 bits of the Windows process status.
                std::process::exit(code as i32);
            }
            Ok(())
        }
        #[cfg(all(windows, feature = "diagnostics"))]
        Command::Replay(options) => replay::run(options),
||||||| a66ac212
            title_args: _,
        } => run::run(&exe, unimplemented, strict, execute, console),
=======
            title_args: _,
        } => {
            let result = run::run(&exe, unimplemented, strict, execute, console);
            // The process's own end, as `main`'s Result maps it: Ok is 0,
            // Err is 1. A title that ends the process itself logs its own
            // line and never returns here.
            let code = if result.is_ok() { 0 } else { 1 };
            tracing::info!("runtime exit: code {code}");
            result
        }
>>>>>>> klbf/beta-2
        Command::Config => show_config(),
        #[cfg(windows)]
        Command::Doctor => doctor::run(),
    }
}

fn show_config() -> Result<()> {
    let cfg = victory_core::Config::load()?;
    let status = |p: &Option<PathBuf>| match p {
        Some(p) if p.exists() => format!("{}", p.display()),
        Some(p) => format!("{}  (does not exist)", p.display()),
        None => "(not set)".to_owned(),
    };
    println!(
        "config file : {}",
        cfg.source
            .as_ref()
            .map_or("(none)".into(), |p| p.display().to_string())
    );
    println!("game_dir    : {}", status(&cfg.game_dir));
    println!("cache_dir   : {}", cfg.cache_dir.display());
    println!("data_dir    : {}", cfg.data_dir.display());
    for t in &cfg.test_titles {
        println!("test_title  : {}", status(&Some(t.clone())));
    }
    check_policy(&cfg)
}

/// Reports each configured path the asset policy refuses, and fails if
/// there is one, so that `victory config` exits nonzero.
fn check_policy(cfg: &victory_core::Config) -> Result<()> {
    let mut violations = 0;
    for p in cfg
        .game_dir
        .iter()
        .chain(&cfg.test_titles)
        .chain([&cfg.cache_dir, &cfg.data_dir])
    {
        if let Err(e) = victory_core::ensure_outside_repo(p) {
            println!("POLICY VIOLATION: {e}");
            violations += 1;
        }
    }
    anyhow::ensure!(
        violations == 0,
        "{violations} configured path(s) violate the asset policy"
    );
    Ok(())
}

/// Paths given on the command line, or the configured game + test titles.
pub(crate) fn default_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if !paths.is_empty() {
        return Ok(paths);
    }
    let cfg = victory_core::Config::load()?;
    let all: Vec<PathBuf> = cfg.game_dir.into_iter().chain(cfg.test_titles).collect();
    anyhow::ensure!(
        !all.is_empty(),
        "no paths given and none configured (see victory.example.toml)"
    );
    Ok(all)
}

/// Applies offline `VICTORY_VIEW_LIMIT` and `VICTORY_HOST_WAVE` overrides so
/// `victory translate` and `victory warm` produce cache keys matching the
/// runtime device's view-heap cap and wave width.
fn apply_host_shader_overrides(view_limit: Option<&str>, host_wave: Option<&str>) {
    if let Some(n) = view_limit.and_then(|v| v.trim().parse().ok()) {
        victory_shader::emit::set_view_heap_limit(n);
    }
    if let Some(n) = host_wave.and_then(|v| v.trim().parse().ok()) {
        victory_shader::emit::set_host_max_wave(n);
    }
}

fn apply_host_shader_overrides_from_env() {
    apply_host_shader_overrides(
        std::env::var("VICTORY_VIEW_LIMIT").ok().as_deref(),
        std::env::var("VICTORY_HOST_WAVE").ok().as_deref(),
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{apply_host_shader_overrides, check_policy};
    use victory_shader::emit::{
        host_max_wave, set_host_max_wave, set_view_heap_limit, translation_fingerprint,
        view_heap_limit,
    };

    fn outside() -> PathBuf {
        std::env::temp_dir().join("victory-config-test")
    }

    #[test]
    fn config_check_passes_paths_outside_the_repository() {
        let cfg = victory_core::Config {
            cache_dir: outside().join("cache"),
            data_dir: outside().join("data"),
            ..Default::default()
        };
        assert!(check_policy(&cfg).is_ok());
    }

    #[test]
    fn config_check_fails_a_path_inside_the_repository() {
        let cfg = victory_core::Config {
            game_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("game")),
            cache_dir: outside().join("cache"),
            data_dir: outside().join("data"),
            ..Default::default()
        };
        let err = check_policy(&cfg).unwrap_err();
        assert!(err.to_string().contains("1 configured path"), "{err}");
    }

    #[test]
    fn offline_host_overrides_update_shader_cache_fingerprint() {
        let prev_limit = view_heap_limit();
        let prev_wave = host_max_wave();

        set_view_heap_limit(0);
        set_host_max_wave(0);
        let default_fp = translation_fingerprint();

        apply_host_shader_overrides(Some(" 999999 "), Some("32"));
        assert_eq!(view_heap_limit(), 999_999);
        assert_eq!(host_max_wave(), 32);
        let tuned_fp = translation_fingerprint();
        assert_ne!(default_fp, tuned_fp);

        // Invalid values keep existing settings untouched.
        apply_host_shader_overrides(Some("bad"), None);
        assert_eq!(view_heap_limit(), 999_999);
        assert_eq!(host_max_wave(), 32);

        set_view_heap_limit(prev_limit);
        set_host_max_wave(prev_wave);
    }
}
