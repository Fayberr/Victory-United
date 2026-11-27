//! Where things live. Part of the contract, see docs/launcher-contract.md ("Folders").
//! The launcher is portable: the engine sits next to the launcher exe, user data under %APPDATA%.
use std::path::PathBuf;

/// Folder of the launcher executable (falls back to the current directory).
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `%APPDATA%\Victory-United`: settings, profiles, logs, caches. Never holds game files.
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("Victory-United"))
        .unwrap_or_else(|| PathBuf::from("Victory-United-data"))
}

pub fn settings_file() -> PathBuf { data_dir().join("settings.toml") }
pub fn profiles_dir() -> PathBuf { data_dir().join("profiles") }
pub fn logs_dir() -> PathBuf { data_dir().join("logs") }
pub fn cache_root() -> PathBuf { data_dir().join("cache") }
pub fn reports_dir() -> PathBuf { data_dir().join("reports") }

/// `<launcher folder>\engine`: one sub folder per installed engine build plus `current.txt`.
pub fn engine_root() -> PathBuf { app_dir().join("engine") }

/// The engine build folder name named in `engine\current.txt`, if any.
pub fn current_engine_name() -> Option<String> {
    let text = std::fs::read_to_string(engine_root().join("current.txt")).ok()?;
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `engine\<current>\victory.exe` when it exists. A developer's own build can override this in Settings.
pub fn active_engine_exe() -> Option<PathBuf> {
    let exe = engine_root().join(current_engine_name()?).join("victory.exe");
    exe.is_file().then_some(exe)
}

/// Name of the shared-memory block the engine process `pid` publishes (see victory-stats).
pub fn stats_block_name(pid: u32) -> String { format!("Local\\VictoryUnited.Stats.{pid}") }
