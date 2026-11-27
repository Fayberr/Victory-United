//! Text lookup. Every string the user sees goes through `tr()` so more languages can be added later.
//! Locale files are flat JSON objects, one per owner and language: `locales/<lang>/core.json` and
//! `locales/<lang>/ops.json` (separate files so the two workers never edit the same file).
//! English is the only language for now; a missing key shows the key itself.
use std::{collections::HashMap, sync::OnceLock};

const SOURCES: &[&str] = &[
    include_str!("../locales/en/core.json"),
    include_str!("../locales/en/ops.json"),
];

fn table() -> &'static HashMap<String, &'static str> {
    static TABLE: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for src in SOURCES {
            let parsed: HashMap<String, String> =
                serde_json::from_str(src).expect("locale file is a flat JSON object of strings");
            for (k, v) in parsed {
                map.insert(k, &*Box::leak(v.into_boxed_str()));
            }
        }
        map
    })
}

/// Translated text for `key`, or the key itself when it is missing.
pub fn tr(key: &'static str) -> &'static str {
    table().get(key).copied().unwrap_or(key)
}

/// Like `tr`, with `{name}` placeholders replaced.
pub fn trf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key).to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_files_parse_and_have_no_duplicate_keys_across_owners() {
        let mut seen = std::collections::HashSet::new();
        for src in SOURCES {
            let parsed: HashMap<String, String> = serde_json::from_str(src).unwrap();
            for k in parsed.keys() {
                assert!(seen.insert(k.clone()), "duplicate locale key {k}");
            }
        }
    }

    #[test]
    fn missing_key_returns_itself_and_placeholders_work() {
        assert_eq!(tr("no.such.key"), "no.such.key");
        assert_eq!(trf("no.{x}.key", &[("x", "y")]), "no.y.key");
    }
}
