//! `victory`: the command-line front end.
//!
//! Every command reads the user's own files and writes only to stdout or to
//! the user's cache directory (outside the repository).

mod agility;
mod census;
#[cfg(windows)]
mod doctor;
mod inspect;
#[cfg(windows)]
mod prebuild;
mod precompile;
#[cfg(all(windows, feature = "diagnostics"))]
mod replay;
mod run;
mod shader;
mod translate;
<<<<<<< HEAD
#[cfg(windows)]
mod warm;
||||||| c7daeea5
=======
mod work;
>>>>>>> integ/vertex

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "victory",
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

fn main() -> Result<()> {
    // LOCAL-ONLY (branch local/win10-agility): keep the Agility exports.
    std::hint::black_box(agility::D3D12SDKVersion);
    std::hint::black_box(&agility::D3D12SDKPath);
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
        Command::Translate {
            paths,
            validate,
            pipelines,
            list,
            jobs,
            cache: false,
            ..
        } => {
            // `VICTORY_VIEW_LIMIT=<null slot>`: translate as on a host whose view heap is capped
            if let Some(n) = std::env::var("VICTORY_VIEW_LIMIT").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_view_heap_limit(n);
            }
            // `VICTORY_HOST_WAVE=32`: translate as for a GPU whose waves are at most 32 lanes
            if let Some(n) = std::env::var("VICTORY_HOST_WAVE").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_host_max_wave(n);
            }
            translate::run(paths, validate, pipelines, list, jobs)
        }
        Command::Precompile {
            paths,
            pipelines,
            only,
            limit,
            jobs,
            pipeline_jobs,
        } => precompile::run(paths, pipelines, only, limit, jobs, pipeline_jobs),
        Command::Shaders { paths, jobs } => translate::precompile(paths, jobs),
        #[cfg(windows)]
        Command::Warm { jobs, limit, again, used, these } => warm::run(jobs, limit, again, used, these),
        Command::Run {
            exe,
            unimplemented,
            strict,
            execute,
            console,
<<<<<<< HEAD
            title_args,
||||||| c7daeea5
            title_args: _,
=======
            work: _,
            title_args: _,
>>>>>>> integ/vertex
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::check_policy;

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
}
