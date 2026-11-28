//! Launcher settings, game selection, presets, saved profiles and launch flags.
use super::{Args, Confirm};
use crate::{
    agent::{
        Backend,
        schema::{Kind, opt, req},
    },
    core::{
        actions, engine,
        flagreg::{self, registry},
        game, launch,
        presets::builtin,
        storage,
        store::{self, Settings},
    },
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Map, Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

const NAME: super::Param = req("name", Kind::Text, "Profile name.");
const FLAG: super::Param = req("name", Kind::Text, "Name of the launch flag.");

pub const TOOLS: &[super::Tool] = &[
    tool!(
        get_settings,
        "Read launcher settings and the active profile.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        set_settings,
        "Change launcher settings. Supported keys: appearance, behavior, update_schedule, engine_override, backups, storage.verify_cache_by_hash and dirs.",
        &[req(
            "launcher",
            Kind::Object,
            "Settings to change. Use null to clear engine_override, backups.root or an entry in dirs."
        )],
        r#"{"launcher":{"appearance":{"theme":"dark"}}}"#,
        false,
        Confirm::When(confirm_settings)
    ),
    tool!(
        set_game_folder,
        "Choose the game folder using MicrosoftGame.Config.",
        &[req(
            "path",
            Kind::Text,
            "Existing game folder or its MicrosoftGame.Config."
        )],
        r#"{"path":"D:/Games/Game/Content"}"#,
        false,
        Confirm::When(|_| Some("Change the selected game folder.".into()))
    ),
    tool!(
        list_presets,
        "List launch presets and the automatic choice.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        apply_preset,
        "Set the active profile's preset.",
        &[req(
            "preset",
            Kind::Text,
            "Preset id from list_presets, or auto."
        )],
        r#"{"preset":"auto"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        list_profiles,
        "List profiles with their settings and mark the active profile.",
        &[],
        "{}",
        true,
        Confirm::Never
    ),
    tool!(
        edit_profile,
        "Edit a profile's console, safe mode, toggles or upscaler settings.",
        &[
            opt(
                "name",
                Kind::Text,
                "Profile to edit. Defaults to the active one."
            ),
            req(
                "options",
                Kind::Object,
                "Options to change: console, safe, toggles, upscaler, upscaler_mode, frame_generation."
            )
        ],
        r#"{"options":{"console":"series-x","safe":true}}"#,
        false,
        Confirm::Never
    ),
    tool!(
        select_profile,
        "Switch to a saved profile.",
        &[NAME],
        r#"{"name":"Default"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        create_profile,
        "Create a new profile and select it. Optionally copy an existing profile.",
        &[
            NAME,
            opt("copy_from", Kind::Text, "Name of the profile to copy.")
        ],
        r#"{"name":"Testing"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        rename_profile,
        "Rename a profile to an unused name.",
        &[NAME, req("new_name", Kind::Text, "New profile name.")],
        r#"{"name":"Testing","new_name":"Baseline"}"#,
        false,
        Confirm::When(|_| Some("Rename the saved profile.".into()))
    ),
    tool!(
        delete_profile,
        "Delete a saved profile.",
        &[NAME],
        r#"{"name":"Testing"}"#,
        false,
        Confirm::Always(|_, args| Ok(json!(format!("Delete profile {}.", args.need("name")?))))
    ),
    tool!(
        import_profile,
        "Read a TOML profile into a new named profile.",
        &[NAME, req("path", Kind::Text, "Profile TOML file.")],
        r#"{"name":"Imported","path":"D:/Exports/profile.toml"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        export_profile,
        "Save a copy of a profile to a new TOML file.",
        &[NAME, req("path", Kind::Text, "New file to write.")],
        r#"{"name":"Default","path":"D:/Exports/profile.toml"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        list_flags,
        "List flag descriptions, risks and types, plus extra flags with their enabled state.",
        &[
            opt("search", Kind::Text, "Match by name, group or description."),
            opt(
                "offset",
                Kind::Int(0, 10000),
                "Skip this many matching flags."
            ),
            opt(
                "limit",
                Kind::Int(1, 200),
                "Return up to this many flags. Defaults to 50."
            )
        ],
        r#"{"search":"threads"}"#,
        true,
        Confirm::Never
    ),
    tool!(
        set_flag,
        "Set an extra flag or an override in the active profile.",
        &[
            FLAG,
            opt(
                "value",
                Kind::Text,
                "Value to set. Omit for an extra flag's default."
            ),
            opt(
                "scope",
                Kind::Choice(&["extra", "profile"]),
                "Layer to edit. Defaults to extra."
            ),
            opt(
                "enabled",
                Kind::Bool,
                "Enable or disable the flag. Defaults to true."
            )
        ],
        r#"{"name":"VICTORY_THIN_THREADS","value":"8"}"#,
        false,
        Confirm::Never
    ),
    tool!(
        clear_flag,
        "Remove an extra flag or return a profile override to its preset value.",
        &[
            FLAG,
            opt(
                "scope",
                Kind::Choice(&["extra", "profile"]),
                "Layer to edit. Defaults to extra."
            )
        ],
        r#"{"name":"VICTORY_THIN_THREADS"}"#,
        false,
        Confirm::Never
    ),
];

fn get_settings(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let mut settings = backend.dirs.settings();
    settings.engine_override = actions::own_engine(&settings);
    Ok(
        json!({ "launcher": settings, "profile": actions::active_profile(&backend.dirs, &settings) }),
    )
}

fn confirm_settings(args: &Args) -> Option<String> {
    let patch = args.object("launcher")?;
    let changes_path = patch.contains_key("engine_override")
        || patch.contains_key("dirs")
        || patch
            .get("backups")
            .is_some_and(|backup| backup.get("root").is_some())
        || patch
            .get("storage")
            .is_some_and(|storage| storage.get("data").is_some() || storage.get("cache").is_some());
    changes_path.then(|| "Change which engine or directories the launcher uses.".into())
}

fn restrict(patch: &Map<String, Value>, keys: &[&str]) -> Result<()> {
    for key in patch.keys() {
        ensure!(keys.contains(&key.as_str()), "Unknown setting \"{key}\".");
    }
    Ok(())
}

fn patch_values(target: &mut Value, patch: &Map<String, Value>) -> Result<()> {
    let object = target.as_object_mut().context("Expected an object.")?;
    for (key, value) in patch {
        let current = object
            .get_mut(key)
            .with_context(|| format!("Unknown setting \"{key}\"."))?;
        if let Some(children) = current.as_object() {
            let entries = value
                .as_object()
                .with_context(|| format!("\"{key}\" needs an object."))?;
            // Toggles have variable keys. Their names are checked against the preset file.
            if key == "toggles" {
                let mut toggles = children.clone();
                toggles.extend(entries.clone());
                *current = Value::Object(toggles);
            } else {
                patch_values(current, entries)?;
            }
        } else {
            *current = value.clone();
        }
    }
    Ok(())
}

fn directory(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_dir(),
        "{} is not an existing directory.",
        path.display()
    );
    Ok(storage::absolute(path)?)
}

fn set_settings(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let mut patch = args
        .object("launcher")
        .context("launcher needs an object.")?
        .clone();
    restrict(
        &patch,
        &[
            "appearance",
            "behavior",
            "update_schedule",
            "engine_override",
            "backups",
            "storage",
            "dirs",
        ],
    )?;
    if let Some(storage) = patch.get("storage") {
        let storage = storage.as_object().context("storage needs an object.")?;
        ensure!(
            !storage.contains_key("cache") && !storage.contains_key("data"),
            "Use move_storage to move cache or game data."
        );
    }
    let custom_dirs = patch.remove("dirs");
    let dirs = &backend.dirs;
    backend.edit(|| {
        let _activity = (confirm_settings(args).is_some())
            .then(|| storage::lock(&dirs.activity_lock()))
            .transpose()?;
        let updated = store::edit_settings(&dirs.settings_file(), |current| {
            let mut merged = serde_json::to_value(&*current)?;
            patch_values(&mut merged, &patch)?;
            let mut settings: Settings =
                serde_json::from_value(merged).context("Invalid launcher settings")?;
            ensure!(
                (80..=150).contains(&settings.appearance.scale_percent),
                "Interface scale must be between 80 and 150."
            );
            let policy = settings.backups.retention;
            ensure!(
                policy.keep_recent <= 1000 && policy.keep_days <= 3650 && policy.max_gib <= 10_000,
                "Backup retention is outside the supported range."
            );
            if patch.contains_key("engine_override") {
                if let Some(path) = &mut settings.engine_override {
                    engine::check(path).map_err(|problem| anyhow!(problem.text()))?;
                    *path = std::path::absolute(&*path)?;
                }
            }
            if patch
                .get("backups")
                .is_some_and(|backup| backup.get("root").is_some())
            {
                let settings_file = dirs.settings_file();
                let engine = dirs.engine_root();
                let game = actions::chosen_game(&settings);
                let context = storage::MoveContext {
                    settings: &settings_file,
                    engine: &engine,
                    game: game.as_ref().map(|g| g.folder.as_path()),
                };
                let root = dirs.backups_root(&settings);
                if settings.backups.root.is_some() {
                    settings.backups.root = Some(storage::validate_backup_root(
                        &directory(&root)?,
                        &context,
                        &settings,
                    )?);
                } else {
                    storage::validate_backup_root(&root, &context, &settings)?;
                }
            }
            if let Some(custom) = &custom_dirs {
                let custom = custom.as_object().context("dirs needs an object.")?;
                let file = builtin();
                for (key, value) in custom {
                    ensure!(
                        file.upscaler
                            .iter()
                            .any(|upscaler| upscaler.files_env.as_deref() == Some(key)),
                        "Unknown directory setting \"{key}\"."
                    );
                    if value.is_null() {
                        settings.dirs.remove(key);
                    } else {
                        let path = value
                            .as_str()
                            .context("Directory settings need a path or null.")?;
                        settings.dirs.insert(
                            key.clone(),
                            directory(Path::new(path))?.to_string_lossy().into_owned(),
                        );
                    }
                }
            }
            *current = settings;
            Ok(())
        })?;
        Ok(json!({ "changed": true, "active_profile": updated.active_profile }))
    })
}

fn set_game_folder(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let pick = storage::absolute(Path::new(args.need("path")?))?;
    let selected = if pick.is_file()
        && pick
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("MicrosoftGame.Config"))
    {
        pick.parent().and_then(game::from_folder)
    } else if pick.is_dir() {
        game::from_pick(&pick)
    } else {
        None
    };
    let game = selected
        .context("Choose a game folder containing MicrosoftGame.Config and its game executable.")?;
    backend.edit(|| {
        let _guard = storage::lock(&backend.dirs.activity_lock())?;
        store::edit_settings(&backend.dirs.settings_file(), |settings| {
            actions::set_game(settings, &game);
            settings.setup_done = true;
            Ok(())
        })?;
        Ok(json!({ "title": game.title, "folder": game.folder, "executable": game.exe }))
    })
}

fn list_presets(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let file = builtin();
    let automatic = backend.has_hardware().then(|| {
        let hw = backend.hardware();
        &file.auto_for(&hw.gpu_name, hw.vendor_id).id
    });
    let presets: Vec<_> = file
        .preset
        .iter()
        .map(|preset| {
            json!({
                "id": preset.id, "name": preset.name, "description": preset.description,
            })
        })
        .collect();
    Ok(json!({ "automatic": automatic, "presets": presets }))
}

fn list_profiles(backend: &Arc<Backend>, _: &Args) -> Result<Value> {
    let dirs = &backend.dirs;
    let settings = dirs.settings();
    let mut profiles = launch::list_profiles(&dirs.profiles_dir());
    if !profiles.iter().any(|p| p.name == settings.active_profile) {
        profiles.push(actions::active_profile(dirs, &settings));
    }
    Ok(json!({ "active": settings.active_profile, "profiles": profiles }))
}

fn profile_named(backend: &Backend, name: Option<&str>) -> Result<launch::Profile> {
    let settings = backend.dirs.settings();
    match name {
        None => Ok(actions::active_profile(&backend.dirs, &settings)),
        Some(name) if name == settings.active_profile => {
            Ok(actions::active_profile(&backend.dirs, &settings))
        }
        Some(name) => launch::list_profiles(&backend.dirs.profiles_dir())
            .into_iter()
            .find(|p| p.name == name)
            .with_context(|| format!("There is no profile called {name}.")),
    }
}

fn activate(backend: &Backend, name: &str) -> Result<()> {
    store::edit_settings(&backend.dirs.settings_file(), |settings| {
        settings.active_profile = name.into();
        Ok(())
    })?;
    Ok(())
}

fn apply_preset(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let mut profile = profile_named(backend, None)?;
        launch::set_preset(builtin(), &mut profile, args.need("preset")?)?;
        launch::save_profile(&backend.dirs.profiles_dir(), &profile)?;
        Ok(json!({ "profile": profile.name, "preset": profile.options.preset }))
    })
}

fn edit_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let patch = args.object("options").context("options needs an object.")?;
    restrict(
        patch,
        &[
            "console",
            "safe",
            "toggles",
            "upscaler",
            "upscaler_mode",
            "frame_generation",
        ],
    )?;
    backend.edit(|| {
        let mut profile = profile_named(backend, args.text("name"))?;
        let mut merged = serde_json::to_value(&profile.options)?;
        patch_values(&mut merged, patch)?;
        let mut options: launch::Options =
            serde_json::from_value(merged).context("Invalid profile options")?;
        validate_options(&mut options, patch)?;
        profile.options = options;
        launch::save_profile(&backend.dirs.profiles_dir(), &profile)?;
        Ok(json!(profile))
    })
}

fn validate_options(options: &mut launch::Options, changed: &Map<String, Value>) -> Result<()> {
    let file = builtin();
    if let Some(toggles) = changed.get("toggles").and_then(Value::as_object) {
        ensure!(
            toggles
                .keys()
                .all(|name| file.toggle.iter().any(|t| t.id == *name)),
            "There is an unknown profile toggle."
        );
    }
    if changed.contains_key("upscaler")
        || changed.contains_key("upscaler_mode")
        || changed.contains_key("frame_generation")
    {
        let up = file
            .upscaler
            .iter()
            .find(|up| up.id == options.upscaler)
            .context("Unknown upscaler.")?;
        ensure!(up.available, "That upscaler is not available.");
        if changed.contains_key("upscaler") && !changed.contains_key("upscaler_mode") {
            if let Some(mode) = up.modes.first() {
                options.upscaler_mode.clone_from(mode);
            }
        }
        if changed.contains_key("upscaler_mode") || !up.modes.is_empty() {
            ensure!(
                up.modes.contains(&options.upscaler_mode),
                "Unknown mode for this upscaler."
            );
        }
        if options.frame_generation {
            ensure!(
                file.frame_generation.available,
                "Frame generation is not available."
            );
            ensure!(
                file.frame_generation
                    .requires_upscaler
                    .as_ref()
                    .is_none_or(|id| id == &up.id),
                "Frame generation needs a different upscaler."
            );
        }
    }
    Ok(())
}

fn select_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let profile = profile_named(backend, Some(args.need("name")?))?;
        activate(backend, &profile.name)?;
        Ok(json!({ "active": profile.name }))
    })
}

fn new_name<'a>(args: &'a Args, field: &str) -> Result<&'a str> {
    let name = args.need(field)?;
    ensure!(
        name.len() <= 100 && launch::file_safe_name(name) == name,
        "Use up to 100 bytes of letters, numbers, spaces, hyphens or underscores in a profile name."
    );
    Ok(name)
}

fn create_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let name = new_name(args, "name")?;
        ensure!(
            backend.dirs.settings().active_profile != name,
            "That profile already exists."
        );
        let copy = args
            .text("copy_from")
            .map(|name| profile_named(backend, Some(name)))
            .transpose()?;
        let profile = launch::create_profile(&backend.dirs.profiles_dir(), name, copy.as_ref())?;
        activate(backend, &profile.name)?;
        Ok(json!(profile))
    })
}

fn rename_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let name = args.need("name")?;
        let new = new_name(args, "new_name")?;
        let mut profile = profile_named(backend, Some(name))?;
        let dir = backend.dirs.profiles_dir();
        let active = backend.dirs.settings().active_profile;
        ensure!(
            active != new && !launch::profile_path(&dir, new).exists(),
            "That profile already exists."
        );
        profile.name = new.to_owned();
        launch::save_renamed(&dir, &profile, name)?;
        if active == name {
            activate(backend, new)?;
        }
        Ok(json!(profile))
    })
}

fn delete_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    backend.edit(|| {
        let active = backend.dirs.settings().active_profile;
        let name = args.need("name")?;
        let next = launch::remove_profile(&backend.dirs.profiles_dir(), name, &active)?;
        if active == name {
            activate(backend, &next.name)?;
        }
        Ok(json!({ "deleted": name }))
    })
}

fn import_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let path = Path::new(args.need("path")?);
    let profile = launch::load_profile_file(path).context("Could not read that profile.")?;
    backend.edit(|| {
        let name = new_name(args, "name")?;
        ensure!(
            backend.dirs.settings().active_profile != name,
            "That profile already exists."
        );
        Ok(json!(launch::create_profile(
            &backend.dirs.profiles_dir(),
            name,
            Some(&profile)
        )?))
    })
}

fn export_profile(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let path = args.need("path")?;
    let profile = profile_named(backend, Some(args.need("name")?))?;
    let text = toml::to_string_pretty(&profile)?;
    let mut file = std::fs::File::options()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    Ok(json!({ "path": path }))
}

fn list_flags(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    let search = args.text("search").unwrap_or("").to_ascii_lowercase();
    let offset = args.int("offset").unwrap_or(0) as usize;
    let limit = args.int("limit").unwrap_or(50) as usize;
    let flags = registry().flags.iter().filter(|flag| {
        [
            flag.name.as_str(),
            flag.doc.as_str(),
            flag.group.as_deref().unwrap_or(""),
        ]
        .iter()
        .any(|text| text.to_ascii_lowercase().contains(&search))
    });
    let total = flags.clone().count();
    let flags: Vec<_> = flags.skip(offset).take(limit).map(|flag| json!({
        "name": flag.name, "type": match flag.kind { flagreg::Kind::Switch => "switch", flagreg::Kind::Number => "number", flagreg::Kind::Text => "text", flagreg::Kind::Choice => "choice" }, "default": flag.default, "risk": flag.risk.label(),
        "description": flag.doc, "group": flag.group, "presence": flag.presence, "min": flag.min,
        "max": flag.max, "choices": flag.choices,
    })).collect();
    let extra: Vec<_> = launch::read_extra(&backend.dirs.launch_flags_file())
        .into_iter()
        .map(|flag| {
            json!({
                "name": flag.name, "enabled": flag.enabled,
                "value": super::super::redact::flag_value(&flag.name, flag.value),
            })
        })
        .collect();
    Ok(json!({ "flags": flags, "total": total, "extra": extra }))
}

fn set_flag(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    change_flag(backend, args, false)
}

fn clear_flag(backend: &Arc<Backend>, args: &Args) -> Result<Value> {
    change_flag(backend, args, true)
}

fn change_flag(backend: &Backend, args: &Args, clear: bool) -> Result<Value> {
    let name = args.need("name")?;
    ensure!(
        name.starts_with("VICTORY_") && launch::valid_name(name),
        "Use a valid VICTORY_ flag name."
    );
    ensure!(
        !launch::PATH_VARS.contains(&name),
        "Use the launcher settings for its managed paths."
    );
    let value = args.0.get("value").and_then(Value::as_str);
    ensure!(
        value.is_none_or(|v| !v.contains(['\0', '\n', '\r'])),
        "A flag value must fit on a single line."
    );
    let enabled = args.flag("enabled").unwrap_or(true);
    backend.edit(|| {
        if args.text("scope") == Some("profile") {
            let mut profile = profile_named(backend, None)?;
            let flag = registry()
                .get(name)
                .context("Profile overrides require a flag from list_flags.")?;
            if clear {
                profile.set_edit(name, None);
            } else if flag.kind == flagreg::Kind::Switch {
                ensure!(value.is_none(), "Use enabled to set a profile switch.");
                profile.set_switch(flag, enabled);
            } else {
                ensure!(enabled, "Use clear_flag to remove an override.");
                let value = value.context("This override needs a value.")?;
                validate_flag_value(flag, value)?;
                profile.set_edit(name, Some(value.into()));
            }
            launch::save_profile(&backend.dirs.profiles_dir(), &profile)?;
        } else {
            let file = backend.dirs.launch_flags_file();
            let mut flags = launch::read_extra(&file);
            if clear {
                flags.retain(|flag| flag.name != name);
            } else {
                let value = launch::default_value(name, value.map(str::to_owned));
                launch::put_extra(&mut flags, name, value);
                if let Some(flag) = flags.iter_mut().find(|flag| flag.name == name) {
                    flag.enabled = enabled;
                }
            }
            launch::write_extra(&file, &flags)?;
        }
        Ok(json!({ "name": name, "changed": true }))
    })
}

fn validate_flag_value(flag: &flagreg::Flag, value: &str) -> Result<()> {
    match flag.kind {
        flagreg::Kind::Number => {
            let n: f64 = value.parse().context("The flag needs a number.")?;
            ensure!(
                n.is_finite()
                    && flag.min.is_none_or(|min| n >= min)
                    && flag.max.is_none_or(|max| n <= max),
                "The flag value is outside its supported range."
            );
        }
        flagreg::Kind::Choice if !flag.choices.iter().any(|choice| choice == value) => {
            bail!("Use a choice from list_flags.")
        }
        _ => {}
    }
    Ok(())
}
