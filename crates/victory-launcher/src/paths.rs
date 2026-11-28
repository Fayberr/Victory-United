//! Where things live. Part of the contract, see docs/launcher-contract.md ("Folders").
//! The launcher is portable: the engine sits next to the launcher exe, user data under %APPDATA%.
use crate::core::store::{Settings, StorageSettings};
use std::{path::PathBuf, sync::RwLock};

static STORAGE: RwLock<StorageSettings> = RwLock::new(StorageSettings {
    cache: None, data: None, verify_cache_by_hash: false,
});

pub fn configure_storage(settings: &Settings) {
    *STORAGE.write().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.storage.clone();
}

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
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|p| PathBuf::from(p).join("Victory-United")))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share/Victory-United")))
        .unwrap_or_else(|| std::env::temp_dir().join("Victory-United"))
}

pub fn settings_file() -> PathBuf { data_dir().join("settings.toml") }
pub fn profiles_dir() -> PathBuf { data_dir().join("profiles") }
pub fn logs_dir() -> PathBuf { data_dir().join("logs") }
pub fn cache_root() -> PathBuf {
    STORAGE.read().unwrap_or_else(std::sync::PoisonError::into_inner).cache.clone().unwrap_or_else(|| data_dir().join("cache"))
}
pub fn game_data_root() -> PathBuf {
    STORAGE.read().unwrap_or_else(std::sync::PoisonError::into_inner).data.clone().unwrap_or_else(|| data_dir().join("data"))
}
pub fn backups_root(settings: &crate::core::store::Settings) -> PathBuf {
    settings.backups.root.clone().unwrap_or_else(|| data_dir().join("backups"))
}
pub fn move_journal() -> PathBuf { data_dir().join("move-journal.toml") }
pub fn reports_dir() -> PathBuf { data_dir().join("reports") }

/// `<launcher folder>\engine`: one sub folder per installed engine build plus `current.txt`.
pub fn engine_root() -> PathBuf { app_dir().join("engine") }

/// Extra launch flags (Flags tab), next to `settings.toml`.
pub fn launch_flags_file() -> PathBuf { data_dir().join("launch-flags.txt") }
