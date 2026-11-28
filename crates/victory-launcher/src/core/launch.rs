//! Launch profiles and how the final environment is resolved.
//!
//! Layers, later wins: 1 preset (common values, the preset and what it inherits), 2 profile options
//! (full screen, panel, ray tracing, upscaler, frame generation), 3 raw edits (the typed fields of the
//! Settings tab, stored in the profile), 4 extra flags (the Flags tab, `launch-flags.txt`). Edits and
//! extra flags can also remove a value: a switch that is off is removed, never set to `0`, unless the
//! flag registry says the engine wants an explicit `0` (see `flagreg::Flag::off_value`).
use super::{
    flagreg::{registry, Flag, Kind},
    presets::{Env, PresetFile},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Console {
    #[default]
    SeriesX,
    SeriesS,
}

impl Console {
    /// What a player reads in the settings.
    pub fn label(self) -> &'static str {
        match self {
            Console::SeriesX => "Series X",
            Console::SeriesS => "Series S",
        }
    }

    pub fn arg(self) -> &'static str {
        match self {
            Console::SeriesX => "series-x",
            Console::SeriesS => "series-s",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Options {
    /// "auto" or a preset id.
    pub preset: String,
    pub console: Console,
    pub safe: bool,
    /// Toggle id to state. A toggle that is missing uses its default from the preset file.
    pub toggles: BTreeMap<String, bool>,
    /// Upscaler id ("off", "fsr", "dlss") and mode.
    pub upscaler: String,
    pub upscaler_mode: String,
    /// NVIDIA DLSS, off unless the person turns it on (see `dlss`).
    pub dlss: super::dlss::DlssSettings,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            preset: "auto".into(),
            console: Console::default(),
            safe: false,
            toggles: BTreeMap::new(),
            upscaler: "off".into(),
            upscaler_mode: "quality".into(),
            dlss: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub options: Options,
    /// Raw edits: flag name to value. Beat the preset and the options.
    pub edits: BTreeMap<String, String>,
    /// Flags the profile removes from the preset on purpose.
    pub removed: Vec<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self { name: "Default".into(), options: Options::default(), edits: BTreeMap::new(), removed: Vec::new() }
    }
}

/// One line of `launch-flags.txt`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExtraFlag {
    pub name: String,
    /// `None` is a presence-only flag.
    pub value: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Preset,
    Option,
    Edit,
    Extra,
    Path,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Preset => "preset",
            Source::Option => "option",
            Source::Edit => "edit",
            Source::Extra => "extra flag",
            Source::Path => "path",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub env: BTreeMap<String, String>,
    pub source: BTreeMap<String, Source>,
}

impl Resolved {
    fn set(&mut self, k: &str, v: &str, s: Source) {
        self.env.insert(k.to_string(), v.to_string());
        self.source.insert(k.to_string(), s);
    }
    fn remove(&mut self, k: &str) {
        self.env.remove(k);
        self.source.remove(k);
    }
    pub fn sorted(&self) -> Vec<(String, String)> {
        self.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

/// The preset id a profile uses (resolving "auto" by the card).
pub fn preset_id<'a>(file: &'a PresetFile, profile: &'a Profile, gpu_name: &str, vendor_id: u32) -> &'a str {
    if profile.options.preset == "auto" || file.preset(&profile.options.preset).is_none() {
        &file.auto_for(gpu_name, vendor_id).id
    } else {
        &profile.options.preset
    }
}

/// Values the options of a profile add (layer 2), also used to show what each control does.
pub fn option_env(file: &PresetFile, o: &Options) -> Env {
    let mut env = Env::new();
    for t in &file.toggle {
        if o.toggles.get(&t.id).copied().unwrap_or(t.default) {
            env.extend(t.on.clone());
        }
    }
    if let Some(u) = file.upscaler.iter().find(|u| u.id == o.upscaler && u.available) {
        for (k, v) in &u.env {
            env.insert(k.clone(), v.replace("{mode}", &o.upscaler_mode));
        }
    }
    env
}

/// The whole resolution. `preset` is the id from `preset_id`.
pub fn resolve(file: &PresetFile, preset: &str, profile: &Profile, extra: &[ExtraFlag]) -> Resolved {
    resolve_with(file, preset, profile, extra, &[])
}

/// `resolve` with extra option-layer values (the DLSS flags, which depend on the card and chosen folders).
pub fn resolve_with(file: &PresetFile, preset: &str, profile: &Profile, extra: &[ExtraFlag], option_layer: &[(String, String)]) -> Resolved {
    let mut r = Resolved::default();
    for (k, v) in file.preset_env(preset, profile.options.safe) {
        r.set(&k, &v, Source::Preset);
    }
    for (k, v) in option_env(file, &profile.options) {
        r.set(&k, &v, Source::Option);
    }
    for (k, v) in option_layer {
        r.set(k, v, Source::Option);
    }
    for k in &profile.removed {
        r.remove(k);
    }
    for (k, v) in &profile.edits {
        r.set(k, v, Source::Edit);
    }
    for f in extra.iter().filter(|f| f.enabled) {
        r.set(&f.name, f.value.as_deref().unwrap_or("1"), Source::Extra);
    }
    r
}

/// Directory names for the per-install cache: readable part plus a hash of the whole path.
pub fn cache_folder_name(game_folder: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in game_folder.to_string_lossy().to_lowercase().replace('\\', "/").bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let readable: String = game_folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(24)
        .collect();
    format!("{readable}-{:08x}", h as u32)
}

/// Everything needed to start the engine.
#[derive(Clone, Debug)]
pub struct Plan {
    pub engine: PathBuf,
    pub game_exe: PathBuf,
    pub console: Console,
    pub env: BTreeMap<String, String>,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

/// Names of path variables the launcher sets itself. Other inherited `VICTORY_*` values are removed from the
/// child environment, as scripts/play.ps1 does, so a stray setting in the user's shell cannot change a run.
pub const PATH_VARS: [&str; 5] = ["VICTORY_CONFIG", "VICTORY_GAME_DIR", "VICTORY_CACHE_DIR", "VICTORY_DATA_DIR", "VICTORY_FIDELITYFX_DIR"];

impl Plan {
    pub fn new(
        engine: PathBuf,
        game_folder: &Path,
        game_exe: PathBuf,
        console: Console,
        resolved: &Resolved,
        dirs: &BTreeMap<String, String>,
        cache_root: &Path,
        data_dir: &Path,
    ) -> Self {
        let cache_dir = cache_root.join(cache_folder_name(game_folder));
        let data_dir = data_dir.to_path_buf();
        let mut env = resolved.env.clone();
        for (k, v) in dirs {
            if !v.is_empty() {
                env.insert(k.clone(), v.clone());
            }
        }
        env.insert("VICTORY_GAME_DIR".into(), game_folder.to_string_lossy().into_owned());
        env.insert("VICTORY_CACHE_DIR".into(), cache_dir.to_string_lossy().into_owned());
        env.insert("VICTORY_DATA_DIR".into(), data_dir.to_string_lossy().into_owned());
        Self { engine, game_exe, console, env, cache_dir, data_dir }
    }

    /// `victory.exe run <exe> --console <c> --execute`
    pub fn run_args(&self) -> Vec<String> {
        vec!["run".into(), self.game_exe.to_string_lossy().into_owned(), "--console".into(), self.console.arg().into(), "--execute".into()]
    }

    /// `victory.exe shaders <game folder>`
    pub fn warm_args(&self) -> Vec<String> {
        vec!["shaders".into(), self.env.get("VICTORY_GAME_DIR").cloned().unwrap_or_default()]
    }

    pub fn command_line(&self) -> String {
        let q = |s: &str| if s.contains(' ') { format!("\"{s}\"") } else { s.to_string() };
        let mut out = q(&self.engine.to_string_lossy());
        for a in self.run_args() {
            out.push(' ');
            out.push_str(&q(&a));
        }
        out
    }
}

// ---------------------------------------------------------------- profile files

pub fn file_safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "profile".into() } else { s }
}

pub fn profile_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.toml", file_safe_name(name)))
}

pub fn save_profile(dir: &Path, p: &Profile) -> std::io::Result<()> {
    super::store::write_atomic(&profile_path(dir, &p.name), toml::to_string_pretty(p).unwrap_or_default().as_bytes())
}

pub fn load_profile_file(path: &Path) -> Option<Profile> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// All saved profiles, by name. A profile whose file name and `name` field disagree keeps the file's name.
pub fn list_profiles(dir: &Path) -> Vec<Profile> {
    let mut out: Vec<Profile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| {
            let mut prof = load_profile_file(&p)?;
            if let Some(stem) = p.file_stem() {
                prof.name = stem.to_string_lossy().into_owned();
            }
            Some(prof)
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

pub fn delete_profile(dir: &Path, name: &str) -> std::io::Result<()> {
    std::fs::remove_file(profile_path(dir, name))
}

pub fn rename_profile(dir: &Path, from: &str, to: &str) -> std::io::Result<()> {
    let mut p = load_profile_file(&profile_path(dir, from)).ok_or_else(|| std::io::Error::other("no such profile"))?;
    p.name = to.to_string();
    save_profile(dir, &p)?;
    if file_safe_name(from) != file_safe_name(to) {
        delete_profile(dir, from)?;
    }
    Ok(())
}

/// The preset a profile uses: `auto` or a preset id from `data/presets.toml`.
pub fn set_preset(file: &PresetFile, profile: &mut Profile, id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(id == "auto" || file.preset(id).is_some(), "There is no preset called {id}.");
    profile.options.preset = id.to_string();
    Ok(())
}

impl Profile {
    /// Stores one edit. `None` goes back to what the preset gives.
    pub fn set_edit(&mut self, flag: &str, value: Option<String>) {
        self.edits.remove(flag);
        self.removed.retain(|r| r != flag);
        if let Some(v) = value {
            self.edits.insert(flag.to_string(), v);
        }
    }

    /// Turns a switch on or off. Off removes the variable unless the registry says the engine wants an
    /// explicit 0.
    pub fn set_switch(&mut self, flag: &Flag, on: bool) {
        self.set_edit(&flag.name, None);
        if on {
            self.edits.insert(flag.name.clone(), "1".into());
        } else if let Some(zero) = flag.off_value() {
            self.edits.insert(flag.name.clone(), zero.into());
        } else {
            self.removed.push(flag.name.clone());
        }
    }
}

/// Names of the saved profiles plus the active one (which may not be saved yet), sorted.
pub fn profile_names(profiles: &[Profile], active: &str) -> Vec<String> {
    let mut names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    if !names.iter().any(|n| n == active) {
        names.push(active.to_string());
    }
    names.sort_by_key(|s| s.to_lowercase());
    names
}

/// Saves a new profile. `from` copies the settings of another one.
pub fn create_profile(dir: &Path, name: &str, from: Option<&Profile>) -> anyhow::Result<Profile> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "A profile needs a name.");
    anyhow::ensure!(!profile_path(dir, name).exists(), "A profile called {name} already exists.");
    let mut p = from.cloned().unwrap_or_default();
    p.name = name.to_string();
    save_profile(dir, &p)?;
    Ok(p)
}

/// Saves `profile` (already carrying its new name) and removes the file it had before.
pub fn save_renamed(dir: &Path, profile: &Profile, old: &str) -> std::io::Result<()> {
    save_profile(dir, profile)?;
    if file_safe_name(old) != file_safe_name(&profile.name) && profile_path(dir, old).is_file() {
        delete_profile(dir, old)?;
    }
    Ok(())
}

/// Deletes a profile and returns the one to use next. The last profile cannot be deleted.
pub fn remove_profile(dir: &Path, name: &str, active: &str) -> anyhow::Result<Profile> {
    let all = list_profiles(dir);
    let names = profile_names(&all, active);
    anyhow::ensure!(names.iter().any(|n| n == name), "There is no profile called {name}.");
    anyhow::ensure!(names.len() > 1, "Keep at least one profile.");
    if profile_path(dir, name).is_file() {
        delete_profile(dir, name)?;
    }
    Ok(list_profiles(dir).into_iter().next().unwrap_or_default())
}

// ---------------------------------------------------------------- launch-flags.txt

/// Format: one flag per line, `NAME=VALUE` or `NAME` (presence only); `#off ` in front switches a line off;
/// other lines starting with `#` are comments.
pub fn parse_extra(text: &str) -> Vec<ExtraFlag> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (enabled, body) = match line.strip_prefix("#off ") {
                Some(rest) => (false, rest.trim()),
                None => (true, line),
            };
            if body.is_empty() || body.starts_with('#') {
                return None;
            }
            let (name, value) = match body.split_once('=') {
                Some((n, v)) => (n.trim(), Some(v.trim().to_string())),
                None => (body, None),
            };
            valid_name(name).then(|| ExtraFlag { name: name.to_string(), value, enabled })
        })
        .collect()
}

/// The value an added flag gets when none was typed: `1` for a known switch, presence only for an
/// unknown name or a presence switch, otherwise the default from the registry.
pub fn default_value(name: &str, typed: Option<String>) -> Option<String> {
    match (typed, registry().get(name)) {
        (Some(v), _) => Some(v),
        (None, Some(f)) if f.kind == Kind::Switch && !f.presence => Some("1".to_string()),
        (None, Some(f)) if f.kind == Kind::Switch => None,
        (None, Some(f)) if !f.default.is_empty() => Some(f.default.clone()),
        (None, _) => None,
    }
}

/// Adds an extra flag or replaces the one with the same name, and switches it on.
pub fn put_extra(list: &mut Vec<ExtraFlag>, name: &str, value: Option<String>) {
    if let Some(existing) = list.iter_mut().find(|e| e.name == name) {
        existing.value = value;
        existing.enabled = true;
    } else {
        list.push(ExtraFlag { name: name.to_string(), value, enabled: true });
    }
}

pub fn read_extra(path: &Path) -> Vec<ExtraFlag> {
    std::fs::read_to_string(path).map(|t| parse_extra(&t)).unwrap_or_default()
}

pub fn write_extra(path: &Path, flags: &[ExtraFlag]) -> std::io::Result<()> {
    super::store::write_atomic(path, format_extra(flags).as_bytes())
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn format_extra(flags: &[ExtraFlag]) -> String {
    let mut out = String::from("# Extra launch flags, one per line: NAME=VALUE, or NAME alone for a presence-only flag.\n# A line starting with '#off ' is switched off.\n");
    for f in flags {
        if !f.enabled {
            out.push_str("#off ");
        }
        out.push_str(&f.name);
        if let Some(v) = &f.value {
            out.push('=');
            out.push_str(v);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::presets::builtin;
    use crate::core::testutil::tempdir;

    #[test]
    fn resolution_order_later_wins() {
        let f = builtin();
        let mut p = Profile::default();
        let base = resolve(f, "3080ti", &p, &[]);
        assert_eq!(base.env["VICTORY_THIN_THREADS"], "6");
        assert_eq!(base.source["VICTORY_THIN_THREADS"], Source::Preset);
        // option layer beats the preset
        p.options.toggles.insert("fullscreen".into(), true);
        let r = resolve(f, "3080ti", &p, &[]);
        assert_eq!(r.env["VICTORY_FULLSCREEN"], "1");
        assert_eq!(r.source["VICTORY_FULLSCREEN"], Source::Option);
        // an edit beats an option and the preset
        p.edits.insert("VICTORY_FULLSCREEN".into(), "0".into());
        p.edits.insert("VICTORY_THIN_THREADS".into(), "3".into());
        let r = resolve(f, "3080ti", &p, &[]);
        assert_eq!((r.env["VICTORY_FULLSCREEN"].as_str(), r.source["VICTORY_FULLSCREEN"]), ("0", Source::Edit));
        assert_eq!(r.env["VICTORY_THIN_THREADS"], "3");
        // an extra flag beats an edit
        let extra = [ExtraFlag { name: "VICTORY_THIN_THREADS".into(), value: Some("9".into()), enabled: true }];
        let r = resolve(f, "3080ti", &p, &extra);
        assert_eq!((r.env["VICTORY_THIN_THREADS"].as_str(), r.source["VICTORY_THIN_THREADS"]), ("9", Source::Extra));
        // a disabled extra flag does nothing, and a presence-only one is set to 1
        let off = [ExtraFlag { name: "VICTORY_THIN_THREADS".into(), value: Some("9".into()), enabled: false }];
        assert_eq!(resolve(f, "3080ti", &p, &off).env["VICTORY_THIN_THREADS"], "3");
        let presence = [ExtraFlag { name: "VICTORY_X".into(), value: None, enabled: true }];
        assert_eq!(resolve(f, "3080ti", &p, &presence).env["VICTORY_X"], "1");
        // removal
        p.edits.clear();
        p.removed.push("VICTORY_THIN_THREADS".into());
        assert!(!resolve(f, "3080ti", &p, &[]).env.contains_key("VICTORY_THIN_THREADS"));
    }

    #[test]
    fn options_drive_toggles_upscaler_and_unavailable_entries() {
        let f = builtin();
        let mut o = Options::default();
        assert_eq!(option_env(f, &o).get("VICTORY_FULLSCREEN").map(String::as_str), Some("1")); // default on
        o.toggles.insert("fullscreen".into(), false);
        o.toggles.insert("ray_tracing".into(), true);
        let e = option_env(f, &o);
        assert!(!e.contains_key("VICTORY_FULLSCREEN"));
        assert_eq!(e["VICTORY_BVH_BUILDER"], "1");
        o.upscaler = "fsr".into();
        o.upscaler_mode = "balanced".into();
        assert_eq!(option_env(f, &o)["VICTORY_FSR_4K"], "balanced");
        o.upscaler = "dlss".into();
        let e = option_env(f, &o);
        assert!(!e.contains_key("VICTORY_DLSS") && !e.contains_key("VICTORY_FRAMEGEN"), "DLSS is not an upscaler entry");
    }

    #[test]
    fn safe_mode_and_plan() {
        let f = builtin();
        let mut p = Profile::default();
        p.options.safe = true;
        let r = resolve(f, "4090", &p, &[]);
        assert_eq!(r.env["VICTORY_UNDER_WIDE"], "0");
        assert!(!r.env.contains_key("VICTORY_THIN"));
        let plan = Plan::new("C:/e/victory.exe".into(), Path::new("D:/Games/My Game"), "D:/Games/My Game/g.exe".into(), Console::SeriesS, &r, &BTreeMap::new(), Path::new("C:/cache"), Path::new("C:/data"));
        assert_eq!(plan.run_args(), ["run", "D:/Games/My Game/g.exe", "--console", "series-s", "--execute"]);
        assert!(plan.command_line().starts_with("C:/e/victory.exe run \"D:/Games/My Game/g.exe\""));
        assert!(plan.env["VICTORY_CACHE_DIR"].contains("My-Game-"));
        assert_ne!(cache_folder_name(Path::new("D:/a/g")), cache_folder_name(Path::new("E:/a/g")));
    }

    #[test]
    fn profiles_round_trip_rename_and_files() {
        let dir = tempdir("profiles");
        let mut p = Profile { name: "Fast one".into(), ..Profile::default() };
        p.edits.insert("VICTORY_X".into(), "2".into());
        save_profile(&dir, &p).unwrap();
        save_profile(&dir, &Profile { name: "Another".into(), ..Profile::default() }).unwrap();
        let names: Vec<_> = list_profiles(&dir).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Another", "Fast one"]);
        rename_profile(&dir, "Fast one", "Renamed").unwrap();
        let all = list_profiles(&dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().find(|p| p.name == "Renamed").unwrap().edits["VICTORY_X"], "2");
        delete_profile(&dir, "Renamed").unwrap();
        assert_eq!(list_profiles(&dir).len(), 1);
        assert_eq!(file_safe_name("a/b:c"), "a_b_c");
    }

    #[test]
    fn extra_flag_file_round_trip() {
        let text = "# c\nVICTORY_A=1\nVICTORY_B\n#off VICTORY_C=x y\nnot a flag!\n";
        let flags = parse_extra(text);
        assert_eq!(flags.len(), 3);
        assert_eq!(flags[1].value, None);
        assert!(!flags[2].enabled && flags[2].value.as_deref() == Some("x y"));
        assert_eq!(parse_extra(&format_extra(&flags)), flags);
    }
}
