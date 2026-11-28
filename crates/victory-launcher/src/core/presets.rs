//! Presets and options as data (`data/presets.toml`, embedded). Nothing about machine classes is
//! hard-coded in Rust: add a preset by editing the file.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

pub type Env = BTreeMap<String, String>;

#[derive(Deserialize, Default, Clone, Debug)]
pub struct Layer {
    #[serde(default)]
    pub env: Env,
    /// Applied when safe mode is off.
    #[serde(default)]
    pub fast: Env,
    /// Applied when safe mode is on.
    #[serde(default)]
    pub safe: Env,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub inherits: Option<String>,
    #[serde(default)]
    pub env: Env,
    #[serde(default)]
    pub fast: Env,
    #[serde(default)]
    pub safe: Env,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AutoRule {
    pub gpu_contains: Option<String>,
    pub vendor: Option<String>,
    pub preset: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Toggle {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default: bool,
    pub on: Env,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Upscaler {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub reason: Option<String>,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub env: Env,
    pub files_env: Option<String>,
    pub files_note: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AutoFallback {
    pub preset: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PresetFile {
    pub common: Layer,
    pub auto: Vec<AutoRule>,
    pub auto_fallback: AutoFallback,
    pub preset: Vec<Preset>,
    pub toggle: Vec<Toggle>,
    pub upscaler: Vec<Upscaler>,
}

const SOURCE: &str = include_str!("../../data/presets.toml");

pub fn parse(text: &str) -> Result<PresetFile, toml::de::Error> {
    toml::from_str(text)
}

pub fn builtin() -> &'static PresetFile {
    static FILE: OnceLock<PresetFile> = OnceLock::new();
    FILE.get_or_init(|| parse(SOURCE).expect("data/presets.toml parses"))
}

impl PresetFile {
    pub fn preset(&self, id: &str) -> Option<&Preset> {
        self.preset.iter().find(|p| p.id == id)
    }

    /// The preset "Automatic" picks for this card (same rules as scripts/play.ps1).
    pub fn auto_for(&self, gpu_name: &str, vendor_id: u32) -> &Preset {
        let nvidia = vendor_id == 0x10DE || gpu_name.to_ascii_uppercase().contains("NVIDIA");
        let id = self
            .auto
            .iter()
            .find(|r| {
                r.gpu_contains.as_ref().is_none_or(|g| gpu_name.to_ascii_lowercase().contains(&g.to_ascii_lowercase()))
                    && r.vendor.as_ref().is_none_or(|v| v == "nvidia" && nvidia)
            })
            .map_or(&self.auto_fallback.preset, |r| &r.preset);
        self.preset(id).expect("auto rules name existing presets")
    }

    /// Values of a preset: common, then what it inherits (oldest first), then itself; each with its
    /// always, then fast-or-safe values.
    pub fn preset_env(&self, id: &str, safe: bool) -> Env {
        let mut chain = Vec::new();
        let mut at = self.preset(id);
        while let Some(p) = at {
            chain.push(p);
            at = p.inherits.as_deref().and_then(|i| self.preset(i));
            if chain.len() > 8 {
                break; // a loop in the data
            }
        }
        let mut env = Env::new();
        let apply = |env: &mut Env, l: &Layer| {
            env.extend(l.env.clone());
            env.extend(if safe { l.safe.clone() } else { l.fast.clone() });
        };
        apply(&mut env, &self.common);
        for p in chain.into_iter().rev() {
            env.extend(p.env.clone());
            env.extend(if safe { p.safe.clone() } else { p.fast.clone() });
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_parses_and_references_resolve() {
        let f = parse(SOURCE).unwrap();
        for r in &f.auto {
            assert!(f.preset(&r.preset).is_some(), "auto rule names {}", r.preset);
        }
        assert!(f.preset(&f.auto_fallback.preset).is_some());
        for p in &f.preset {
            if let Some(i) = &p.inherits {
                assert!(f.preset(i).is_some(), "{} inherits unknown {i}", p.id);
            }
            for safe in [false, true] {
                let env = f.preset_env(&p.id, safe);
                assert!(!env.is_empty());
                for k in env.keys() {
                    assert!(k.starts_with("VICTORY_"), "{k}");
                }
            }
        }
        let ids: std::collections::HashSet<_> = f.preset.iter().map(|p| &p.id).collect();
        assert_eq!(ids.len(), f.preset.len(), "preset ids are unique");
        assert!(f.upscaler.iter().any(|u| u.id == "off"));
        for u in &f.upscaler {
            if !u.available {
                assert!(u.reason.is_some(), "{} is unavailable without a reason", u.id);
            }
        }
    }

    #[test]
    fn auto_picks_like_play_ps1() {
        let f = builtin();
        assert_eq!(f.auto_for("NVIDIA GeForce RTX 3080 Ti", 0x10DE).id, "3080ti");
        assert_eq!(f.auto_for("NVIDIA GeForce RTX 4090", 0x10DE).id, "4090");
        assert_eq!(f.auto_for("NVIDIA GeForce RTX 4070", 0x10DE).id, "4090");
        assert_eq!(f.auto_for("AMD Radeon RX 7900 XTX", 0x1002).id, "7900xtx");
        assert_eq!(f.auto_for("", 0).id, "7900xtx");
    }

    #[test]
    fn presets_match_the_script_values() {
        let f = builtin();
        let e = f.preset_env("3080ti", false);
        assert_eq!(e["VICTORY_THIN_THREADS"], "6");
        assert_eq!(e["VICTORY_TEXTURE_ROOM_PERCENT"], "85");
        assert_eq!(e["VICTORY_MAP_THREADS"], "auto"); // inherited from the 4090 block, as in play.ps1
        assert_eq!(e["VICTORY_THIN"], "1");
        let s = f.preset_env("3080ti", true);
        assert!(!s.contains_key("VICTORY_TEXTURE_ROOM_PERCENT"));
        assert_eq!(s["VICTORY_UNDER_WIDE"], "0");
        assert!(!s.contains_key("VICTORY_THIN"));
        let e = f.preset_env("4070", false);
        assert_eq!(e["VICTORY_MAP_THREADS"], "1"); // the 4070 block overrides the 4090's
        assert_eq!(f.preset_env("7900xtx", false)["VICTORY_CARD_AWAKE"], "1");
    }
}
