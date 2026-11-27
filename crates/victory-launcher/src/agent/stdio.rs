//! `--mcp-stdio`: MCP over standard input and output, one JSON message per line.
//!
//! Each message goes to the launcher that is serving agents (found through `agent.json`). When no
//! launcher runs, this process answers itself, like `--headless`. When a window is open with agent
//! control switched off, tool calls are refused: the switch in the window wins.
use super::{auth, http, mcp, Backend};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

const MAX_LINE: u64 = http::MAX_BODY as u64;
/// Longest a forwarded call may take (a long measurement or a large backup).
const CALL_TIME: Duration = Duration::from_secs(600);
const OFF: &str = "A Victory Launcher window is open with agent control switched off. Turn on \
\"Let AI agents control the launcher\" in its Settings, or close the window.";

/// Where messages go.
pub struct Router {
    own: Option<Arc<Backend>>,
    make: Box<dyn Fn() -> Arc<Backend>>,
    dirs: crate::core::actions::Dirs,
}

impl Router {
    pub fn new(dirs: crate::core::actions::Dirs, make: impl Fn() -> Arc<Backend> + 'static) -> Self {
        Self { own: None, make: Box::new(make), dirs }
    }

    /// The answer to one line, if it needs one.
    pub fn answer(&mut self, line: &[u8]) -> Option<String> {
        // A game this process started stays with this process, whatever else is running.
        let busy = self.own.as_ref().is_some_and(|b| b.runner().busy() || b.job_running());
        if !busy {
            if let Some(endpoint) = auth::read_endpoint(&self.dirs) {
                let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
                if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
                    match http::exchange(stream, &endpoint, line, CALL_TIME) {
                        Ok((202, _)) => return None,
                        Ok((200 | 400, body)) => return Some(String::from_utf8_lossy(&body).into_owned()),
                        // The port belongs to someone else now: the file is left over.
                        Ok((401 | 403 | 404 | 405, _)) => {}
                        Ok((status, _)) => return refusal(line, &format!("The launcher answered with status {status}.")),
                        // Never run it a second time here: the launcher may have done it already.
                        Err(error) => return refusal(line, &format!("{error:#}")),
                    }
                }
            }
            if auth::window_is_open(&self.dirs) {
                return switched_off(line);
            }
        }
        let backend = self.own.get_or_insert_with(|| (self.make)()).clone();
        mcp::handle(&backend, line).body.map(|v| v.to_string())
    }

    pub fn wants_stop(&self) -> bool {
        self.own.as_ref().is_some_and(|b| b.shutdown_requested.load(Ordering::SeqCst))
    }

    /// The client is gone. A game started here keeps this process until it ends, so that the backup
    /// after the exit still happens.
    pub fn finish(self) {
        if let Some(backend) = self.own {
            backend.finish_jobs();
            let mut runner = backend.runner();
            if runner.busy() && !backend.shutdown_requested.load(Ordering::SeqCst) {
                runner.wait_idle();
            }
        }
    }
}

/// The ids of the requests in a line (notifications have none).
fn requests(line: &[u8]) -> Vec<(Value, Value)> {
    let Ok(message) = serde_json::from_slice::<Value>(line) else { return Vec::new() };
    let list = match message {
        Value::Array(list) => list,
        one => vec![one],
    };
    list.into_iter().filter_map(|m| Some((m.get("id")?.clone(), m))).collect()
}

fn pack(line: &[u8], answers: Vec<Value>) -> Option<String> {
    let batch = line.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[');
    match (batch, answers.len()) {
        (_, 0) => None,
        (false, _) => Some(answers[0].to_string()),
        (true, _) => Some(Value::Array(answers).to_string()),
    }
}

fn refusal(line: &[u8], why: &str) -> Option<String> {
    pack(line, requests(line).into_iter().map(|(id, _)| mcp::error(id, mcp::UNAVAILABLE, why)).collect())
}

/// While the window has agents switched off, a client can still connect and see the tools.
fn switched_off(line: &[u8]) -> Option<String> {
    let answers = requests(line)
        .into_iter()
        .map(|(id, message)| {
            let answer = match message.get("method").and_then(Value::as_str) {
                Some("initialize") => mcp::initialize(message.get("params").unwrap_or(&Value::Null)),
                Some("ping") => json!({}),
                Some("tools/list") => json!({ "tools": super::tools::listing() }),
                Some("tools/call") => json!({ "content": [{ "type": "text", "text": OFF }], "isError": true }),
                _ => return mcp::error(id, mcp::UNAVAILABLE, OFF),
            };
            json!({ "jsonrpc": "2.0", "id": id, "result": answer })
        })
        .collect();
    pack(line, answers)
}

/// Reads lines until the input ends, writing the answers.
pub fn serve(router: &mut Router, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut input = input;
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.by_ref().take(MAX_LINE + 1).read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.len() as u64 > MAX_LINE {
            // Skip the rest of the oversized line.
            while !line.ends_with(b"\n") {
                line.clear();
                if input.by_ref().take(MAX_LINE).read_until(b'\n', &mut line)? == 0 {
                    break;
                }
            }
            writeln!(output, "{}", mcp::error(Value::Null, mcp::INVALID_REQUEST, "The message is too large"))?;
            output.flush()?;
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if let Some(answer) = router.answer(line.trim_ascii()) {
            writeln!(output, "{answer}")?;
            output.flush()?;
        }
        if router.wants_stop() {
            break;
        }
    }
    Ok(())
}

pub fn run() -> i32 {
    let mut router = Router::new(crate::core::actions::Dirs::real(), Backend::standalone);
    let result = serve(&mut router, std::io::stdin().lock(), std::io::stdout().lock());
    router.finish();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("victory-launcher: {error}");
            1
        }
    }
}
