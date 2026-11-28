//! Read the engine's latest shared statistics and configure its launch-time profiler.
use super::{Args, Confirm};
use crate::{
    agent::{
        Backend, lock,
        schema::{Kind, opt, req},
    },
    core::{actions, flagreg::registry, launch, storage},
    ops::{monitor, profile, settings::OpsSettings},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Instant};

const SECONDS: super::Param = opt(
    "seconds",
    Kind::Int(1, 120),
    "History window in seconds. Defaults to 30.",
);

pub const TOOLS: &[super::Tool] = &[
    tool!(
        monitor_snapshot,
        "Read frame rate, timing, memory and counters from the running engine.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        monitor_history,
        "Read available frame samples from the last N seconds.",
        &[SECONDS],
        r#"{"seconds":15}"#,
        true,
        Confirm::Never
    ),
    tool!(
        export_monitor_csv,
        "Export the available frame samples to a local CSV.",
        &[opt(
            "seconds",
            Kind::Int(10, 120),
            "History window in seconds. Defaults to 30."
        )],
        r#"{"seconds":30}"#,
        false,
        Confirm::Never
    ),
    tool!(
        get_overlay,
        "Read whether the in-game performance panel will be enabled for the next launch.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        set_overlay,
        "Enable or disable the in-game performance panel on the next launch.",
        &[req(
            "enabled",
            Kind::Bool,
            "Enable the in-game performance panel."
        )],
        r#"{"enabled":true}"#,
        false,
        Confirm::Never
    ),
    tool!(
        profiler_status,
        "Read the sampling schedule for the next launch.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        start_profiler,
        "Enable scheduled profile sampling on the next launch.",
        &[
            opt(
                "windows",
                Kind::Windows,
                "Sampling windows in seconds after the engine starts."
            ),
            opt(
                "sample_ms",
                Kind::Int(1, 50),
                "Milliseconds between samples."
            ),
            opt("threads", Kind::Int(4, 28), "Maximum threads to sample."),
            opt("depth", Kind::Int(1, 40), "Maximum stack depth."),
            opt(
                "filter",
                Kind::Text,
                "Match thread names containing this text."
            ),
            opt(
                "symbolize",
                Kind::Bool,
                "Resolve module and function names."
            ),
            opt(
                "folder",
                Kind::Text,
                "Existing output folder. Omit to keep the current folder."
            )
        ],
        r#"{"windows":[{"from":20,"to":40}]}"#,
        false,
        Confirm::When(|args| args
            .0
            .contains_key("folder")
            .then(|| "Change where the engine writes profiler output.".into()))
    ),
    tool!(
        stop_profiler,
        "Disable sampling for future launches. A running engine keeps its existing schedule.",
        &[],
        "{}",
        false,
        Confirm::Never
    ),
    tool!(
        profiler_overview,
        "Read the most recent capture or a selected file in the configured profiler folder.",
        &[
            opt(
                "name",
                Kind::Text,
                "Capture file name. Defaults to the newest capture."
            ),
            opt(
                "group",
                Kind::Choice(&["function", "module", "thread"]),
                "Group the rows by function, module or thread."
            ),
            opt("top", Kind::Int(1, 250), "Maximum rows."),
            opt(
                "thread",
                Kind::Int(0, u32::MAX as i64),
                "Only include this thread id."
            ),
            opt("search", Kind::Text, "Match row names.")
        ],
        r#"{"group":"function","top":15}"#,
        true,
        Confirm::Never
    ),
];

fn with_monitor<T>(
    backend: &Backend,
    get: impl FnOnce(&monitor::MonitorState, Option<u32>) -> T,
) -> T {
    let pid = backend.runner().running_pid();
    let cfg = OpsSettings::load_from(&backend.dirs.ops_file());
    let mut monitor = lock(&backend.monitor);
    if let Some(pid) = pid {
        monitor.refresh(pid, Instant::now(), cfg.monitor.rate.duration());
    } else {
        monitor.forget();
    }
    get(&monitor, pid)
}

fn monitor_snapshot(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    Ok(with_monitor(backend, |monitor, pid| {
        json!({
            "pid": pid, "available": monitor.summary.is_some(),
            "sample": monitor.summary.as_ref().map(monitor::Summary::snapshot_json),
        })
    }))
}

fn monitor_history(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let seconds = args.int("seconds").unwrap_or(30) as u32;
    Ok(with_monitor(backend, |monitor, pid| {
        let rows: Vec<_> = monitor::recent(&monitor.history, seconds).collect();
        json!({ "pid": pid, "requested_seconds": seconds, "frames": rows })
    }))
}

fn export_monitor_csv(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let seconds = args.int("seconds").unwrap_or(30) as u32;
    with_monitor(backend, |monitor, _| {
        ensure!(
            !monitor.history.is_empty(),
            "There are no frame samples to export."
        );
        let path = monitor::export_to(&backend.dirs.reports_dir(), &monitor.history, seconds)?;
        Ok(json!({ "path": path }))
    })
}

fn get_overlay(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings = dirs.settings();
    let profile = actions::active_profile(dirs, &settings);
    let extra = launch::read_extra(&dirs.launch_flags_file());
    let hw = backend.has_hardware().then(|| backend.hardware());
    let (resolved, _) = actions::resolve(&profile, hw, &extra);
    let ops = OpsSettings::load_from(&dirs.ops_file());
    let chosen = matches!(
        resolved.source.get("VICTORY_PANEL"),
        Some(launch::Source::Edit | launch::Source::Extra)
    ) || profile.removed.iter().any(|name| name == "VICTORY_PANEL");
    let profile_value = resolved.env.get("VICTORY_PANEL").map(String::as_str);
    let value = if chosen {
        profile_value
    } else {
        crate::ops::settings::panel_flag(&ops).or(profile_value)
    };
    Ok(json!({ "enabled_on_launch": value.is_some_and(|value| value != "0" && !value.is_empty()) }))
}

fn set_overlay(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let dirs = &backend.dirs;
        let mut profile = actions::active_profile(dirs, &dirs.settings());
        let panel = registry()
            .get("VICTORY_PANEL")
            .context("The in-game performance panel is unavailable.")?;
        profile.set_switch(panel, args.yes("enabled"));
        launch::save_profile(&dirs.profiles_dir(), &profile)?;
        get_overlay(backend, args)
    })
}

fn profiler_status(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let cfg = OpsSettings::load_from(&backend.dirs.ops_file());
    Ok(json!({ "next_launch": cfg.profiler }))
}

fn start_profiler(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let file = backend.dirs.ops_file();
        let mut cfg = OpsSettings::load_from(&file);
        let profile = &mut cfg.profiler;
        profile.enabled = true;
        if let Some(windows) = args.0.get("windows") {
            profile.windows = serde_json::from_value(windows.clone())?;
        }
        if let Some(ms) = args.int("sample_ms") {
            profile.sample_ms = ms as u32;
        }
        if let Some(threads) = args.int("threads") {
            profile.threads = threads as u32;
        }
        if let Some(depth) = args.int("depth") {
            profile.depth = depth as u32;
        }
        if let Some(filter) = args.0.get("filter").and_then(Value::as_str) {
            profile.filter = filter.into();
        }
        if let Some(symbolize) = args.flag("symbolize") {
            profile.symbolize = symbolize;
        }
        if args.0.contains_key("folder") {
            let folder = Path::new(args.need("folder")?);
            ensure!(folder.is_dir(), "Choose an existing profiler directory.");
            profile.folder = storage::absolute(folder)?.to_string_lossy().into_owned();
        }
        profile::launch_flags(profile, &backend.dirs.data, 0)?;
        cfg.save_to(&file)?;
        Ok(json!({ "next_launch": cfg.profiler }))
    })
}

fn stop_profiler(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    backend.edit(|| {
        let file = backend.dirs.ops_file();
        let mut cfg = OpsSettings::load_from(&file);
        cfg.profiler.enabled = false;
        cfg.save_to(&file)?;
        Ok(json!({ "next_launch": cfg.profiler }))
    })
}

fn profiler_overview(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let cfg = OpsSettings::load_from(&backend.dirs.ops_file());
    let folder = cfg.profiler.folder(&backend.dirs.data);
    let path = if let Some(name) = args.text("name") {
        ensure!(
            !name.contains(['/', '\\', ':'])
                && name.starts_with("profile-")
                && name.ends_with(".txt"),
            "Use a profile file name in the configured profiler directory."
        );
        folder.join(name)
    } else {
        profile::latest(&folder)?
    };
    ensure!(
        storage::within(&storage::absolute(&path)?, &storage::absolute(&folder)?),
        "Choose a file inside the profiler directory."
    );
    let data = profile::open(&path)?;
    let mut view = cfg.profile_view;
    if let Some(group) = args.0.get("group") {
        view.group = serde_json::from_value(group.clone())?;
    }
    if let Some(top) = args.int("top") {
        view.top = top as usize;
    }
    let rows: Vec<_> = profile::overview(&data, &view, args.int("thread").map(|n| n as u32), args.text("search").unwrap_or(""))?
        .into_iter().map(|row| json!({
            "name": row.name, "self_samples": row.self_samples, "inclusive_samples": row.inclusive_samples,
            "self_percent": row.self_percent, "inclusive_percent": row.inclusive_percent,
        })).collect();
    Ok(json!({ "path": path, "rounds": data.rounds, "rows": rows }))
}
