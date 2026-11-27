//! Launcher settings in `settings.toml` (game folder, active profile, language, engine override).
//! Plain data with load and save on explicit paths so tests can use temp folders.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    Dark,
    Light,
    #[default]
    System,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeChoice,
    pub scale_percent: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: ThemeChoice::System, scale_percent: 100 }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct Backups {
    pub root: Option<PathBuf>,
    pub before_launch: bool,
    pub after_exit: bool,
    pub retention: super::saves::Retention,
}

impl Default for Backups {
    fn default() -> Self {
        Self { root: None, before_launch: true, after_exit: true, retention: Default::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Folder that holds `MicrosoftGame.Config` and the game executable.
    pub game_folder: Option<PathBuf>,
    /// The game executable inside that folder.
    pub game_exe: Option<PathBuf>,
    /// A developer's own `victory.exe`. Empty means the installed engine.
    pub engine_override: Option<PathBuf>,
    /// Name of the active launch profile (file `profiles\<name>.toml`).
    pub active_profile: String,
    pub language: String,
    /// Folders the user chose for files the engine looks for, by environment variable name
    /// (for example `VICTORY_FIDELITYFX_DIR`).
    pub dirs: std::collections::BTreeMap<String, String>,
    /// The first-run walk-through was finished or skipped.
    pub setup_done: bool,
    pub appearance: Appearance,
    pub backups: Backups,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_folder: None,
            game_exe: None,
            engine_override: None,
            active_profile: "Default".into(),
            language: "en".into(),
            dirs: Default::default(),
            setup_done: false,
            appearance: Appearance::default(),
            backups: Backups::default(),
        }
    }
}

impl Settings {
    /// A missing or unreadable file gives the defaults (a broken settings file must never block starting).
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, text.as_bytes())
    }

    pub fn load() -> Self {
        Self::load_from(&crate::paths::settings_file())
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&crate::paths::settings_file())
    }
}

/// Writes through a temp file and a rename so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::tempdir;

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempdir("store");
        let file = dir.join("settings.toml");
        assert_eq!(Settings::load_from(&file), Settings::default());
        let s = Settings { game_folder: Some("D:/g".into()), setup_done: true, ..Settings::default() };
        s.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), s);
        std::fs::write(&file, "this is not toml [[[").unwrap();
        assert_eq!(Settings::load_from(&file), Settings::default());
        std::fs::write(&file, "setup_done = true\nunknown_key = 1\n").unwrap();
        assert!(Settings::load_from(&file).setup_done);
    }
}
