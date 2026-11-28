//! Installed engine builds and verified updates from the release manifest.
use super::{Args, Confirm, info};
use crate::{
    agent::{
        Backend, lock,
        schema::{Kind, opt},
    },
    core::storage,
    ops::{OpsSettings, updates},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

pub const TOOLS: &[super::Tool] = &[
    tool!(
        list_engine_builds,
        "List installed engine builds, including the active and previous builds.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        check_engine_update,
        "Check the release manifest for the configured engine channel and build kind.",
        &[
            opt(
                "channel",
                Kind::Choice(&["stable", "dev"]),
                "Release channel."
            ),
            opt(
                "kind",
                Kind::Choice(&["release", "diagnostics"]),
                "Engine build kind."
            )
        ],
        r#"{"channel":"stable","kind":"release"}"#,
        true,
        Confirm::Never
    ),
    tool!(
        install_engine_update,
        "Download and verify the engine found by check_engine_update, then make it active.",
        &[],
        "{}",
        false,
        Confirm::Always(install_preview)
    ),
    tool!(
        rollback_engine,
        "Select an installed engine build or return to the previous build.",
        &[opt(
            "build",
            Kind::Text,
            "Installed build name. Defaults to the previous build."
        )],
        "{}",
        false,
        Confirm::Always(|_, args| Ok(json!(format!(
            "Switch to {}.",
            args.text("build").unwrap_or("the previous engine build")
        ))))
    ),
    tool!(
        cleanup_engine_builds,
        "Remove superseded engine builds while keeping the active and rollback builds.",
        &[],
        "{}",
        false,
        Confirm::Always(|backend, _| Ok(json!({
            "remove": victory_update::superseded(&backend.dirs.engine_root(), updates::KEEP_BUILDS),
        })))
    ),
];

fn list_engine_builds(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let builds: Vec<_> = victory_update::installed(&backend.dirs.engine_root())
        .into_iter()
        .map(|build| {
            json!({
                "name": build.name, "active": build.is_current, "previous": build.is_previous,
                "size_bytes": build.bytes,
            })
        })
        .collect();
    Ok(json!({ "builds": builds }))
}

fn check_engine_update(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let cfg = OpsSettings::load_from(&backend.dirs.ops_file());
    let channel = args.text("channel").unwrap_or(&cfg.channel);
    let kind = args.text("kind").unwrap_or(&cfg.engine_kind);
    let have: Vec<_> = victory_update::installed(&backend.dirs.engine_root())
        .into_iter()
        .map(|build| build.name)
        .collect();
    let mut update_slot = lock(&backend.update);
    *update_slot = None;
    let update = updates::check_now(channel, kind, &have)?;
    let result = json!({
        "build": update.folder, "kind": update.engine.kind, "channel": channel,
        "translator": update.engine.translator, "size_bytes": update.engine.size,
        "notes": update.engine.notes, "newer": update.newer, "launcher_version": update.manifest.launcher_version,
    });
    *update_slot = Some(update);
    Ok(result)
}

fn install_preview(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let update = lock(&backend.update);
    let Some(update) = update.as_ref() else {
        return Ok(json!(
            "Download and activate the engine found by check_engine_update. Check for an update first."
        ));
    };
    Ok(json!({
        "install": update.folder,
        "download_bytes": update.engine.size,
        "make_active": true,
        "remove_superseded_builds": true,
    }))
}

fn install_engine_update(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let update = lock(&backend.update)
        .clone()
        .context("Run check_engine_update first.")?;
    backend.edit(|| {
        let activity = storage::lock(&backend.dirs.activity_lock())?;
        let job = backend.start_job("engine_install", move |backend, job| {
            let _activity = activity;
            let dirs = &backend.dirs;
            job.total.store(update.engine.size, Ordering::Relaxed);
            updates::install_now(
                &update.engine,
                &update.folder,
                &dirs.downloads_dir(),
                &dirs.engine_root(),
                &mut |done, total| {
                    job.done.store(done, Ordering::Relaxed);
                    job.total.store(total, Ordering::Relaxed);
                    !job.cancel.load(Ordering::Relaxed)
                },
            )?;
            Ok(format!("Installed and selected {}.", update.folder))
        })?;
        Ok(info::job_json(&job))
    })
}

fn rollback_engine(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        ensure!(
            !backend.job_running(),
            "Wait for the current launcher job to finish."
        );
        let _activity = storage::lock(&backend.dirs.activity_lock())?;
        let build = updates::rollback_now(&backend.dirs.engine_root(), args.text("build"))?;
        Ok(json!({ "active": build }))
    })
}

fn cleanup_engine_builds(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    backend.edit(|| {
        ensure!(
            !backend.job_running(),
            "Wait for the current launcher job to finish."
        );
        let _activity = storage::lock(&backend.dirs.activity_lock())?;
        Ok(json!({ "removed": updates::cleanup_now(&backend.dirs.engine_root())? }))
    })
}
