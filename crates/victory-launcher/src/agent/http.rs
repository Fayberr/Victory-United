//! A small HTTP server for MCP's Streamable HTTP transport, on 127.0.0.1 only, and the matching client
//! the stdio proxy uses.
//!
//! One request per connection, JSON answers, no event stream. Every request must carry the token, must
//! not come from a web page (`Origin`) and must name this machine in `Host`, so a page in a browser
//! cannot reach it even through DNS rebinding. Sizes, header counts, time and the number of connections
//! are all limited. The accept thread blocks, so an idle server costs nothing.
use super::{Backend, auth, mcp};
use anyhow::{Context, Result, anyhow, bail};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;
pub const MAX_BODY: usize = 1024 * 1024;
const MAX_CONNECTIONS: usize = 8;
/// Time a client has to send its whole request.
const REQUEST_TIME: Duration = Duration::from_secs(15);

pub struct Server {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    backend: Arc<Backend>,
    _lease: std::fs::File,
}

impl Server {
    /// Listens on a free port of 127.0.0.1, writes `agent.json` and answers until dropped.
    pub fn start(backend: Arc<Backend>) -> Result<Self> {
        let _lease = crate::core::storage::lock(&backend.dirs.data.join("agent.lock")).context(
            "Another launcher already owns the agent endpoint, or its folder cannot be written",
        )?;
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("Could not listen on 127.0.0.1")?;
        let port = listener.local_addr()?.port();
        let token = auth::new_token()?;
        backend
            .token
            .set(token.clone())
            .map_err(|_| anyhow!("This agent endpoint was already started."))?;
        auth::write_endpoint(
            &backend.dirs,
            &auth::Endpoint {
                url: format!("http://127.0.0.1:{port}/mcp"),
                port,
                token: token.clone(),
                pid: std::process::id(),
                headless: backend.headless,
                version: env!("CARGO_PKG_VERSION").into(),
            },
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            backend: backend.clone(),
            port,
            token: token.clone(),
            stop: stop.clone(),
            open: AtomicUsize::new(0),
        });
        let thread = match std::thread::Builder::new()
            .name("agent-accept".into())
            .spawn(move || accept(listener, shared))
        {
            Ok(thread) => thread,
            Err(error) => {
                auth::remove_endpoint(&backend.dirs, &token);
                return Err(error.into());
            }
        };
        Ok(Self {
            port,
            token,
            stop,
            thread: Some(thread),
            backend,
            _lease,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    #[cfg(test)]
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.backend.closed.store(true, Ordering::Release);
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked accept so the thread can end and close the port.
        let _ = TcpStream::connect_timeout(&local(self.port), Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        auth::remove_endpoint(&self.backend.dirs, &self.token);
    }
}

struct Shared {
    backend: Arc<Backend>,
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
    open: AtomicUsize,
}

fn local(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

fn accept(listener: TcpListener, shared: Arc<Shared>) {
    for connection in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = connection else {
            // Out of handles or similar. Waiting a moment avoids a busy loop while it lasts.
            std::thread::park_timeout(Duration::from_millis(200));
            continue;
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        if shared.open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let _ = write_response(&mut stream, 503, &[], None);
            finish(stream);
            continue;
        }
        shared.open.fetch_add(1, Ordering::SeqCst);
        let worker = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("agent-request".into())
            .spawn(move || {
                serve(stream, &worker);
                worker.open.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            shared.open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// An answer that ends the request early.
struct Refuse {
    status: u16,
    why: &'static str,
}

fn refuse<T>(status: u16, why: &'static str) -> Result<T, Refuse> {
    Err(Refuse { status, why })
}

#[derive(Debug)]
pub struct Head {
    pub method: String,
    pub path: String,
    /// Names in lower case.
    pub headers: Vec<(String, String)>,
}

impl Head {
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

pub fn parse_head(text: &str) -> Option<Head> {
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path, version) = (first.next()?, first.next()?, first.next()?);
    if first.next().is_some()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || method.is_empty()
        || !path.starts_with('/')
    {
        return None;
    }
    let mut headers = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':')?;
        // No spaces before the colon and no folded lines: both are used to smuggle headers.
        if name.is_empty()
            || name
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return None;
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Some(Head {
        method: method.into(),
        path: path.into(),
        headers,
    })
}

/// The rules every request has to pass before its body is read. Returns the body length.
fn admit(head: &Head, port: u16, token: &str) -> Result<usize, Refuse> {
    let hosts: Vec<&str> = head.all("host").collect();
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if hosts.len() != 1 || !allowed.iter().any(|a| a.eq_ignore_ascii_case(hosts[0])) {
        return refuse(403, "The Host header must name this computer and port.");
    }
    if head.all("origin").next().is_some() {
        return refuse(403, "Requests from web pages are not accepted.");
    }
    let given: Vec<&str> = head.all("authorization").collect();
    let given = match given.as_slice() {
        [one] => one
            .split_once(' ')
            .filter(|(kind, _)| kind.eq_ignore_ascii_case("bearer"))
            .map(|(_, t)| t.trim()),
        _ => None,
    };
    if !given.is_some_and(|t| auth::same_secret(t, token)) {
        return refuse(401, "A valid bearer token is required.");
    }
    if head.path != "/mcp" {
        return refuse(404, "The endpoint is /mcp.");
    }
    if head.method != "POST" {
        return refuse(405, "Only POST is supported.");
    }
    let versions: Vec<_> = head.all("mcp-protocol-version").collect();
    if !matches!(versions.as_slice(), [] | [_])
        || versions.first().is_some_and(|v| !mcp::VERSIONS.contains(v))
    {
        return refuse(400, "Unsupported MCP protocol version.");
    }
    if head.all("transfer-encoding").next().is_some() {
        return refuse(411, "Send the body with Content-Length.");
    }
    let lengths: Vec<&str> = head.all("content-length").collect();
    let [length] = lengths.as_slice() else {
        return refuse(411, "Exactly one Content-Length header is required.");
    };
    if length.is_empty() || !length.bytes().all(|b| b.is_ascii_digit()) {
        return refuse(400, "Content-Length is not a number.");
    }
    match length.parse::<usize>() {
        Ok(n) if n <= MAX_BODY => Ok(n),
        _ => refuse(413, "The request is too large."),
    }
}

/// Reads with one deadline for the whole request, so a slow client cannot keep a connection forever.
fn read_some(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, Refuse> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
        return refuse(408, "The request took too long.");
    }
    match stream.read(buffer) {
        Ok(0) => refuse(400, "The connection closed early."),
        Ok(n) => Ok(n),
        Err(_) => refuse(408, "The request took too long."),
    }
}

fn read_request(stream: &mut TcpStream, shared: &Shared) -> Result<Vec<u8>, Refuse> {
    let deadline = Instant::now() + REQUEST_TIME;
    let mut data = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if data.len() > MAX_HEAD {
            return refuse(431, "The headers are too large.");
        }
        let n = read_some(stream, &mut chunk, deadline)?;
        data.extend_from_slice(&chunk[..n]);
    };
    if end > MAX_HEAD {
        return refuse(431, "The headers are too large.");
    }
    let Some(head) = std::str::from_utf8(&data[..end]).ok().and_then(parse_head) else {
        return refuse(400, "This is not a valid HTTP request.");
    };
    if head.headers.len() > MAX_HEADERS {
        return refuse(431, "Too many headers.");
    }
    let length = admit(&head, shared.port, &shared.token)?;
    let mut body = data.split_off(end + 4);
    if body.len() < length
        && head
            .all("expect")
            .any(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    while body.len() < length {
        let n = read_some(stream, &mut chunk, deadline)?;
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(body)
}

fn serve(mut stream: TcpStream, shared: &Shared) {
    match read_request(&mut stream, shared) {
        Ok(body) => {
            if shared.stop.load(Ordering::SeqCst) {
                let _ = write_response(&mut stream, 503, &[], None);
            } else {
                let reply = mcp::handle(&shared.backend, &body);
                let text = reply.body.map(|v| v.to_string());
                let _ = write_response(&mut stream, reply.status, &[], text.as_deref());
            }
        }
        Err(Refuse { status, why }) => {
            let body = serde_json::json!({ "error": why }).to_string();
            let extra: &[(&str, &str)] = match status {
                401 => &[("WWW-Authenticate", "Bearer")],
                405 => &[("Allow", "POST")],
                _ => &[],
            };
            let _ = write_response(&mut stream, status, extra, Some(&body));
        }
    }
    finish(stream);
    // The answer to `shutdown` is out: now the process may end.
    if shared.backend.shutdown_requested.load(Ordering::SeqCst) {
        shared.backend.stop.set();
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    extra: &[(&str, &str)],
    body: Option<&str>,
) -> std::io::Result<()> {
    let body = body.unwrap_or("");
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        reason(status),
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    head.push_str(body);
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

/// Closes politely: reading what is still on the way keeps the client from losing the answer to a reset.
fn finish(mut stream: TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut chunk = [0u8; 4096];
    let mut drained = 0;
    while drained < 2 * MAX_BODY {
        match read_some(&mut stream, &mut chunk, deadline) {
            Ok(n) => drained += n,
            Err(_) => break,
        }
    }
}

// ---------------------------------------------------------------- client

#[cfg(test)]
/// Sends one JSON-RPC message to a running endpoint. Returns the status and the body.
pub fn post(endpoint: &auth::Endpoint, body: &[u8], wait: Duration) -> Result<(u16, Vec<u8>)> {
    let stream = TcpStream::connect_timeout(&local(endpoint.port), Duration::from_secs(2))
        .context("The launcher does not answer")?;
    exchange(stream, endpoint, body, wait, None)
}

pub fn exchange(
    mut stream: TcpStream,
    endpoint: &auth::Endpoint,
    body: &[u8],
    wait: Duration,
    protocol: Option<&str>,
) -> Result<(u16, Vec<u8>)> {
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    stream.set_read_timeout(Some(wait))?;
    let protocol = protocol.map_or(String::new(), |version| {
        format!("MCP-Protocol-Version: {version}\r\n")
    });
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n{protocol}Connection: close\r\n\r\n",
        endpoint.port,
        endpoint.token,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    let mut answer = Vec::new();
    stream
        .take(64 << 20)
        .read_to_end(&mut answer)
        .context("The launcher did not finish its answer")?;
    let end = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow!("No answer from the launcher"))?;
    let status = std::str::from_utf8(&answer[..end])
        .ok()
        .and_then(|h| h.split(' ').nth(1))
        .and_then(|s| s.parse().ok());
    let Some(status) = status else {
        bail!("The answer is not HTTP")
    };
    Ok((status, answer.split_off(end + 4)))
}
