//! Helpers for unit tests.
use std::path::PathBuf;

/// A fresh empty folder under the system temp dir, unique per process and thread.
pub fn tempdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vl-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
