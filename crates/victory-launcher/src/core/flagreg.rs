//! The flag registry (`data/flags.toml`, embedded): every `VICTORY_*` variable the code reads, with type,
//! default, risk and a one-line description. The roughly fifty important ones are documented and have a
//! `group`, which puts them on the Settings tab; the rest have `doc = ""`. Regenerate the list of names with
//! `tools/gen_flags.py` (it keeps existing entries), then `tools/doc_flags.py`.
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Safe,
    Advanced,
    Experimental,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Safe => "safe",
            Risk::Advanced => "advanced",
            Risk::Experimental => "experimental",
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Switch,
    Number,
    Text,
    Choice,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Flag {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub default: String,
    /// For switches: `"0"` writes 0 when turned off, `"remove"` takes the variable out of the environment.
    pub off: Option<String>,
    /// The engine only checks that the variable exists (generated guess).
    #[serde(default)]
    pub presence: bool,
    pub group: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
    pub risk: Risk,
    #[serde(default)]
    pub doc: String,
}

impl Flag {
    /// Switch turned off: write this value, or `None` to remove the variable. Presence-only flags are
    /// always removed, never set to 0 (the engine would still read them as on).
    pub fn off_value(&self) -> Option<&'static str> {
        if self.presence || self.off.as_deref() != Some("0") { None } else { Some("0") }
    }
}

#[derive(Deserialize)]
struct File {
    flag: Vec<Flag>,
}

pub struct Registry {
    pub flags: Vec<Flag>,
    index: HashMap<String, usize>,
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&Flag> {
        self.index.get(name).map(|&i| &self.flags[i])
    }

    pub fn documented(&self) -> impl Iterator<Item = &Flag> {
        self.flags.iter().filter(|f| !f.doc.is_empty())
    }

    /// Flags for the Settings tab in one group, in file order.
    pub fn group<'a>(&'a self, group: &'a str) -> impl Iterator<Item = &'a Flag> {
        self.flags.iter().filter(move |f| f.group.as_deref() == Some(group) && !f.doc.is_empty())
    }
}

const SOURCE: &str = include_str!("../../data/flags.toml");

pub fn parse(text: &str) -> Result<Registry, toml::de::Error> {
    let file: File = toml::from_str(text)?;
    let index = file.flag.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    Ok(Registry { flags: file.flag, index })
}

pub fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| parse(SOURCE).expect("data/flags.toml parses"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, path::Path};

    fn walk(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                // The launcher's own tests use made-up names.
                if name == "target" || name == "victory-launcher" || name.starts_with('.') {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let b = text.as_bytes();
                let mut i = 0;
                while let Some(at) = text[i..].find("VICTORY_") {
                    let start = i + at;
                    let mut end = start + 8;
                    while end < b.len() && (b[end].is_ascii_uppercase() || b[end].is_ascii_digit() || b[end] == b'_') {
                        end += 1;
                    }
                    if end > start + 8 {
                        out.insert(text[start..end].trim_end_matches('_').to_string());
                    }
                    i = end;
                }
            }
        }
    }

    #[test]
    fn flag_registry_covers_the_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut seen = BTreeSet::new();
        for sub in ["crates", "apps"] {
            walk(&root.join(sub), &mut seen);
        }
        assert!(seen.len() > 300, "the walk found only {} flags, is the path right?", seen.len());
        let reg = registry();
        let missing: Vec<_> = seen.iter().filter(|n| reg.get(n).is_none()).collect();
        assert!(
            missing.is_empty(),
            "VICTORY_* flags in the code but not in crates/victory-launcher/data/flags.toml: {missing:?}\nRun tools/gen_flags.py from the repository root."
        );
    }

    #[test]
    fn registry_is_well_formed() {
        let reg = registry();
        let mut names = BTreeSet::new();
        for f in &reg.flags {
            assert!(names.insert(&f.name), "duplicate {}", f.name);
            assert!(f.name.starts_with("VICTORY_"));
            if f.kind == Kind::Choice {
                assert!(!f.choices.is_empty(), "{} has no choices", f.name);
            }
            if let (Some(lo), Some(hi)) = (f.min, f.max) {
                assert!(lo <= hi, "{}", f.name);
            }
            if f.group.is_some() {
                assert!(!f.doc.is_empty(), "{} is on the Settings tab without a description", f.name);
                assert!(!f.doc.contains('\u{2014}'));
            }
        }
        let documented = reg.documented().count();
        assert!((30..=70).contains(&documented), "{documented} documented");
        // every value the presets set is a known flag
        for p in &super::super::presets::builtin().preset {
            for k in p.env.keys().chain(p.fast.keys()).chain(p.safe.keys()) {
                assert!(reg.get(k).is_some(), "preset {} sets unknown {k}", p.id);
            }
        }
        assert_eq!(reg.get("VICTORY_UNDER_WIDE").unwrap().off_value(), Some("0"));
        assert_eq!(reg.get("VICTORY_FULLSCREEN").unwrap().off_value(), None);
    }
}
