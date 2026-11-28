//! Launch preview and game control, run logs and bug reports.
use super::{Args, Confirm, info};
use crate::{
    agent::{
        Backend, redact,
        schema::{Kind, opt},
    },
    core::{
        actions::{self, LaunchInput},
        launch, report,
        runner::RunState,
        storage::Location,
    },
    ops::{self, OpsSettings},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;

pub const TOOLS: &[super::Tool] = &[
    tool!(
        launch_preview,
        "Show the command, paths and environment the launcher would use.",
        &[opt(
            "without_extra_flags",
            Kind::Bool,
            "Leave the extra flags out for this launch."
        )],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        play,
        "Start the configured game, including shader warm-up when needed.",
        &[
            opt(
                "without_extra_flags",
                Kind::Bool,
                "Leave the extra flags out for this launch."
            ),
            opt(
                "start_anyway",
                Kind::Bool,
                "Acknowledge missing mesh shader support."
            )
        ],
        "{}",
        false,
        Confirm::Never
    ),
    tool!(
        stop,
        "Stop the game or shader warm-up and its child processes.",
        &[],
        "{}",
        false,
        Confirm::Always(|_, _| Ok(json!(
            "Stop the game or shader warm-up. Unsaved progress may be lost."
        )))
    ),
    tool!(
        warmup_status,
        "Show shader warm-up progress and the current run state.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        skip_warmup,
        "Stop translating shaders and continue starting the game.",
        &[],
        "{}",
        false,
        Confirm::Always(|_, _| Ok(json!(
            "Skip the current shader warm-up and continue to the game."
        )))
    ),
    tool!(
        log_tail,
        "Read the tail of a game or warm-up log. Optionally filter the returned lines.",
        &[
            opt(
                "name",
                Kind::Text,
                "Log file name. Defaults to the current or most recent game log."
            ),
            opt(
                "lines",
                Kind::Int(1, 500),
                "Maximum lines to read before filtering. Defaults to 100."
            ),
            opt(
                "search",
                Kind::Text,
                "Keep lines containing this text, ignoring case."
            )
        ],
        r#"{"lines":50,"search":"error"}"#,
        true,
        Confirm::Never
    ),
    tool!(
        make_bug_report,
        "Create an anonymized bug report ZIP in the local reports folder.",
        &[opt(
            "items",
            Kind::TextList,
            "Include only these item names: log-tail.txt, flags.txt, versions.txt, hardware.txt, settings.txt."
        )],
        "{}",
        false,
        Confirm::Never
    ),
];

fn launch_preview(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings = dirs.settings();
    let profile = actions::active_profile(dirs, &settings);
    let game = actions::chosen_game(&settings);
    let extra = extra_for(backend, args);
    let hardware = backend.hardware();
    let engine = dirs.engine_exe(&settings);
    let input = LaunchInput {
        settings: &settings,
        profile: &profile,
        extra: &extra,
        hardware: Some(hardware),
        game: game.as_ref(),
        engine,
    };
    let (resolved, preset) = actions::resolve(&profile, Some(hardware), &extra);
    let mut plan = input.plan(
        &dirs.location(Location::Cache, &settings),
        &dirs.location(Location::Data, &settings),
    );
    if let Some(plan) = &mut plan {
        let cfg = OpsSettings::load_from(&dirs.ops_file());
        let flags = ops::settings::preview_engine_options(&cfg, &dirs.data)?;
        actions::merge_ops_flags(plan, &profile, &resolved, flags);
    }
    let environment = plan.as_ref().map_or(&resolved.env, |plan| &plan.env);
    Ok(json!({
        "profile": profile.name,
        "preset": preset,
        "ready": plan.is_some(),
        "game_selected": game.is_some(),
        "engine_available": input.engine.is_some(),
        "mesh_shaders": hardware.mesh_shaders,
        "command": plan.as_ref().map(|plan| plan.command_line()),
        "cache_dir": plan.as_ref().map(|plan| &plan.cache_dir),
        "data_dir": plan.as_ref().map(|plan| &plan.data_dir),
        "environment": environment,
    }))
}

fn extra_for(backend: &Backend, args: &Args) -> Vec<launch::ExtraFlag> {
    if args.yes("without_extra_flags") {
        Vec::new()
    } else {
        launch::read_extra(&backend.dirs.launch_flags_file())
    }
}

fn play(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let hardware = backend.hardware();
    backend.edit(|| {
        ensure!(
            !backend.job_running(),
            "Wait for the current launcher job to finish."
        );
        let dirs = &backend.dirs;
        let settings = dirs.settings();
        let profile = actions::active_profile(dirs, &settings);
        let game = actions::chosen_game(&settings);
        let extra = extra_for(backend, args);
        let ops = OpsSettings::load_from(&dirs.ops_file());
        let input = LaunchInput {
            settings: &settings,
            profile: &profile,
            extra: &extra,
            hardware: Some(hardware),
            game: game.as_ref(),
            engine: dirs.engine_exe(&settings),
        };
        let mut runner = backend.runner();
        actions::start_game(&mut runner, dirs, &input, &ops, args.yes("start_anyway"))?;
        Ok(info::describe(&runner))
    })
}

fn stop(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    backend.edit(|| {
        let runner = backend.runner();
        if !runner.busy() || matches!(runner.state, RunState::Finishing { .. }) {
            return Ok(json!({ "stop_requested": false, "run": info::describe(&runner) }));
        }
        runner.stop()?;
        Ok(json!({ "stop_requested": true, "run": info::describe(&runner) }))
    })
}

fn warmup_status(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    Ok(info::describe(&backend.runner()))
}

fn skip_warmup(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    backend.edit(|| {
        let runner = backend.runner();
        let warming = matches!(runner.state, RunState::Warming { .. });
        if warming {
            runner.skip_warmup()?;
        }
        Ok(json!({ "skip_requested": warming }))
    })
}

fn log_tail(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let logs = backend.dirs.logs_dir();
    let file = match args.text("name") {
        Some(name) => actions::log_by_name(&logs, name)?,
        None => actions::log_for_report(backend.runner().log_file(), &logs)
            .context("There is no run log yet.")?,
    };
    let tail = report::tail_lines(&file, args.int("lines").unwrap_or(100) as usize)?;
    let tail = redact::text_of(&tail, backend.token());
    let search = args.text("search").map(str::to_lowercase);
    let lines: Vec<_> = tail
        .lines()
        .filter(|line| {
            search
                .as_ref()
                .is_none_or(|s| line.to_lowercase().contains(s))
        })
        .collect();
    Ok(json!({ "name": file.file_name().map(|s| s.to_string_lossy()), "lines": lines }))
}

fn make_bug_report(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings = dirs.settings();
    let profile = actions::active_profile(dirs, &settings);
    let game = actions::chosen_game(&settings);
    let extra = launch::read_extra(&dirs.launch_flags_file());
    let hw = backend.has_hardware().then(|| backend.hardware());
    let (resolved, preset) = actions::resolve(&profile, hw, &extra);
    let (run, log) = {
        let runner = backend.runner();
        (
            runner.state.clone(),
            actions::log_for_report(runner.log_file(), &dirs.logs_dir()),
        )
    };
    let input = report::Inputs {
        settings: &settings,
        profile: &profile,
        extra: &extra,
        resolved: &resolved,
        preset: &preset,
        hardware: hw,
        rendering_adapter: None,
        game: game.as_ref(),
        engine: dirs.engine_exe(&settings),
        run: &run,
        log,
    };
    let identity = report::Identity::current();
    let mut items = report::build_items(&input, &identity);
    if let Some(names) = args.list("items") {
        ensure!(
            !names.is_empty()
                && names
                    .iter()
                    .all(|name| items.iter().any(|item| item.file == *name)),
            "Select one or more known report items."
        );
        for item in &mut items {
            item.include = names.contains(&item.file);
        }
    }
    let token = backend.token();
    for item in &mut items {
        item.text = redact::text_of(&item.text, token);
    }
    backend.edit(|| {
        let path = report::write_zip(&dirs.reports_dir(), &items)?;
        Ok(json!({
            "path": path,
            "items": items.iter().filter(|item| item.include).map(|item| item.file).collect::<Vec<_>>(),
            "issue_text": report::issue_text(&input, &identity, Some(&path)),
        }))
    })
}
