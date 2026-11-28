//! The release manifest (`manifest.json`, an asset of every GitHub release) and finding it.
use crate::{Error, REPO, Result, http::Http};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Highest `schema` this code understands. A larger number means a newer launcher is needed.
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    /// Version of the launcher shipped in the same release.
    pub launcher_version: String,
    /// Where to get that launcher (page of the release); shown as a link only.
    #[serde(default)]
    pub launcher_url: String,
    /// `stable` and `dev`.
    pub channels: BTreeMap<String, Channel>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Channel {
    pub engines: Vec<Engine>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Engine {
    /// Commit the engine was built from.
    pub build: String,
    /// Version of the shader translator inside it.
    #[serde(default)]
    pub translator: String,
    /// `release` or `diagnostics`.
    pub kind: String,
    pub url: String,
    /// Lower case hex SHA-256 of the ZIP.
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub notes: String,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error::BadManifest(e.to_string()))?;
        if m.schema == 0 || m.schema > SCHEMA {
            return Err(Error::BadManifest(format!(
                "schema {} is not known to this launcher (it understands up to {SCHEMA}), a newer launcher is needed",
                m.schema
            )));
        }
        for c in m.channels.values() {
            for e in &c.engines {
                if e.build.is_empty() || e.url.is_empty() || e.sha256.len() != 64 || !e.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(Error::BadManifest(format!("engine entry {:?} is incomplete", e.build)));
                }
            }
        }
        Ok(m)
    }

    /// The engine of `kind` in `channel`.
    pub fn engine(&self, channel: &str, kind: &str) -> Option<&Engine> {
        self.channels.get(channel)?.engines.iter().find(|e| e.kind == kind)
    }
}

/// Name of the folder an engine installs into: `<first 8 of the commit>-<kind>`.
pub fn folder_name(e: &Engine) -> String {
    let short: String = e.build.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    format!("{short}-{}", e.kind)
}

/// Result of a check.
#[derive(Clone, Debug)]
pub struct Update {
    pub manifest: Manifest,
    pub engine: Engine,
    /// Folder the engine would install into.
    pub folder: String,
    /// False when that folder is already installed.
    pub newer: bool,
}

/// Asks GitHub for the manifest of `channel` and picks the engine of `kind`.
/// `have` is the list of installed folder names.
pub fn check(http: &dyn Http, channel: &str, kind: &str, have: &[String]) -> Result<Update> {
    let url = if channel == "dev" {
        format!("https://api.github.com/repos/{REPO}/releases?per_page=10")
    } else {
        format!("https://api.github.com/repos/{REPO}/releases/latest")
    };
    let listing = http.get_text(&url)?;
    let manifest_url = parse_release_assets(&listing, channel == "dev")?;
    let manifest = Manifest::parse(&http.get_text(&manifest_url)?)?;
    let engine = manifest
        .engine(channel, kind)
        .cloned()
        .ok_or_else(|| Error::BadManifest(format!("no {kind} engine in channel {channel}")))?;
    let folder = folder_name(&engine);
    let newer = !have.contains(&folder);
    Ok(Update { manifest, engine, folder, newer })
}

/// The `manifest.json` download URL out of GitHub's answer: one release object (stable, the latest)
/// or a list (dev: the newest pre-release).
pub fn parse_release_assets(json: &str, dev: bool) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| Error::Remote(e.to_string()))?;
    let release = match &v {
        serde_json::Value::Array(list) => list
            .iter()
            .find(|r| r["prerelease"].as_bool().unwrap_or(false) == dev && !r["draft"].as_bool().unwrap_or(false))
            .ok_or_else(|| Error::Remote("no matching release published yet".into()))?,
        one => one,
    };
    release["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == "manifest.json"))
        .and_then(|x| x["browser_download_url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| Error::Remote("the release has no manifest.json".into()))
}
