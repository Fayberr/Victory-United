//! Status, hardware, finding the game and ending a headless launcher.
use super::{Args, Confirm, Tool};
use crate::{
    agent::{Backend, Job},
    core::{
        actions, hw, launch,
        presets::builtin,
        runner::{self, RunState},
        store::Settings,
    },
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
};

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "status",
        description: "Launcher version, engine build, game folder, whether the game runs, active profile and where the files are. Start here.",
        params: &[],
        example: "{}",
        read_only: true,
        confirm: Confirm::Never,
        run: status,
    },
    Tool {
        name: "hardware",
        description: "Graphics card, driver, memory, processor, Direct3D capabilities and warnings. The first call looks at the graphics card.",
        params: &[],
        example: "{}",
        read_only: true,
        confirm: Confirm::Never,
        run: hardware,
    },
    Tool {
        name: "shutdown",
        description: "Ends a launcher that runs without a window (--headless or --mcp-stdio). A running game keeps running, but nothing backs up the saves after it.",
        params: &[],
        example: "{}",
        read_only: false,
        confirm: Confirm::Always(shutdown_preview),
        run: shutdown,
    },
];

pub fn path(p: &Path) -> Value {
    json!(p.to_string_lossy())
}

/// The state of the warm-up and the game.
pub fn describe(runner: &runner::Runner) -> Value {
    let log = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let mut state = match &runner.state {
        RunState::Idle => json!({ "state": "idle" }),
        RunState::Warming { done, total, since } => json!({
            "state": "warming_up",
            "done_bytes": done,
            "total_bytes": total,
            "elapsed_seconds": since.elapsed().as_secs(),
            "remaining_seconds": runner::remaining(*done, *total, since.elapsed()).map(|d| d.as_secs()),
        }),
        RunState::Starting => json!({ "state": "starting" }),
        RunState::Running {
            pid,
            since,
            log: file,
        } => json!({
            "state": "running", "pid": pid, "running_seconds": since.elapsed().as_secs(), "log": log(file),
        }),
        RunState::Finishing { log: file } => json!({ "state": "finishing", "log": log(file) }),
        RunState::Ended(s) => json!({
            "state": "ended",
            "exit_code": s.code,
            "ran_seconds": s.secs,
            "log": s.log.as_deref().and_then(log),
            "error": s.error,
            "stopped_by_request": s.cancelled,
            "quit_early_with_extra_flags": s.quick_exit_with_flags,
        }),
    };
    if let Some(warning) = &runner.backup_warning {
        state["backup_warning"] = json!(warning);
    }
    state
}

pub fn job_json(job: &Job) -> Value {
    let result = job
        .result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let mut out = json!({
        "kind": job.kind,
        "running": !job.finished.is_set(),
        "done_bytes": job.done.load(Ordering::Relaxed),
        "total_bytes": job.total.load(Ordering::Relaxed),
    });
    match result {
        Some(Ok(message)) => out["result"] = json!(message),
        Some(Err(error)) => out["error"] = json!(error),
        None => {}
    }
    out
}

fn status(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings: Settings = dirs.settings();
    let profile = actions::active_profile(dirs, &settings);
    let engine = dirs.engine_exe(&settings);
    let own = actions::own_engine(&settings).is_some();
    let runner = backend.runner();
    let build = Some(actions::build_name(engine.as_deref())).filter(|b| !b.is_empty());
    let extra = launch::read_extra(&dirs.launch_flags_file())
        .iter()
        .filter(|f| f.enabled)
        .count();
    Ok(json!({
        "launcher_version": env!("CARGO_PKG_VERSION"),
        "mode": if backend.headless { "headless" } else { "window" },
        "engine": {
            "build": build,
            "executable": engine.as_deref().map(path),
            "source": match (&engine, own) { (None, _) => "none", (_, true) => "own build", _ => "installed" },
        },
        "game": actions::chosen_game(&settings).map(|g| json!({ "title": g.title, "folder": path(&g.folder), "executable": path(&g.exe) })),
        "game_running": runner.running_pid().is_some(),
        "run": describe(&runner),
        "active_profile": profile.name,
        "preset": profile.options.preset,
        "console": profile.options.console.arg(),
        "extra_flags_on": extra,
        "job": backend.job().map(|j| job_json(&j)),
        "paths": {
            "data": path(&dirs.data),
            "settings": path(&dirs.settings_file()),
            "profiles": path(&dirs.profiles_dir()),
            "logs": path(&dirs.logs_dir()),
            "reports": path(&dirs.reports_dir()),
            "engine": path(&dirs.engine_root()),
            "game_data": path(&dirs.location(crate::core::storage::Location::Data, &settings)),
            "cache": path(&dirs.location(crate::core::storage::Location::Cache, &settings)),
            "backups": path(&dirs.backups_root(&settings)),
        },
    }))
}

fn hardware(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let hardware = backend.hardware();
    let warnings: Vec<Value> = hw::warnings(hardware)
        .into_iter()
        .map(|w| json!({ "severity": if w.severity == hw::Severity::Blocker { "blocker" } else { "warning" }, "text": w.text }))
        .collect();
    let capabilities: serde_json::Map<String, Value> = hardware
        .capabilities(None)
        .into_iter()
        .map(|(key, value)| (crate::i18n::tr(key).to_string(), json!(value)))
        .collect();
    let auto = builtin().auto_for(&hardware.gpu_name, hardware.vendor_id);
    Ok(json!({
        "gpu": hardware.gpu_name,
        "vram_mb": hardware.vram_mb,
        "driver": hardware.driver,
        "mesh_shaders": hardware.mesh_shaders,
        "ram_mb": hardware.ram_mb,
        "cpu": hardware.cpu_name,
        "cpu_cores": hardware.cpu_cores,
        "cpu_threads": hardware.cpu_threads,
        "windows_build": hardware.windows_build,
        "capabilities": capabilities,
        "warnings": warnings,
        "automatic_preset": auto.id,
        "note": hardware.note,
    }))
}

fn headless_only(backend: &Backend) -> Result<()> {
    ensure!(
        backend.headless,
        "This launcher has a window. Close the window, or turn agent control off in its Settings."
    );
    Ok(())
}

fn shutdown_preview(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    headless_only(backend)?;
    let mut would = vec!["End this launcher and its agent endpoint.".to_string()];
    if backend.runner().busy() {
        would.push("The game keeps running. Its saves are not backed up after it ends, and a warm-up is not followed by a start.".into());
    }
    if backend.job_running() {
        would.push("The running storage or engine job is paused or cancelled first.".into());
    }
    Ok(json!(would))
}

fn shutdown(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    headless_only(backend)?;
    backend.shutdown_requested.store(true, Ordering::SeqCst);
    Ok(json!({ "shutting_down": true }))
}
