//! `victory-pso`: console shader containers and pipeline state to host
//! D3D12 pipelines, on their own.
//!
//! Nothing of a title is loaded: no executable, launcher, asset or runtime.
//! `build` runs the translator (`victory_shader::emit::translate`) over
//! the shader containers a pipeline description names, maps the state the
//! description gives, and writes the DXIL with a manifest of its bindings.
//! `verify` creates the root signature and pipeline state those define on an
//! adapter picked through DXGI, and can run a compute pipeline and compare
//! what it wrote. `fixture` writes synthetic inputs for both. `bench` times
//! the host's D3D12 driver on work of its own.
//!
//! Exit codes: 0 done; 1 an error (bad input, unsupported state, a pipeline
//! the device refuses); 3 the adapter asked for is not there; 4 a run's
//! output differs from what was expected.

#[cfg(windows)]
mod adapter;
#[cfg(windows)]
mod bench;
mod build;
mod fixture;
mod manifest;
mod state;
#[cfg(windows)]
mod verify;
mod wrap;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use build::Wave64Mode;

/// Results that are not errors of the input: each has its own exit code.
#[derive(Debug)]
pub enum Failure {
    NoAdapter(String),
    Mismatch(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::NoAdapter(why) => write!(f, "adapter not available: {why}"),
            Failure::Mismatch(what) => write!(f, "output mismatch: {what}"),
        }
    }
}

impl std::error::Error for Failure {}

#[derive(Parser)]
#[command(
    name = "victory-pso",
    version,
    about = "Translate console shaders and pipeline state to D3D12 pipelines, without a title"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write the synthetic fixtures (our own code, in the translator's
    /// input format) and their pipeline descriptions into a folder.
    Fixture { dir: PathBuf },
    /// Wrap RDNA2 machine code of our own (an ELF object's `.text`, or raw
    /// code) as a shader container, as its program description says: for
    /// test shaders compiled by LLVM's AMD GPU back end.
    Wrap {
        /// The program description (TOML).
        description: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Translate a pipeline description's shaders and state offline: writes
    /// the DXIL and a manifest of the bindings into --out.
    Build {
        /// The pipeline description (TOML).
        description: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// The host 64-lane compute programs are translated for: `wave32`
        /// one whose widest wave has 32 lanes (NVIDIA), `native` one with
        /// 64-lane waves.
        #[arg(long, value_enum, default_value_t = Wave::Wave32)]
        wave64: Wave,
    },
    /// Create a build folder's root signature and pipeline state on an
    /// adapter; with --run, dispatch it and compare what it wrote.
    Verify {
        /// A folder `build` wrote.
        dir: PathBuf,
        #[cfg(windows)]
        #[arg(long, value_enum, default_value_t = adapter::Want::Nvidia)]
        adapter: adapter::Want,
        /// Run the compute pipeline with the description's [run] bindings.
        #[arg(long)]
        run: bool,
    },
    /// Time what the host's D3D12 driver does with draws, copies, waits,
    /// pipelines and resources of our own, and check the results.
    #[cfg(windows)]
    Bench(bench::Options),
}

#[derive(Clone, Copy, ValueEnum)]
enum Wave {
    Native,
    Wave32,
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Fixture { dir } => fixture::run(&dir),
        Command::Wrap { description, out } => wrap::run(&description, &out),
        Command::Build {
            description,
            out,
            wave64,
        } => {
            let mode = match wave64 {
                Wave::Native => Wave64Mode::Native,
                Wave::Wave32 => Wave64Mode::Wave32,
            };
            build::run(&description, &out, mode)
        }
        #[cfg(windows)]
        Command::Bench(options) => bench::run(&options),
        #[cfg(windows)]
        Command::Verify { dir, adapter, run } => verify::run(&dir, adapter, run),
        #[cfg(not(windows))]
        Command::Verify { .. } => anyhow::bail!("verify needs Windows: it creates D3D12 objects"),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(match e.downcast_ref::<Failure>() {
                Some(Failure::NoAdapter(_)) => 3,
                Some(Failure::Mismatch(_)) => 4,
                None => 1,
            })
        }
    }
}
