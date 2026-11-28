//! Shared save backup operations and verified storage moves.
use super::{Args, Confirm, info};
use crate::{
    agent::{
        Backend,
        schema::{Kind, opt, req},
    },
    core::{
        actions::{self, SaveAction, StorageAction},
        saves,
        storage::{self, Location},
    },
    i18n::tr,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
};

const ID: super::Param = req("id", Kind::Text, "Backup id from list_backups.");
const LOCATION: super::Param = req(
    "location",
    Kind::Choice(&["cache", "data"]),
    "Shader cache or saved game data.",
);
const PATH: super::Param = req("path", Kind::Text, "Local file or folder path.");

pub const TOOLS: &[super::Tool] = &[
    tool!(
        list_backups,
        "List backups with their labels, sizes and creation times.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        create_backup,
        "Take a manual backup of the current saves.",
        &[opt(
            "label",
            Kind::Text,
            "Optional label to keep with the backup."
        )],
        r#"{"label":"Before moving saves"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        verify_backup,
        "Check the files and hashes inside a backup.",
        &[ID],
        r#"{"id":"backup-id"}"#,
        true,
        Confirm::Never
    ),
    tool!(
        restore_backup,
        "Replace current saves with a backup. Makes a recovery backup first.",
        &[ID],
        r#"{"id":"backup-id"}"#,
        false,
        Confirm::Always(|_, args| Ok(json!(format!(
            "Replace current saves with backup {} and make a recovery backup first.",
            args.need("id")?
        ))))
    ),
    tool!(
        rename_backup,
        "Change the label of a save backup.",
        &[ID, req("label", Kind::Text, "New label.")],
        r#"{"id":"backup-id","label":"Chapter one"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        delete_backup,
        "Delete a save backup.",
        &[ID],
        r#"{"id":"backup-id"}"#,
        false,
        Confirm::Always(|_, args| Ok(json!(format!("Delete backup {}.", args.need("id")?))))
    ),
    tool!(
        import_backup,
        "Verify and import a backup ZIP.",
        &[PATH],
        r#"{"path":"D:/Backups/saves.zip"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        export_backup,
        "Copy a verified backup to a ZIP file.",
        &[ID, PATH],
        r#"{"id":"backup-id","path":"D:/Backups/saves.zip"}"#,
        false,
        Confirm::When(|_| Some(
            "Write the backup to the destination ZIP, replacing any existing file.".into()
        ))
    ),
    tool!(
        storage_sizes,
        "Measure game data, shader cache and backup folders and report any pending move.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        move_storage,
        "Start a verified background move of the cache or game data to an empty folder.",
        &[LOCATION, PATH],
        r#"{"location":"cache","path":"D:/VictoryCache"}"#,
        false,
        Confirm::Always(|_, args| Ok(json!(format!(
            "Copy the {} to {} and select it after verification.",
            args.need("location")?,
            args.need("path")?
        ))))
    ),
    tool!(
        resume_storage_move,
        "Resume an interrupted storage move.",
        &[],
        "{}",
        false,
        Confirm::Always(|_, _| Ok(json!(
            "Resume copying to the selected destination and switch to it after verification."
        )))
    ),
    tool!(
        finish_storage_move,
        "Close a completed move or cancel an incomplete move.",
        &[opt(
            "delete_old",
            Kind::Bool,
            "Remove the old copy after a completed move."
        )],
        r#"{"delete_old":false}"#,
        false,
        Confirm::Always(finish_preview)
    ),
    tool!(
        clear_cache,
        "Clear the shader and pipeline cache. The next run may need shader warm-up.",
        &[],
        "{}",
        false,
        Confirm::Always(|_, _| Ok(json!("Clear the shader and pipeline cache.")))
    ),
    tool!(
        clear_game_data,
        "Clear saves and game data after making a recovery backup.",
        &[],
        "{}",
        false,
        Confirm::Always(|_, _| Ok(json!(
            "Make a recovery backup, then clear saves and game data."
        )))
    ),
    tool!(
        job_status,
        "Read progress or the final result of the latest background launcher job.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        cancel_job,
        "Request cancellation of the running storage move or engine download.",
        &[],
        "{}",
        false,
        Confirm::Never
    ),
];

fn list_backups(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let root = backend.dirs.backups_root(&backend.dirs.settings());
    let backups: Vec<_> = saves::list(&root)?
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id, "label": s.manifest.label, "kind": s.manifest.kind.id(),
                "size_bytes": s.manifest.total_size, "files": s.manifest.files.len(),
                "created_unix_seconds": s.manifest.created, "engine_build": s.manifest.engine_build,
            })
        })
        .collect();
    Ok(json!({ "directory": root, "backups": backups }))
}

fn save(backend: &Backend, action: SaveAction) -> Result<Value> {
    backend.edit(|| {
        let dirs = &backend.dirs;
        let settings = dirs.settings();
        let root = dirs.backups_root(&settings);
        let game = actions::chosen_game(&settings);
        let engine = dirs.engine_exe(&settings);
        let message = actions::save_action(
            dirs,
            &root,
            game.as_ref().map(|g| g.folder.as_path()),
            engine.as_deref(),
            settings.backups.retention,
            action,
        )?;
        Ok(json!({ "message": tr(message) }))
    })
}

fn create_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(
        backend,
        SaveAction::Create(args.text("label").unwrap_or("").into()),
    )
}

fn verify_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(backend, SaveAction::Verify(args.need("id")?.into()))
}

fn restore_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(backend, SaveAction::Restore(args.need("id")?.into()))
}

fn rename_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let label = args
        .0
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or_default();
    save(
        backend,
        SaveAction::Rename(args.need("id")?.into(), label.into()),
    )
}

fn delete_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(backend, SaveAction::Delete(args.need("id")?.into()))
}

fn import_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(backend, SaveAction::Import(args.need("path")?.into()))
}

fn export_backup(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    save(
        backend,
        SaveAction::Export(args.need("id")?.into(), args.need("path")?.into()),
    )
}

fn label(location: Location) -> &'static str {
    match location {
        Location::Cache => "cache",
        Location::Data => "data",
    }
}

fn storage_sizes(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings = dirs.settings();
    let usage = |path: &Path| match storage::measure(path) {
        Ok(usage) => {
            json!({ "path": path, "size_bytes": usage.bytes, "free_bytes": usage.free, "network": usage.network })
        }
        Err(error) => json!({ "path": path, "error": format!("{error:#}") }),
    };
    let journal = storage::read_journal(&dirs.move_journal())?;
    Ok(json!({
        "cache": usage(&dirs.location(Location::Cache, &settings)),
        "data": usage(&dirs.location(Location::Data, &settings)),
        "backups": usage(&dirs.backups_root(&settings)),
        "move": journal.map(|j| json!({ "location": label(j.location), "source": j.source, "destination": j.destination, "phase": j.phase })),
    }))
}

fn start_move(backend: &Arc<Backend>, action: StorageAction) -> Result<Value> {
    backend.edit(|| {
        ensure!(
            !backend.runner().busy(),
            "Close the game before moving storage."
        );
        let job = backend.start_job("storage_move", |backend, job| {
            let dirs = &backend.dirs;
            let settings = dirs.settings();
            let game = actions::chosen_game(&settings);
            let build = actions::build_name(dirs.engine_exe(&settings).as_deref());
            let message = action.done_message();
            actions::storage_action(
                dirs,
                game.as_ref().map(|g| g.folder.as_path()),
                &build,
                action,
                &job.cancel,
                |done, total| {
                    job.done.store(done, Ordering::Relaxed);
                    job.total.store(total, Ordering::Relaxed);
                },
            )?;
            Ok(tr(message).into())
        })?;
        Ok(info::job_json(&job))
    })
}

fn move_storage(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let location = if args.text("location") == Some("cache") {
        Location::Cache
    } else {
        Location::Data
    };
    let target = args.need("path")?.into();
    start_move(backend, StorageAction::Move(location, target))
}

fn resume_storage_move(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    start_move(backend, StorageAction::Resume)
}

fn finish_preview(_: &Arc<Backend>, args: &Args) -> Result<Value> {
    Ok(json!(if args.yes("delete_old") {
        "Finish the move and delete the old copy if the move completed."
    } else {
        "Close the move, keeping the old copy after a completed move or removing the partial copy after an incomplete move."
    }))
}

fn storage_change(backend: &Backend, action: StorageAction) -> Result<Value> {
    backend.edit(|| {
        let dirs = &backend.dirs;
        let settings = dirs.settings();
        let game = actions::chosen_game(&settings);
        let build = actions::build_name(dirs.engine_exe(&settings).as_deref());
        let message = action.done_message();
        actions::storage_action(
            dirs,
            game.as_ref().map(|g| g.folder.as_path()),
            &build,
            action,
            &Default::default(),
            |_, _| {},
        )?;
        Ok(json!({ "message": tr(message) }))
    })
}

fn finish_storage_move(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    storage_change(backend, StorageAction::Finish(args.yes("delete_old")))
}

fn clear_cache(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    storage_change(backend, StorageAction::Clear(Location::Cache))
}

fn clear_game_data(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    storage_change(backend, StorageAction::Clear(Location::Data))
}

fn job_status(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    Ok(json!({ "job": backend.job().map(|job| info::job_json(&job)) }))
}

fn cancel_job(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let job = backend.job().filter(|job| !job.finished.is_set());
    if let Some(job) = &job {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(json!({ "cancel_requested": job.is_some() }))
}
