//! Release manifest, download, SHA-256 verification, install and rollback for the engine.
//! No UI code here: the launcher calls into this crate from worker threads.
//!
//! Layout it manages (see docs/launcher-contract.md):
//!
//! ```text
//! engine\current.txt          name of the active build folder
//! engine\previous.txt         the build that was active before the last switch (rollback target)
//! engine\<name>\victory.exe   one folder per installed build, `<short commit>-<kind>`
//! ```
//!
//! Steps of an update: [`check`] reads the manifest of a GitHub release, [`download`] fetches the ZIP
//! into a `.part` file and verifies size and SHA-256 before it becomes a real file, [`install`]
//! unpacks into `engine\<name>.partial`, renames that folder to its final name and only then writes
//! `current.txt` (through a temporary file and a rename), so an interrupted install leaves the
//! active build untouched. [`rollback`] flips back, [`cleanup`] removes old builds.
//!
//! Network: [`http::Http`] is a small trait. On Windows it is WinHTTP (the system's TLS and proxy
//! settings, no C toolchain needed); tests use a fake.

pub mod http;
mod install;
mod manifest;

pub use install::{Installed, cleanup, download, install, installed, rollback, set_current, sha256_file, superseded};
pub use manifest::{Channel, Engine, Manifest, Update, check, folder_name, parse_release_assets};

pub const REPO: &str = "Fayberr/Victory-United";

#[derive(Debug)]
pub enum Error {
    /// No network, or the server did not answer.
    Offline(String),
    /// The server answered with something we do not understand or an error code.
    Remote(String),
    BadManifest(String),
    /// The download does not match the manifest (size or SHA-256).
    Corrupt(String),
    /// The ZIP is not a usable engine build.
    BadArchive(String),
    Io(std::io::Error),
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Offline(m) => write!(f, "cannot reach the server: {m}"),
            Error::Remote(m) => write!(f, "the server answered unexpectedly: {m}"),
            Error::BadManifest(m) => write!(f, "the release manifest is not valid: {m}"),
            Error::Corrupt(m) => write!(f, "the download is damaged: {m}"),
            Error::BadArchive(m) => write!(f, "the engine package is not usable: {m}"),
            Error::Io(e) => write!(f, "file error: {e}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
