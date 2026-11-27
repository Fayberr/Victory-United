//! Tool output never carries secrets: the endpoint token, and anything in a log or a flag that is
//! written like a key (`..._API_KEY=...`, `Authorization: Bearer ...`).
use serde_json::Value;

pub const HIDDEN: &str = "[hidden]";
const NAMES: [&str; 8] = ["api_key", "apikey", "api-key", "secret", "token", "password", "passwd", "private_key"];

fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    NAMES.iter().any(|n| name.ends_with(n))
}

/// Hides every secret in the strings of `value`. `token` is the endpoint token, if there is one.
pub fn value(value: &mut Value, token: Option<&str>) {
    match value {
        Value::String(text) => *text = text_of(text, token),
        Value::Array(list) => list.iter_mut().for_each(|v| self::value(v, token)),
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s) if secret_name(key) && !s.is_empty() => *s = HIDDEN.into(),
                    _ => self::value(v, token),
                }
            }
        }
        _ => {}
    }
}

pub fn text_of(text: &str, token: Option<&str>) -> String {
    let mut text = match token {
        Some(t) if !t.is_empty() => text.replace(t, HIDDEN),
        _ => text.to_string(),
    };
    if !might_hold_secret(&text) {
        return text;
    }
    let trailing = text.ends_with('\n');
    text = text.lines().map(line).collect::<Vec<_>>().join("\n");
    if trailing {
        text.push('\n');
    }
    text
}

fn might_hold_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("bearer ") || NAMES.iter().any(|n| lower.contains(n))
}

fn line(line: &str) -> String {
    let bytes = line.as_bytes();
    let name_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    let mut out = String::new();
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !name_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && name_byte(bytes[i]) {
            i += 1;
        }
        let word = &line[start..i];
        let bearer = word.eq_ignore_ascii_case("bearer");
        if !bearer && !secret_name(word) {
            continue;
        }
        // What follows the name: quotes, blanks and one `=` or `:`, then the secret itself.
        let mut at = i;
        let mut assigned = bearer;
        while at < bytes.len() && matches!(bytes[at], b' ' | b'"' | b'\'' | b'=' | b':') {
            assigned |= matches!(bytes[at], b'=' | b':');
            at += 1;
        }
        if !assigned || at == i {
            continue;
        }
        let mut end = at;
        while end < bytes.len() && !matches!(bytes[end], b' ' | b'"' | b'\'' | b',' | b';' | b'&') {
            end += 1;
        }
        // `Authorization: Bearer x` is handled when the scan reaches `Bearer`.
        if end == at || line[at..end].eq_ignore_ascii_case("bearer") {
            continue;
        }
        out.push_str(&line[copied..at]);
        out.push_str(HIDDEN);
        copied = end;
        i = end;
    }
    out.push_str(&line[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_tokens_and_bearer_values_are_hidden() {
        for (input, expected) in [
            ("GEMINI_API_KEY=abc123 next", "GEMINI_API_KEY=[hidden] next"),
            ("Authorization: Bearer abc.def", "Authorization: Bearer [hidden]"),
            ("password: \"hunter2\", user=x", "password: \"[hidden]\", user=x"),
            ("client_secret=s3cr3t&grant=1", "client_secret=[hidden]&grant=1"),
            ("the token file is here: C:/Users/me/agent.json", "the token file is here: C:/Users/me/agent.json"),
            ("VICTORY_THIN_THREADS=6", "VICTORY_THIN_THREADS=6"),
            ("grün token=ü ok", "grün token=[hidden] ok"),
        ] {
            assert_eq!(text_of(input, None), expected);
        }
        assert_eq!(text_of("a\nb T0KEN c\n", Some("T0KEN")), "a\nb [hidden] c\n");
        let mut v = json!({ "token": "abc", "list": ["api_key=zzz"], "tokens": 5, "note": "ok" });
        value(&mut v, None);
        assert_eq!(v, json!({ "token": "[hidden]", "list": ["api_key=[hidden]"], "tokens": 5, "note": "ok" }));
    }
}
