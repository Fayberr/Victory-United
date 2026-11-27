//! The ops half's own small settings file (`ops.toml` next to `settings.toml`): overlay, update checks.
use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];
    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Corner::TopLeft => tr("ops.corner.tl"),
            Corner::TopRight => tr("ops.corner.tr"),
            Corner::BottomLeft => tr("ops.corner.bl"),
            Corner::BottomRight => tr("ops.corner.br"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OpsSettings {
    /// The small always-on-top fps overlay.
    pub overlay: bool,
    pub overlay_corner: Corner,
    /// `stable` or `dev`.
    pub channel: String,
    /// Look for an engine update once per launch (off by default).
    /// Which engine build to download: `release` or `diagnostics`.
    pub engine_kind: String,
    /// A developer's own victory.exe (Developer tab). Empty: the installed build is used.
    pub own_engine: String,
}

impl Default for OpsSettings {
    fn default() -> Self {
        Self {
            overlay: false,
            overlay_corner: Corner::TopLeft,
            channel: "stable".into(),
            engine_kind: "release".into(),
            own_engine: String::new(),
        }
    }
}

impl OpsSettings {
    pub fn file() -> std::path::PathBuf {
        paths::data_dir().join("ops.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::file()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(paths::data_dir());
        if let Ok(t) = toml::to_string_pretty(self) {
            let _ = std::fs::write(Self::file(), t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_defaults() {
        let s = OpsSettings { overlay: true, overlay_corner: Corner::BottomRight, ..Default::default() };
        let t = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<OpsSettings>(&t).unwrap(), s);
        // Missing keys fall back to defaults; unknown keys do not break loading.
        assert_eq!(toml::from_str::<OpsSettings>("overlay = true\nfuture = 1").unwrap().channel, "stable");
    }
}
