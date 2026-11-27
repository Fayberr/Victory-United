//! The tool table: every action an agent can take, once. The MCP server, the argument checks and the
//! reference in `docs/launcher-agent.md` are all made from it, so they cannot drift apart.
mod config;
mod data;
mod engine;
mod info;
mod monitor;
mod run;

use super::{
    calls::Outcome,
    redact,
    schema::{self, Kind, Param},
    Backend,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub type Run = fn(&Arc<Backend>, &Args) -> Result<Value>;

/// When a tool needs `"confirm": true`.
#[derive(Clone, Copy)]
pub enum Confirm {
    Never,
    /// Destructive. The function says what a confirmed call would do and changes nothing.
    Always(Run),
    /// Only for some arguments: the function returns why this call needs it.
    When(fn(&Args) -> Option<String>),
}

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    /// Arguments that pass the schema, as JSON.
    pub example: &'static str,
    /// Changes nothing on this computer.
    pub read_only: bool,
    pub confirm: Confirm,
    pub run: Run,
}

impl Tool {
    pub fn destructive(&self) -> bool {
        matches!(self.confirm, Confirm::Always(_))
    }

    /// Every argument, `confirm` included where it applies.
    pub fn all_params(&self) -> Vec<Param> {
        let mut params = self.params.to_vec();
        match self.confirm {
            Confirm::Never => {}
            Confirm::Always(_) => params.push(schema::opt("confirm", Kind::Bool, "Set to true to really do it. Without it the answer only says what would happen.")),
            Confirm::When(_) => params.push(schema::opt("confirm", Kind::Bool, "Needed for changes that decide which programs or files are loaded.")),
        }
        params
    }

    pub fn input_schema(&self) -> Value {
        schema::object(&self.all_params())
    }
}

/// The checked arguments of one call.
pub struct Args(pub serde_json::Map<String, Value>);

impl Args {
    pub fn text(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
    }
    /// A required text argument.
    pub fn need(&self, name: &str) -> Result<&str> {
        self.text(name).ok_or_else(|| anyhow::anyhow!("\"{name}\" must not be empty"))
    }
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.0.get(name).and_then(Value::as_bool)
    }
    pub fn yes(&self, name: &str) -> bool {
        self.flag(name) == Some(true)
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        self.0.get(name).and_then(Value::as_i64)
    }
    pub fn number(&self, name: &str) -> Option<f64> {
        self.0.get(name).and_then(Value::as_f64)
    }
    pub fn object(&self, name: &str) -> Option<&serde_json::Map<String, Value>> {
        self.0.get(name).and_then(Value::as_object)
    }
    pub fn list(&self, name: &str) -> Option<Vec<&str>> {
        self.0.get(name).and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).collect())
    }
}

/// All tools, grouped the way the reference lists them.
pub fn table() -> &'static [Tool] {
    static TABLE: std::sync::OnceLock<Vec<Tool>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut all = Vec::new();
        for (_, tools) in groups() {
            all.extend(tools.iter().map(|t| Tool { ..*t }));
        }
        all
    })
}

pub fn groups() -> [(&'static str, &'static [Tool]); 6] {
    [
        ("Status and hardware", info::TOOLS),
        ("Settings, presets, profiles and flags", config::TOOLS),
        ("Playing, logs and bug reports", run::TOOLS),
        ("Monitor and profiler", monitor::TOOLS),
        ("Saves and storage", data::TOOLS),
        ("Engine builds", engine::TOOLS),
    ]
}

pub fn find(name: &str) -> Option<&'static Tool> {
    table().iter().find(|t| t.name == name)
}

/// The answer to `tools/list`.
pub fn listing() -> Vec<Value> {
    table()
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema(),
                "annotations": { "readOnlyHint": t.read_only, "destructiveHint": t.destructive() },
            })
        })
        .collect()
}

/// What a call produced, as the text a client gets.
pub struct Done {
    pub text: String,
    pub failed: bool,
}

/// No tool with that name.
#[derive(Debug)]
pub struct Unknown;

/// Checks the arguments, asks for confirmation where needed, runs the tool, records the call and hides
/// secrets in the answer.
pub fn call(backend: &Arc<Backend>, name: &str, arguments: &Value) -> Result<Done, Unknown> {
    let tool = find(name).ok_or(Unknown)?;
    let (outcome, mut value) = match attempt(backend, tool, arguments) {
        Ok((outcome, value)) => (outcome, value),
        Err(error) => (Outcome::Error, json!({ "error": format!("{error:#}") })),
    };
    backend.calls.add(tool.name, outcome);
    backend.wake();
    let token = super::auth::read_endpoint(&backend.dirs).map(|e| e.token);
    redact::value(&mut value, token.as_deref());
    Ok(Done {
        text: serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into()),
        failed: outcome == Outcome::Error,
    })
}

fn attempt(backend: &Arc<Backend>, tool: &Tool, arguments: &Value) -> Result<(Outcome, Value)> {
    schema::check(&tool.input_schema(), arguments).map_err(anyhow::Error::msg)?;
    let args = Args(arguments.as_object().cloned().unwrap_or_default());
    if !args.yes("confirm") {
        let asked = match tool.confirm {
            Confirm::Never => None,
            Confirm::Always(preview) => Some(preview(backend, &args)?),
            Confirm::When(why) => why(&args).map(Value::String),
        };
        if let Some(would) = asked {
            return Ok((Outcome::Asked, json!({
                "confirmation_required": true,
                "nothing_changed": true,
                "would": would,
                "next": "Call the tool again with the same arguments and \"confirm\": true.",
            })));
        }
    }
    Ok((Outcome::Ok, (tool.run)(backend, &args)?))
}
