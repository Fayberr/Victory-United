//! The work behind the buttons as plain functions: no widgets, plain arguments, a `Result`.
//! The screens and the agent interface call the same ones, so they cannot behave differently.
use super::{
    game::Game,
    hw::Hardware,
    launch::{self, ExtraFlag, Plan, Profile, Resolved, Source},
    play, presets::builtin,
    runner::{self, RunStorage, Runner},
    saves, storage,
    store::{Settings, StorageSettings},
};
use crate::ops::OpsSettings;
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use storage::{Location, MoveContext};

/// The two folders everything else lives in. The real ones come from `paths`; tests use temp folders.
#[derive(Clone, Debug, PartialEq)]
pub struct Dirs {
    /// `%APPDATA%\Victory-United`
    pub data: PathBuf,
    /// Folder of the launcher executable.
    pub app: PathBuf,
}

impl Dirs {
    pub fn real() -> Self {
        Self { data: crate::paths::data_dir(), app: crate::paths::app_dir() }
    }
    pub fn settings_file(&self) -> PathBuf { self.data.join("settings.toml") }
    pub fn ops_file(&self) -> PathBuf { self.data.join("ops.toml") }
    pub fn profiles_dir(&self) -> PathBuf { self.data.join("profiles") }
    pub fn logs_dir(&self) -> PathBuf { self.data.join("logs") }
    pub fn reports_dir(&self) -> PathBuf { self.data.join("reports") }
    pub fn downloads_dir(&self) -> PathBuf { self.data.join("downloads") }
    pub fn launch_flags_file(&self) -> PathBuf { self.data.join("launch-flags.txt") }
    pub fn activity_lock(&self) -> PathBuf { self.data.join("activity.lock") }
    pub fn move_journal(&self) -> PathBuf { self.data.join("move-journal.toml") }
    pub fn engine_root(&self) -> PathBuf { self.app.join("engine") }

    pub fn settings(&self) -> Settings {
        Settings::load_from(&self.settings_file())
    }
    pub fn location(&self, location: Location, settings: &Settings) -> PathBuf {
        location.path(&self.data, settings)
    }
    pub fn backups_root(&self, settings: &Settings) -> PathBuf {
        settings.backups.root.clone().unwrap_or_else(|| self.data.join("backups"))
    }

    /// The engine build folder name in `engine\current.txt`, if any.
    pub fn current_engine_name(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.engine_root().join("current.txt")).ok()?;
        let name = text.trim();
        (!name.is_empty()
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
            .then(|| name.to_string())
    }

    /// A developer's own engine from the settings, otherwise the installed build.
    pub fn engine_exe(&self, settings: &Settings) -> Option<PathBuf> {
        own_engine(settings).or_else(|| {
            let exe = self.engine_root().join(self.current_engine_name()?).join("victory.exe");
            exe.is_file().then_some(exe)
        })
    }
}

/// A developer build, if the selected file is still a usable engine.
pub fn own_engine(settings: &Settings) -> Option<PathBuf> {
    settings.engine_override.clone().filter(|path| super::engine::check(path).is_ok())
}

/// The name of the build folder an engine executable sits in.
pub fn build_name(engine: Option<&Path>) -> String {
    engine
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The game chosen in the settings, if its folder is still a game folder.
pub fn chosen_game(settings: &Settings) -> Option<Game> {
    settings
        .game_folder
        .as_deref()
        .and_then(super::game::from_pick)
        .or_else(|| settings.game_exe.as_deref().and_then(super::game::from_pick))
}

/// Records the game selected by the folder picker.
pub fn set_game(settings: &mut Settings, game: &Game) {
    settings.game_folder = Some(game.folder.clone());
    settings.game_exe = Some(game.exe.clone());
}

/// The saved profile the settings name, or the default one.
pub fn active_profile(dirs: &Dirs, settings: &Settings) -> Profile {
    launch::list_profiles(&dirs.profiles_dir())
        .into_iter()
        .find(|p| p.name == settings.active_profile)
        .unwrap_or_else(|| Profile { name: settings.active_profile.clone(), ..Profile::default() })
}

/// The preset in use for this card and the environment a start would get from it, the profile and the
/// extra flags.
pub fn resolve(profile: &Profile, hw: Option<&Hardware>, dirs: &BTreeMap<String, String>, extra: &[ExtraFlag]) -> (Resolved, String) {
    let file = builtin();
    let (gpu, vendor) = hw.map(|h| (h.gpu_name.as_str(), h.vendor_id)).unwrap_or_default();
    let id = launch::preset_id(file, profile, gpu, vendor).to_string();
    let dlss = dlss_flags(profile, hw, dirs);
    // One upscaler at a time: the engine refuses to start with both. DLSS wins when it is switched on for a
    // card that has it, which is what the Settings tab shows as the chosen upscaler.
    let dlss_chosen = profile.options.dlss.enabled && super::dlss::card(gpu, vendor).offered();
    let mut without_fsr;
    let profile = if dlss_chosen && profile.options.upscaler != "off" {
        without_fsr = profile.clone();
        without_fsr.options.upscaler = "off".into();
        &without_fsr
    } else {
        profile
    };
    (launch::resolve_with(file, &id, profile, extra, &dlss), id)
}

/// The DLSS environment for this profile: none in safe mode, none on a card without RTX.
pub fn dlss_flags(profile: &Profile, hw: Option<&Hardware>, dirs: &BTreeMap<String, String>) -> Vec<(String, String)> {
    if profile.options.safe {
        return Vec::new();
    }
    let (gpu, vendor) = hw.map(|h| (h.gpu_name.as_str(), h.vendor_id)).unwrap_or_default();
    super::dlss::flags(&profile.options.dlss, super::dlss::card(gpu, vendor), dirs)
}

/// Everything a start is built from.
pub struct LaunchInput<'a> {
    pub settings: &'a Settings,
    pub profile: &'a Profile,
    /// The extra flags that apply to this start (empty for "start without my flags").
    pub extra: &'a [ExtraFlag],
    pub hardware: Option<&'a Hardware>,
    pub game: Option<&'a Game>,
    pub engine: Option<PathBuf>,
}

impl LaunchInput<'_> {
    pub fn plan(&self, cache_root: &Path, data_dir: &Path) -> Option<Plan> {
        let game = self.game?;
        let engine = self.engine.clone()?;
        let (resolved, _) = resolve(self.profile, self.hardware, &self.settings.dirs, self.extra);
        Some(Plan::new(
            engine,
            &game.folder,
            game.exe.clone(),
            self.profile.options.console,
            &resolved,
            &super::dlss::without_dlss_dirs(&self.settings.dirs),
            cache_root,
            data_dir,
        ))
    }
}

/// Adds what the monitor and profiler settings need to the environment. A value the player set on
/// purpose (an edit, an extra flag or a removal) wins.
pub fn apply_ops_flags(
    plan: &mut Plan,
    profile: &Profile,
    resolved: &Resolved,
    ops: &OpsSettings,
    data: &Path,
) -> Result<()> {
    merge_ops_flags(plan, profile, resolved, crate::ops::settings::engine_options(ops, data)?);
    Ok(())
}

pub fn merge_ops_flags(
    plan: &mut Plan,
    profile: &Profile,
    resolved: &Resolved,
    flags: std::collections::BTreeMap<String, String>,
) {
    for (key, value) in flags {
        let chosen = matches!(resolved.source.get(&key), Some(Source::Edit | Source::Extra))
            || profile.removed.contains(&key);
        if !chosen {
            plan.env.insert(key, value);
        }
    }
}

/// Starts the warm-up and the game. Returns the storage locations it started with (another launcher may
/// have moved them since `input.settings` was read).
pub fn start_game(
    runner: &mut Runner,
    dirs: &Dirs,
    input: &LaunchInput<'_>,
    ops: &OpsSettings,
    acknowledged: bool,
) -> Result<StorageSettings> {
    play::hardware_allows_play(input.hardware, acknowledged)?;
    runner.drain();
    if runner.busy() {
        bail!("The game is already running or starting.");
    }
    let settings_file = dirs.settings_file();
    let engine_root = dirs.engine_root();
    let game = input.game.map(|g| g.folder.as_path());
    let ctx = MoveContext { settings: &settings_file, engine: &engine_root, game };
    let guard = storage::lock(&dirs.activity_lock())?;
    let current = Settings::load_from(&settings_file);
    storage::validate_data_work(&ctx, &current)?;
    saves::recover_restore(&dirs.location(Location::Data, &current))?;
    let cache = dirs.location(Location::Cache, &current);
    let data = dirs.location(Location::Data, &current);
    let Some(mut plan) = input.plan(&cache, &data) else {
        return Err(anyhow!(if input.game.is_none() {
            "Pick the game folder first."
        } else {
            "No engine is installed yet."
        }));
    };
    let (resolved, _) = resolve(input.profile, input.hardware, &input.settings.dirs, input.extra);
    apply_ops_flags(&mut plan, input.profile, &resolved, ops, &dirs.data)?;
    let cold = runner::shader_entries(&plan.cache_dir, runner::COLD_CACHE_ENTRIES) < runner::COLD_CACHE_ENTRIES;
    let saves = RunStorage {
        _guard: guard,
        backups: dirs.backups_root(input.settings),
        before: input.settings.backups.before_launch,
        after: input.settings.backups.after_exit,
        retention: input.settings.backups.retention,
    };
    if !runner.start(plan, dirs.logs_dir(), cold, !input.extra.is_empty(), Some(saves)) {
        bail!("The game is already running or starting.");
    }
    Ok(current.storage)
}

// ---------------------------------------------------------------- backups

pub enum SaveAction {
    Create(String),
    Restore(String),
    Verify(String),
    Export(String, PathBuf),
    Import(PathBuf),
    Delete(String),
    Rename(String, String),
    Cleanup(Vec<String>),
}

/// Runs `work` while nothing else uses the saves. `work` gets the data folder and the engine build name.
pub fn with_saves<T>(
    dirs: &Dirs,
    root: &Path,
    game: Option<&Path>,
    engine: Option<&Path>,
    work: impl FnOnce(&Path, &str) -> Result<T>,
) -> Result<T> {
    let _guard = storage::lock(&dirs.activity_lock())?;
    let settings_file = dirs.settings_file();
    let engine_root = dirs.engine_root();
    let ctx = MoveContext { settings: &settings_file, engine: &engine_root, game };
    let current = Settings::load_from(&settings_file);
    storage::validate_backup_root(root, &ctx, &current)?;
    let data = dirs.location(Location::Data, &current);
    storage::validate_data_work(&ctx, &current)?;
    work(&data, &build_name(engine))
}

/// One backup action. Returns the key of the text that says what happened.
pub fn save_action(
    dirs: &Dirs,
    root: &Path,
    game: Option<&Path>,
    engine: Option<&Path>,
    policy: saves::Retention,
    action: SaveAction,
) -> Result<&'static str> {
    with_saves(dirs, root, game, engine, |data, build| {
        Ok(match action {
            SaveAction::Create(label) => {
                let options = saves::Create { kind: saves::Kind::Manual, label: &label, engine_build: build, deadline: None, force: false };
                if saves::create(data, root, options)?.is_some() { "saves.created" } else { "saves.unchanged" }
            }
            SaveAction::Restore(id) => {
                saves::restore(data, root, &id, build)?;
                "saves.restored"
            }
            SaveAction::Verify(id) => {
                saves::verify(root, &id)?;
                "saves.verified"
            }
            SaveAction::Export(id, out) => {
                saves::export(root, &id, &out)?;
                "saves.exported"
            }
            SaveAction::Import(source) => {
                saves::import(root, &source)?;
                "saves.imported"
            }
            SaveAction::Delete(id) => {
                saves::delete(root, &id)?;
                "saves.deleted"
            }
            SaveAction::Rename(id, label) => {
                saves::rename(root, &id, &label)?;
                "saves.renamed"
            }
            SaveAction::Cleanup(ids) => {
                saves::cleanup(root, policy, &ids)?;
                "saves.cleaned"
            }
        })
    })
}

// ---------------------------------------------------------------- storage

pub enum StorageAction {
    Move(Location, PathBuf),
    Resume,
    /// Close a move: delete the old folder or keep it. Also cancels a move that was not finished.
    Finish(bool),
    Clear(Location),
}

impl StorageAction {
    pub fn done_message(&self) -> &'static str {
        match self {
            StorageAction::Move(..) | StorageAction::Resume => "storage.moved",
            StorageAction::Finish(_) => "storage.finished",
            StorageAction::Clear(_) => "storage.cleared",
        }
    }
}

pub fn storage_action(
    dirs: &Dirs,
    game: Option<&Path>,
    build: &str,
    action: StorageAction,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<()> {
    let settings = dirs.settings_file();
    let engine = dirs.engine_root();
    let runtime = MoveContext { settings: &settings, engine: &engine, game };
    match action {
        StorageAction::Move(location, target) => {
            storage::start_move(&runtime, location, &target, build, cancel, progress).map(|_| ())
        }
        StorageAction::Resume => storage::resume_move(&runtime, cancel, progress).map(|_| ()),
        StorageAction::Finish(delete_old) => storage::finish_move(&runtime, delete_old),
        StorageAction::Clear(location) => storage::clear(&runtime, location, build),
    }
}

/// The newest run log, or the one the runner is writing.
pub fn log_for_report(running: Option<PathBuf>, logs: &Path) -> Option<PathBuf> {
    running.or_else(|| runner::latest_run_log(logs))
}

/// A log file by name inside the log folder. Names with a path in them are refused.
pub fn log_by_name(logs: &Path, name: &str) -> Result<PathBuf> {
    let plain = Path::new(name).file_name().is_some_and(|n| n == name) && !name.contains(['/', '\\', ':']);
    let ours = (name.starts_with("run-") || name.starts_with("warmup-")) && name.ends_with(".log");
    anyhow::ensure!(plain && ours, "Use a log name such as run-20260101-120000.log.");
    let path = logs.join(name);
    storage::metadata(&path)?.filter(|metadata| metadata.is_file()).context("There is no log with that name.")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_names_cannot_leave_the_log_folder() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("run-1.log"), "x").unwrap();
        assert!(log_by_name(temp.path(), "run-1.log").is_ok());
        for bad in ["../run-1.log", "run-/../../x.log", "C:run-1.log", "settings.toml", "run-2.log", ""] {
            assert!(log_by_name(temp.path(), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fsr_and_dlss_never_reach_the_engine_together() {
        let mut nvidia = Hardware::demo();
        nvidia.gpu_name = "NVIDIA GeForce RTX 3080 Ti".into();
        nvidia.vendor_id = 0x10DE;
        let mut amd = nvidia.clone();
        amd.gpu_name = "AMD Radeon RX 7900 XTX".into();
        amd.vendor_id = 0x1002;
        let mut profile = Profile::default();
        profile.options.upscaler = "fsr".into();
        profile.options.upscaler_mode = "balanced".into();
        profile.options.dlss.enabled = true;
        let dirs: BTreeMap<String, String> = [(super::super::dlss::STREAMLINE_DIR.to_string(), "C:/sl".to_string())].into();
        // RTX card with both switched on: DLSS runs, FSR is left out.
        let (r, _) = resolve(&profile, Some(&nvidia), &dirs, &[]);
        assert_eq!(r.env.get("VICTORY_DLSS").map(String::as_str), Some("1"));
        assert!(!r.env.contains_key("VICTORY_FSR_4K"), "FSR must not be set next to DLSS");
        // DLSS chosen but no NVIDIA folder yet: still not FSR next to it (the Settings tab shows DLSS as the choice).
        let (r, _) = resolve(&profile, Some(&nvidia), &BTreeMap::new(), &[]);
        assert!(!r.env.contains_key("VICTORY_DLSS") && !r.env.contains_key("VICTORY_FSR_4K"));
        // A card without RTX cannot use DLSS, so the stored FSR choice stays.
        let (r, _) = resolve(&profile, Some(&amd), &dirs, &[]);
        assert_eq!(r.env.get("VICTORY_FSR_4K").map(String::as_str), Some("balanced"));
        assert!(!r.env.contains_key("VICTORY_DLSS"));
        // FSR alone is unchanged.
        profile.options.dlss.enabled = false;
        let (r, _) = resolve(&profile, Some(&nvidia), &dirs, &[]);
        assert_eq!(r.env.get("VICTORY_FSR_4K").map(String::as_str), Some("balanced"));
        assert!(!r.env.contains_key("VICTORY_DLSS"));
    }

    #[test]
    fn starting_needs_a_game_and_an_engine_and_takes_the_lock() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = Dirs { data: temp.path().join("data-root"), app: temp.path().join("app") };
        let settings = Settings::default();
        let profile = Profile::default();
        let mut hardware = Hardware::demo();
        let input = LaunchInput { settings: &settings, profile: &profile, extra: &[], hardware: Some(&hardware), game: None, engine: None };
        let mut run = Runner::new(Default::default());
        let error = start_game(&mut run, &dirs, &input, &OpsSettings::default(), false).unwrap_err();
        assert!(error.to_string().contains("Pick the game folder"), "{error}");
        assert!(!run.busy());
        hardware.mesh_shaders = Some(false);
        let input = LaunchInput { settings: &settings, profile: &profile, extra: &[], hardware: Some(&hardware), game: None, engine: None };
        assert!(start_game(&mut run, &dirs, &input, &OpsSettings::default(), false).is_err());
        let _held = storage::lock(&dirs.activity_lock()).unwrap();
        let error = start_game(&mut run, &dirs, &input, &OpsSettings::default(), true).unwrap_err();
        assert!(error.to_string().contains("Close the game"), "{error}");
    }
}
