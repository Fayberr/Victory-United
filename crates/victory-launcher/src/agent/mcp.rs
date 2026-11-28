//! MCP over JSON-RPC 2.0: `initialize`, `ping`, `tools/list`, `tools/call`. The same code answers HTTP
//! requests and the stdio transport. Answers are plain JSON, no event stream.
use super::{Backend, tools};
use serde_json::{Value, json};
use std::sync::Arc;

/// Protocol versions this server speaks, newest first.
pub const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const MAX_BATCH: usize = 50;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// The launcher cannot take the request (used by the stdio proxy).
pub const UNAVAILABLE: i64 = -32000;

const INSTRUCTIONS: &str = "Victory United Launcher starts the Victory United engine and manages its settings, \
profiles, launch flags, saves, storage, engine builds, profiler and live statistics. Start with `status`. \
Tools that delete, overwrite, move or stop something only describe what they would do until they are \
called again with \"confirm\": true; ask the person before confirming anything they did not ask for.";

/// What goes back: an HTTP status and, unless the message needed no answer, a body.
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn handle(backend: &Arc<Backend>, body: &[u8]) -> Reply {
    respond(body, |_, method, params| match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::listing() })),
        "tools/call" => call(backend, params),
        _ => Err((METHOD_NOT_FOUND, "Method not found".into())),
    })
}

pub fn respond(
    body: &[u8],
    dispatch: impl Fn(&Value, &str, &Value) -> Result<Value, (i64, String)>,
) -> Reply {
    let fail = |code, message| Reply {
        status: 400,
        body: Some(error(Value::Null, code, message)),
    };
    let Ok(message) = serde_json::from_slice::<Value>(body) else {
        return fail(PARSE_ERROR, "Parse error");
    };
    match message {
        Value::Array(batch) => {
            if batch.is_empty() || batch.len() > MAX_BATCH {
                return fail(INVALID_REQUEST, "A batch needs 1 to 50 messages");
            }
            let answers: Vec<Value> = batch
                .into_iter()
                .filter_map(|m| one(m, &dispatch))
                .collect();
            if answers.is_empty() {
                Reply {
                    status: 202,
                    body: None,
                }
            } else {
                Reply {
                    status: 200,
                    body: Some(Value::Array(answers)),
                }
            }
        }
        Value::Object(_) => match one(message, &dispatch) {
            Some(answer) => Reply {
                status: 200,
                body: Some(answer),
            },
            None => Reply {
                status: 202,
                body: None,
            },
        },
        _ => fail(INVALID_REQUEST, "Invalid request"),
    }
}

/// One message. `None` for notifications and for answers a client sends back.
fn one(
    message: Value,
    dispatch: &impl Fn(&Value, &str, &Value) -> Result<Value, (i64, String)>,
) -> Option<Value> {
    let Value::Object(message) = message else {
        return Some(error(Value::Null, INVALID_REQUEST, "Invalid request"));
    };
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to something we never asked: nothing to say. Anything else is malformed.
        if message.contains_key("result") || message.contains_key("error") {
            return None;
        }
        return Some(error(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "Invalid request",
        ));
    };
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }
    // Without an id it is a notification: never answered.
    let id = id?;
    if !(id.is_string() || id.is_number()) {
        return Some(error(
            Value::Null,
            INVALID_REQUEST,
            "The id must be a string or a number",
        ));
    }
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match dispatch(&id, method, &params) {
        Ok(value) => result(id, value),
        Err((code, message)) => error(id, code, &message),
    })
}

pub fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| VERSIONS.contains(v))
        .unwrap_or(VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "victory-launcher", "title": "Victory United Launcher", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn call(backend: &Arc<Backend>, params: &Value) -> Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((INVALID_PARAMS, "tools/call needs a tool name".into()));
    };
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(value) => value.clone(),
    };
    match tools::call(backend, name, &arguments) {
        Ok(done) => Ok(json!({
            "content": [{ "type": "text", "text": done.text }],
            "isError": done.failed,
        })),
        Err(tools::Unknown) => Err((INVALID_PARAMS, "There is no tool with that name".into())),
    }
}
