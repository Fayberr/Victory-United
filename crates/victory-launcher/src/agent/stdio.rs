//! The local stdio transport forwards to a running launcher or owns a headless launcher.
use super::{Backend, auth, http, mcp};
use crate::core::actions::Dirs;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

const MAX_LINE: u64 = http::MAX_BODY as u64;
const CALL_TIME: Duration = Duration::from_secs(600);
const OFF: &str = "Agent control is off in the open Victory United Launcher window. Turn on \
\"Let AI agents control the launcher\" in Settings.";

enum Event {
    Line(Vec<u8>),
    Oversize,
    End(io::Result<()>),
    Shutdown,
}

struct Router {
    own: Option<(Arc<Backend>, http::Server)>,
    protocol: Option<String>,
    dirs: Dirs,
    events: mpsc::SyncSender<Event>,
}

impl Router {
    fn answer(&mut self, line: &[u8]) -> Option<String> {
        if let Some((backend, _)) = &self.own {
            return mcp::handle(backend, line)
                .body
                .map(|value| value.to_string());
        }
        if let Some(endpoint) = auth::read_endpoint(&self.dirs) {
            let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                match http::exchange(stream, &endpoint, line, CALL_TIME, self.protocol.as_deref()) {
                    Ok((202, _)) => return None,
                    Ok((200 | 400, body)) => {
                        let version =
                            serde_json::from_slice::<Value>(&body)
                                .ok()
                                .and_then(|reply| {
                                    reply["result"]["protocolVersion"]
                                        .as_str()
                                        .map(str::to_owned)
                                });
                        if version.is_some() {
                            self.protocol = version;
                        }
                        return Some(String::from_utf8_lossy(&body).into_owned());
                    }
                    // A file left over by a previous launcher may describe a port since reused.
                    Ok((401 | 403 | 404 | 405, _)) => {}
                    Ok((status, _)) => {
                        return unavailable(
                            line,
                            &format!("The launcher answered with status {status}."),
                        )
                        .body
                        .map(|v| v.to_string());
                    }
                    // The call may already have run. Do not repeat it on another endpoint.
                    Err(error) => {
                        return unavailable(line, &format!("{error:#}"))
                            .body
                            .map(|v| v.to_string());
                    }
                }
            }
        }
        if auth::window_is_open(&self.dirs) {
            return mcp::respond(line, |_, method, params| match method {
                "initialize" => Ok(mcp::initialize(params)),
                "ping" => Ok(json!({})),
                "tools/list" => Ok(json!({ "tools": super::tools::listing() })),
                "tools/call" => {
                    Ok(json!({ "content": [{ "type": "text", "text": OFF }], "isError": true }))
                }
                _ => Err((mcp::METHOD_NOT_FOUND, "Method not found".into())),
            })
            .body
            .map(|v| v.to_string());
        }
        let backend = Backend::standalone_in(self.dirs.clone());
        match http::Server::start(backend.clone()) {
            Ok(server) => {
                let events = self.events.clone();
                let owned = backend.clone();
                std::thread::spawn(move || {
                    owned.stop.wait();
                    let _ = events.send(Event::Shutdown);
                });
                self.own = Some((backend.clone(), server));
                mcp::handle(&backend, line).body.map(|v| v.to_string())
            }
            Err(error) => unavailable(line, &format!("{error:#}"))
                .body
                .map(|v| v.to_string()),
        }
    }

    fn wants_stop(&self) -> bool {
        self.own
            .as_ref()
            .is_some_and(|(backend, _)| backend.shutdown_requested.load(Ordering::Acquire))
    }

    fn finish(self) {
        if let Some((backend, server)) = self.own {
            drop(server);
            backend.stop.set();
            backend.finish_jobs();
            let mut runner = backend.runner();
            if runner.busy() && !backend.shutdown_requested.load(Ordering::Acquire) {
                runner.wait_idle();
            }
        }
    }
}

fn unavailable(line: &[u8], why: &str) -> mcp::Reply {
    mcp::respond(line, |_, _, _| Err((mcp::UNAVAILABLE, why.to_owned())))
}

fn read_lines(mut input: impl BufRead, events: &mpsc::SyncSender<Event>) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if input
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)?
            == 0
        {
            return Ok(());
        }
        let event = if line.len() as u64 > MAX_LINE && !line.ends_with(b"\n") {
            while !line.ends_with(b"\n") {
                line.clear();
                if input.by_ref().take(MAX_LINE).read_until(b'\n', &mut line)? == 0 {
                    break;
                }
            }
            Event::Oversize
        } else {
            Event::Line(std::mem::take(&mut line))
        };
        if events.send(event).is_err() {
            return Ok(());
        }
    }
}

fn serve(
    router: &mut Router,
    incoming: mpsc::Receiver<Event>,
    mut output: impl Write,
) -> io::Result<()> {
    for event in incoming {
        let reply = match event {
            Event::Line(line) if line.iter().all(u8::is_ascii_whitespace) => continue,
            Event::Line(line) => router.answer(line.trim_ascii()),
            Event::Oversize => Some(
                mcp::error(
                    Value::Null,
                    mcp::INVALID_REQUEST,
                    "The message is too large",
                )
                .to_string(),
            ),
            Event::End(result) => return result,
            Event::Shutdown => break,
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
        if router.wants_stop() {
            break;
        }
    }
    Ok(())
}

pub fn run() -> i32 {
    let (events, incoming) = mpsc::sync_channel(1);
    let mut router = Router {
        own: None,
        protocol: None,
        dirs: Dirs::real(),
        events: events.clone(),
    };
    std::thread::spawn(move || {
        let result = read_lines(io::stdin().lock(), &events);
        let _ = events.send(Event::End(result));
    });
    let result = serve(&mut router, incoming, io::stdout().lock());
    router.finish();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("victory-launcher: {error}");
            1
        }
    }
}
