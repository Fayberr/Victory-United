//! NVIDIA DLSS as an opt-in modifier on top of a preset. Pure functions: the settings and the card go in,
//! the environment the engine reads comes out. Names and meanings follow docs/dlss.md of the engine branch
//! that has DLSS (`VICTORY_DLSS=1` is Streamline Super Resolution, `VICTORY_FRAMEGEN=dlss` is NVIDIA frame
//! generation). The launcher never ships, downloads or copies NVIDIA files; it only points at the folders
//! the user already has.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const STREAMLINE_DIR: &str = "VICTORY_STREAMLINE_DIR";
pub const SM86_DIR: &str = "VICTORY_DLSSG_SM86_DIR";

/// Every variable this module can write. The Settings tab owns them, so the raw flag list does not repeat them.
pub const OWNED: [&str; 5] = ["VICTORY_DLSS", "VICTORY_DLSS_MODE", "VICTORY_FRAMEGEN", STREAMLINE_DIR, SM86_DIR];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)] // DLSS calls its best-looking mode "Quality"
pub enum Quality {
    #[default]
    Quality,
    Balanced,
    Performance,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Quality, Quality::Balanced, Quality::Performance];

    /// The value the engine reads in `VICTORY_DLSS_MODE`.
    pub fn value(self) -> &'static str {
        match self {
            Quality::Quality => "quality",
            Quality::Balanced => "balanced",
            Quality::Performance => "performance",
        }
    }
}

/// What the person chose. Off by default.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct DlssSettings {
    pub enabled: bool,
    pub quality: Quality,
    pub frame_generation: bool,
}

/// What the graphics card can do with DLSS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    /// Not an NVIDIA RTX card: DLSS is not offered.
    NoRtx,
    /// RTX 20 or 30: Super Resolution works, frame generation needs the sm86 add-on folder.
    NeedsAddon,
    /// RTX 40 and newer (or an RTX card whose generation we cannot read): frame generation works without the add-on.
    Native,
}

/// Classify the card by name. A name with "RTX" from NVIDIA counts; the first four digit number tells the
/// generation (2060, 3080 Ti, 4090, 5090). RTX workstation names without such a number count as `Native`.
pub fn card(gpu_name: &str, vendor_id: u32) -> Card {
    let upper = gpu_name.to_ascii_uppercase();
    let nvidia = vendor_id == 0x10DE || upper.contains("NVIDIA");
    if !nvidia || !upper.contains("RTX") {
        return Card::NoRtx;
    }
    let digits = upper.split(|c: char| !c.is_ascii_digit()).find(|d| d.len() == 4).and_then(|d| d.parse::<u32>().ok());
    match digits {
        Some(n) if n < 4000 => Card::NeedsAddon,
        _ => Card::Native,
    }
}

impl Card {
    pub fn offered(self) -> bool {
        self != Card::NoRtx
    }
}

fn folder<'a>(dirs: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    dirs.get(name).map(|s| s.trim()).filter(|s| !s.is_empty())
}

/// Why frame generation would not be set right now, in plain words, or `None` when it would be.
pub fn frame_generation_blocker(s: &DlssSettings, card: Card, dirs: &BTreeMap<String, String>) -> Option<&'static str> {
    if !s.enabled {
        return Some("Turn on DLSS Super Resolution first.");
    }
    if folder(dirs, STREAMLINE_DIR).is_none() {
        return Some("Choose the NVIDIA Streamline folder first.");
    }
    if card == Card::NeedsAddon && folder(dirs, SM86_DIR).is_none() {
        return Some("This card needs the add-on folder for frame generation.");
    }
    None
}

/// The environment for the engine. Empty when DLSS is off, the card is not RTX, or the NVIDIA folder is
/// missing (the engine would only fall back to its own upscaler, so nothing is claimed). Frame generation
/// is added only when its prerequisites are there: the same folder, the add-on on RTX 20 and 30, and the
/// single-thread submit the engine documents for it (`VICTORY_THIN=1`).
pub fn flags(s: &DlssSettings, card: Card, dirs: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !s.enabled || !card.offered() {
        return out;
    }
    let Some(streamline) = folder(dirs, STREAMLINE_DIR) else { return out };
    out.push(("VICTORY_DLSS".into(), "1".into()));
    out.push(("VICTORY_DLSS_MODE".into(), s.quality.value().into()));
    out.push((STREAMLINE_DIR.into(), streamline.into()));
    if s.frame_generation && frame_generation_blocker(s, card, dirs).is_none() {
        out.push(("VICTORY_FRAMEGEN".into(), "dlss".into()));
        out.push(("VICTORY_THIN".into(), "1".into()));
        if card == Card::NeedsAddon
            && let Some(sm86) = folder(dirs, SM86_DIR) {
                out.push((SM86_DIR.into(), sm86.into()));
            }
    }
    out
}

/// The folder settings that belong to DLSS are only passed on through `flags`, so a chosen folder never
/// leaks into a launch where DLSS is off.
pub fn without_dlss_dirs(dirs: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    dirs.iter().filter(|(k, _)| k.as_str() != STREAMLINE_DIR && k.as_str() != SM86_DIR).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The folder check for the Settings tab: which of the files the engine loads are there. Only names the
/// docs list; nothing is read or copied.
pub fn missing_files(streamline: &std::path::Path, frame_generation: bool) -> Vec<&'static str> {
    let mut need = vec!["sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "nvngx_dlss.dll"];
    if frame_generation {
        need.extend(["sl.dlss_g.dll", "sl.reflex.dll", "sl.pcl.dll", "nvngx_dlssg.dll"]);
    }
    need.into_iter().filter(|n| !streamline.join(n).is_file()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    fn get(f: &[(String, String)], k: &str) -> Option<String> {
        f.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }
    fn on(quality: Quality, fg: bool) -> DlssSettings {
        DlssSettings { enabled: true, quality, frame_generation: fg }
    }

    #[test]
    fn off_by_default_and_off_writes_nothing() {
        assert!(!DlssSettings::default().enabled);
        let d = dirs(&[(STREAMLINE_DIR, "C:/sl"), (SM86_DIR, "C:/sm86")]);
        let off = DlssSettings { enabled: false, quality: Quality::Performance, frame_generation: true };
        assert!(flags(&off, Card::Native, &d).is_empty());
    }

    #[test]
    fn every_quality_mode_maps_to_its_value() {
        let d = dirs(&[(STREAMLINE_DIR, "C:/sl")]);
        for (q, v) in [(Quality::Quality, "quality"), (Quality::Balanced, "balanced"), (Quality::Performance, "performance")] {
            let f = flags(&on(q, false), Card::Native, &d);
            assert_eq!(get(&f, "VICTORY_DLSS").as_deref(), Some("1"));
            assert_eq!(get(&f, "VICTORY_DLSS_MODE").as_deref(), Some(v));
            assert_eq!(get(&f, STREAMLINE_DIR).as_deref(), Some("C:/sl"));
            assert!(get(&f, "VICTORY_FRAMEGEN").is_none());
        }
    }

    #[test]
    fn missing_or_blank_folder_sets_nothing() {
        assert!(flags(&on(Quality::Quality, false), Card::Native, &dirs(&[])).is_empty());
        assert!(flags(&on(Quality::Quality, false), Card::Native, &dirs(&[(STREAMLINE_DIR, "  ")])).is_empty());
    }

    #[test]
    fn not_offered_without_an_rtx_card() {
        let d = dirs(&[(STREAMLINE_DIR, "C:/sl")]);
        assert!(flags(&on(Quality::Quality, false), Card::NoRtx, &d).is_empty());
        assert_eq!(card("AMD Radeon RX 7900 XTX", 0x1002), Card::NoRtx);
        assert_eq!(card("NVIDIA GeForce GTX 1660", 0x10DE), Card::NoRtx);
        assert_eq!(card("Intel Arc A770", 0x8086), Card::NoRtx);
        assert!(!Card::NoRtx.offered());
    }

    #[test]
    fn card_generations() {
        assert_eq!(card("NVIDIA GeForce RTX 2060", 0x10DE), Card::NeedsAddon);
        assert_eq!(card("NVIDIA GeForce RTX 3080 Ti", 0x10DE), Card::NeedsAddon);
        assert_eq!(card("NVIDIA GeForce RTX 4070 Laptop GPU", 0x10DE), Card::Native);
        assert_eq!(card("NVIDIA GeForce RTX 5090", 0x10DE), Card::Native);
        assert_eq!(card("NVIDIA RTX A4000", 0x10DE), Card::Native);
    }

    #[test]
    fn frame_generation_needs_its_prerequisites() {
        let sl = dirs(&[(STREAMLINE_DIR, "C:/sl")]);
        let both = dirs(&[(STREAMLINE_DIR, "C:/sl"), (SM86_DIR, "C:/sm86")]);
        let s = on(Quality::Balanced, true);
        // RTX 40: only the Streamline folder is needed, and the add-on folder is not passed on.
        let f = flags(&s, Card::Native, &both);
        assert_eq!(get(&f, "VICTORY_FRAMEGEN").as_deref(), Some("dlss"));
        assert_eq!(get(&f, "VICTORY_THIN").as_deref(), Some("1"));
        assert!(get(&f, SM86_DIR).is_none());
        assert!(frame_generation_blocker(&s, Card::Native, &sl).is_none());
        // RTX 30 without the add-on: Super Resolution stays, frame generation does not.
        let f = flags(&s, Card::NeedsAddon, &sl);
        assert!(get(&f, "VICTORY_DLSS").is_some() && get(&f, "VICTORY_FRAMEGEN").is_none() && get(&f, "VICTORY_THIN").is_none());
        assert!(frame_generation_blocker(&s, Card::NeedsAddon, &sl).is_some());
        // RTX 30 with the add-on.
        let f = flags(&s, Card::NeedsAddon, &both);
        assert_eq!(get(&f, "VICTORY_FRAMEGEN").as_deref(), Some("dlss"));
        assert_eq!(get(&f, SM86_DIR).as_deref(), Some("C:/sm86"));
        // Frame generation asked while Super Resolution is off: nothing at all.
        let s = DlssSettings { enabled: false, quality: Quality::Quality, frame_generation: true };
        assert!(flags(&s, Card::Native, &both).is_empty());
        assert!(frame_generation_blocker(&s, Card::Native, &both).is_some());
    }

    #[test]
    fn owned_names_cover_everything_flags_writes() {
        let d = dirs(&[(STREAMLINE_DIR, "a"), (SM86_DIR, "b")]);
        for card in [Card::NeedsAddon, Card::Native] {
            for (k, _) in flags(&on(Quality::Quality, true), card, &d) {
                assert!(OWNED.contains(&k.as_str()) || k == "VICTORY_THIN", "{k}");
            }
        }
    }

    #[test]
    fn dlss_dirs_are_stripped_from_plain_dirs() {
        let d = dirs(&[(STREAMLINE_DIR, "a"), (SM86_DIR, "b"), ("VICTORY_FIDELITYFX_DIR", "c")]);
        let s = without_dlss_dirs(&d);
        assert_eq!(s.len(), 1);
        assert!(s.contains_key("VICTORY_FIDELITYFX_DIR"));
    }

    #[test]
    fn missing_files_lists_what_the_folder_lacks() {
        let tmp = std::env::temp_dir().join(format!("vu-dlss-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(missing_files(&tmp, false).len(), 4);
        for n in ["sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "nvngx_dlss.dll"] {
            std::fs::write(tmp.join(n), b"").unwrap();
        }
        assert!(missing_files(&tmp, false).is_empty());
        assert_eq!(missing_files(&tmp, true).len(), 4);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn settings_round_trip_and_old_profiles_load() {
        let s = on(Quality::Performance, true);
        let text = toml::to_string(&s).unwrap();
        assert_eq!(toml::from_str::<DlssSettings>(&text).unwrap(), s);
        assert_eq!(toml::from_str::<DlssSettings>("").unwrap(), DlssSettings::default());
    }
}
