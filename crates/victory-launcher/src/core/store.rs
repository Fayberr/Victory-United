//! Launcher settings in `settings.toml` (game folder, active profile, language, engine override).
//! Plain data with load and save on explicit paths so tests can use temp folders.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    /// The first-run walk-through was finished or skipped.
    pub setup_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_folder: None,
            game_exe: None,
            engine_override: None,
            active_profile: "Default".into(),
            language: "en".into(),
            setup_done: false,
        }
    }
}

impl Settings {
    /// A missing or unreadable file gives the defaults (a broken settings file must never block starting).
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        write_atomic(path, toml::to_string_pretty(self).unwrap_or_default().as_bytes())
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
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
