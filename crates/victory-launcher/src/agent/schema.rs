//! The arguments of a tool: described once, turned into a JSON schema, and checked against that schema
//! before the tool runs. Only the parts of JSON Schema the tool table uses are supported.
use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
pub enum Kind {
    Text,
    Bool,
    /// Whole number with bounds.
    Int(i64, i64),
    Choice(&'static [&'static str]),
    /// An object whose keys are checked by the tool itself.
    Object,
    TextList,
    /// Sampling windows: a list of `{ "from": seconds, "to": seconds }`.
    Windows,
}

#[derive(Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub help: &'static str,
}

pub const fn opt(name: &'static str, kind: Kind, help: &'static str) -> Param {
    Param {
        name,
        kind,
        required: false,
        help,
    }
}

pub const fn req(name: &'static str, kind: Kind, help: &'static str) -> Param {
    Param {
        name,
        kind,
        required: true,
        help,
    }
}

fn kind_schema(kind: Kind) -> Value {
    match kind {
        Kind::Text => json!({ "type": "string" }),
        Kind::Bool => json!({ "type": "boolean" }),
        Kind::Int(min, max) => json!({ "type": "integer", "minimum": min, "maximum": max }),
        Kind::Choice(choices) => json!({ "type": "string", "enum": choices }),
        Kind::Object => json!({ "type": "object" }),
        Kind::TextList => json!({ "type": "array", "items": { "type": "string" } }),
        Kind::Windows => json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "from": { "type": "number", "minimum": 0, "maximum": 86400 },
                    "to": { "type": "number", "minimum": 0, "maximum": 86400 }
                },
                "required": ["from", "to"],
                "additionalProperties": false
            }
        }),
    }
}

/// The `inputSchema` of a tool.
pub fn object(params: &[Param]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for p in params {
        let mut schema = kind_schema(p.kind);
        schema["description"] = json!(p.help);
        properties.insert(p.name.into(), schema);
        if p.required {
            required.push(p.name);
        }
    }
    let mut schema =
        json!({ "type": "object", "properties": properties, "additionalProperties": false });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

/// Checks `value` against `schema`. The error names the argument.
pub fn check(schema: &Value, value: &Value) -> Result<(), String> {
    check_at(schema, value, "arguments")
}

fn check_at(schema: &Value, value: &Value, at: &str) -> Result<(), String> {
    if let Some(kind) = schema["type"].as_str() {
        let fits = match kind {
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            other => return Err(format!("{at}: the schema uses an unsupported type {other}")),
        };
        if !fits {
            let article = if kind.starts_with(['a', 'i', 'o']) {
                "an"
            } else {
                "a"
            };
            return Err(format!("{at} must be {article} {kind}"));
        }
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            let list: Vec<&str> = choices.iter().filter_map(Value::as_str).collect();
            return Err(format!("{at} must be one of: {}", list.join(", ")));
        }
    }
    if let Some(n) = value.as_f64() {
        if schema["minimum"].as_f64().is_some_and(|min| n < min)
            || schema["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return Err(format!(
                "{at} must be from {} to {}",
                schema["minimum"], schema["maximum"]
            ));
        }
    }
    if let Some(object) = value.as_object() {
        for name in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(name) {
                return Err(format!("{at} needs \"{name}\""));
            }
        }
        if let Some(properties) = schema["properties"].as_object() {
            for (name, inner) in object {
                match properties.get(name) {
                    Some(sub) => check_at(sub, inner, name)?,
                    None if schema["additionalProperties"] == json!(false) => {
                        let known: Vec<&str> = properties.keys().map(String::as_str).collect();
                        let known = if known.is_empty() {
                            "none".into()
                        } else {
                            known.join(", ")
                        };
                        return Err(format!(
                            "{at}: \"{name}\" is not an argument (arguments: {known})"
                        ));
                    }
                    None => {}
                }
            }
        }
    }
    if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
        for (i, entry) in list.iter().enumerate() {
            check_at(items, entry, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_types_bounds_and_unknown_names() {
        let schema = object(&[
            req("name", Kind::Text, "n"),
            opt("lines", Kind::Int(1, 10), "l"),
            opt("mode", Kind::Choice(&["a", "b"]), "m"),
            opt("windows", Kind::Windows, "w"),
        ]);
        assert!(check(&schema, &json!({ "name": "x", "lines": 10, "mode": "a", "windows": [{ "from": 1, "to": 2.5 }] })).is_ok());
        for bad in [
            json!({}),
            json!({ "name": 1 }),
            json!({ "name": "x", "lines": 0 }),
            json!({ "name": "x", "lines": 1.5 }),
            json!({ "name": "x", "mode": "c" }),
            json!({ "name": "x", "extra": 1 }),
            json!({ "name": "x", "windows": [{ "from": 1 }] }),
            json!([]),
        ] {
            assert!(check(&schema, &bad).is_err(), "{bad}");
        }
    }
}
