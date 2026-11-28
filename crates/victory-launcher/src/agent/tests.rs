use super::{Backend, auth, docs, http, mcp, schema, tools};
use crate::core::{actions::Dirs, store::ThemeChoice};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    sync::Arc,
    time::Duration,
};

fn setup() -> (tempfile::TempDir, Arc<Backend>) {
    let temp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        data: temp.path().join("config"),
        app: temp.path().join("app"),
    };
    (temp, Backend::standalone_in(dirs))
}

fn rpc(name: &str, args: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": name, "arguments": args } })
}

fn tool_result(backend: &Arc<Backend>, name: &str, args: Value) -> (bool, Value) {
    let done = tools::call(backend, name, &args).unwrap();
    (done.failed, serde_json::from_str(&done.text).unwrap())
}

fn request(
    server: &http::Server,
    host: &str,
    token: Option<&str>,
    origin: Option<&str>,
    length: usize,
    body: &str,
) -> (u16, Value) {
    let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        client,
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Length: {length}\r\n"
    )
    .unwrap();
    if let Some(token) = token {
        write!(client, "Authorization: Bearer {token}\r\n").unwrap();
    }
    if let Some(origin) = origin {
        write!(client, "Origin: {origin}\r\n").unwrap();
    }
    write!(client, "\r\n{body}").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut reply = String::new();
    client.read_to_string(&mut reply).unwrap();
    let (head, body) = reply.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or_default())
}

#[test]
fn streamable_http_enforces_token_origin_host_and_body_limit() {
    let (_temp, backend) = setup();
    let server = http::Server::start(backend).unwrap();
    let port = server.port();
    let host = format!("127.0.0.1:{port}");
    let localhost = format!("localhost:{port}");
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let send = |host, token, origin| request(&server, host, token, origin, ping.len(), ping).0;
    assert_eq!(send(&host, None, None), 401);
    assert_eq!(send(&host, Some("wrong-token"), None), 401);
    assert_eq!(send(&host, Some(server.token()), Some("null")), 403);
    assert_eq!(send(&host, Some(server.token()), Some("")), 403);
    assert_eq!(send("example.test", Some(server.token()), None), 403);
    assert_eq!(send("127.0.0.1", Some(server.token()), None), 403);
    assert_eq!(send(&localhost, Some(server.token()), None), 200);
    assert_eq!(send(&host, Some(server.token()), None), 200);
    assert_eq!(
        request(
            &server,
            &host,
            Some(server.token()),
            None,
            http::MAX_BODY + 1,
            ""
        )
        .0,
        413
    );
    let (status, reply) = request(&server, &host, Some(server.token()), None, 1, "{");
    assert_eq!(status, 400);
    assert_eq!(reply["error"]["code"], mcp::PARSE_ERROR);
}

#[test]
fn json_rpc_handles_batches_notifications_and_unknown_methods() {
    let (_temp, backend) = setup();
    let batch = r#"[
        {"jsonrpc":"2.0","id":1,"method":"ping"},
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        {"jsonrpc":"2.0","id":"xyz","method":"no/such"},
        {"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18"}},
        42
    ]"#;
    let reply = mcp::handle(&backend, batch.as_bytes());
    assert_eq!(reply.status, 200);
    let reply = reply.body.unwrap();
    assert_eq!(reply.as_array().unwrap().len(), 4);
    assert_eq!(reply[0]["result"], json!({}));
    assert_eq!(reply[1]["id"], "xyz");
    assert_eq!(reply[1]["error"]["code"], mcp::METHOD_NOT_FOUND);
    assert_eq!(reply[2]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply[3]["error"]["code"], mcp::INVALID_REQUEST);
    assert_eq!(mcp::handle(&backend, b"[]").status, 400);
    let notification = mcp::handle(
        &backend,
        br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    assert_eq!(notification.status, 202);
    assert!(notification.body.is_none());
}

#[test]
fn loopback_calls_list_and_status_and_releases_the_port() {
    let (_temp, backend) = setup();
    let server = http::Server::start(backend.clone()).unwrap();
    let endpoint = auth::read_endpoint(&backend.dirs).unwrap();
    assert_ne!(endpoint.port, 0);
    assert_eq!(
        endpoint.url,
        format!("http://127.0.0.1:{}/mcp", server.port())
    );
    assert!(http::Server::start(Backend::standalone_in(backend.dirs.clone())).is_err());
    let call = |request: Value| {
        let (status, body) = http::post(
            &endpoint,
            request.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 200);
        serde_json::from_slice::<Value>(&body).unwrap()
    };
    let listed = call(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }));
    assert_eq!(listed["result"]["tools"], json!(tools::listing()));
    let status = call(rpc("status", json!({})));
    assert_eq!(status["result"]["isError"], false);
    let text = status["result"]["content"][0]["text"].as_str().unwrap();
    let status: Value = serde_json::from_str(text).unwrap();
    assert_eq!(status["mode"], "headless");
    assert_eq!(status["game_running"], false);
    assert!(!backend.has_hardware());
    assert_eq!(backend.calls.recent()[0].tool, "status");
    drop(server);
    assert!(auth::read_endpoint(&backend.dirs).is_none());
    // Windows can keep a recently used port in TIME_WAIT. Test that it stopped accepting instead of rebinding it.
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).is_err());
    let replacement = http::Server::start(Backend::standalone_in(backend.dirs.clone())).unwrap();
    assert_ne!(replacement.token(), endpoint.token);
}

#[test]
fn shutdown_requires_confirmation_and_replies_before_signalling_exit() {
    let (_temp, backend) = setup();
    let server = http::Server::start(backend.clone()).unwrap();
    let endpoint = auth::read_endpoint(&backend.dirs).unwrap();
    let shutdown = |args| {
        let body = rpc("shutdown", args).to_string();
        let (status, reply) =
            http::post(&endpoint, body.as_bytes(), Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        let reply: Value = serde_json::from_slice(&reply).unwrap();
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str::<Value>(text).unwrap()
    };
    assert_eq!(shutdown(json!({}))["confirmation_required"], true);
    assert!(!backend.stop.is_set());
    assert_eq!(shutdown(json!({ "confirm": true }))["shutting_down"], true);
    assert!(backend.stop.wait_for(Duration::from_secs(2)));
    drop(server);
}

#[test]
fn a_slow_request_cannot_write_after_the_endpoint_is_closed() {
    let (_temp, backend) = setup();
    let server = http::Server::start(backend.clone()).unwrap();
    let gate = backend.gate.enter();
    let endpoint = auth::read_endpoint(&backend.dirs).unwrap();
    let body = rpc(
        "set_settings",
        json!({ "launcher": { "appearance": { "theme": "dark" } } }),
    )
    .to_string();
    let reply =
        std::thread::spawn(move || http::post(&endpoint, body.as_bytes(), Duration::from_secs(5)));
    drop(server);
    drop(gate);
    let _ = reply.join().unwrap();
    assert_eq!(
        backend.dirs.settings().appearance.theme,
        ThemeChoice::System
    );
}

#[test]
fn every_example_validates_and_reference_matches_the_tool_table() {
    let mut names = std::collections::BTreeSet::new();
    for tool in tools::table() {
        assert!(names.insert(tool.name), "duplicate tool {}", tool.name);
        let args: Value = serde_json::from_str(tool.example).unwrap();
        schema::check(&tool.input_schema(), &args)
            .unwrap_or_else(|error| panic!("{}: {error}", tool.name));
    }
    assert!(tools::find("find_games").is_none());
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/launcher-agent.md");
    assert_eq!(
        fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
        docs::render()
    );
}

#[test]
fn destructive_tools_do_not_run_until_confirmed() {
    let (_temp, backend) = setup();
    let dir = &backend.dirs;
    let cache = dir.data.join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("sentinel"), "do not delete").unwrap();
    for tool in tools::table()
        .iter()
        .filter(|tool| matches!(tool.confirm, tools::Confirm::Always(_)))
    {
        let mut args: Value = serde_json::from_str(tool.example).unwrap();
        args["confirm"] = json!(false);
        let (failed, reply) = tool_result(&backend, tool.name, args);
        assert!(!failed, "{}: {reply}", tool.name);
        assert_eq!(
            reply["confirmation_required"], true,
            "{}: {reply}",
            tool.name
        );
    }
    assert!(!dir.move_journal().exists());
    assert!(!dir.settings_file().exists());
    assert!(!backend.stop.is_set());
    assert!(backend.job().is_none());
    assert!(cache.join("sentinel").is_file());
    let (failed, reply) = tool_result(&backend, "clear_cache", json!({ "confirm": true }));
    assert!(!failed, "{reply}");
    assert!(!cache.join("sentinel").exists());
}

#[test]
fn set_settings_checks_paths_and_values_and_requires_confirmation_for_paths() {
    let (temp, backend) = setup();
    let good = temp.path().join("victory.exe");
    let wrong = temp.path().join("some-other-program.exe");
    fs::write(&good, "fixture only").unwrap();
    fs::write(&wrong, "fixture only").unwrap();
    let action = |launcher, confirm| {
        tool_result(
            &backend,
            "set_settings",
            json!({ "launcher": launcher, "confirm": confirm }),
        )
    };
    let (_, preview) = action(json!({ "engine_override": good }), false);
    assert_eq!(preview["confirmation_required"], true);
    assert!(backend.dirs.settings().engine_override.is_none());
    for invalid in [
        json!({ "engine_override": wrong }),
        json!({ "backups": { "root": temp.path().join("absent") } }),
        json!({ "appearance": { "theme": "sepia" } }),
        json!({ "appearance": { "scale_percent": 151 } }),
        json!({ "agent_control": true }),
        json!({ "storage": { "cache": temp.path() } }),
    ] {
        assert!(action(invalid, true).0);
        assert!(!backend.dirs.settings_file().exists());
    }
    assert!(
        !action(
            json!({ "engine_override": good, "appearance": { "theme": "dark" } }),
            true
        )
        .0
    );
    assert_eq!(
        backend.dirs.engine_exe(&backend.dirs.settings()),
        Some(good.clone())
    );
    assert_eq!(backend.dirs.settings().appearance.theme, ThemeChoice::Dark);
    fs::remove_file(good).unwrap();
    assert!(backend.dirs.engine_exe(&backend.dirs.settings()).is_none());
    let (_, settings) = tool_result(&backend, "get_settings", json!({}));
    assert!(settings["launcher"]["engine_override"].is_null());
}

#[test]
fn selecting_a_game_config_uses_its_executable_and_does_not_launch() {
    let (temp, backend) = setup();
    let dir = temp.path().join("game");
    fs::create_dir(&dir).unwrap();
    let cfg = dir.join("MicrosoftGame.Config");
    fs::write(&cfg, r#"<Executable Name="game.exe">"#).unwrap();
    fs::write(dir.join("game.exe"), "fixture only").unwrap();
    let (_, preview) = tool_result(&backend, "set_game_folder", json!({ "path": dir }));
    assert_eq!(preview["confirmation_required"], true);
    let (failed, reply) = tool_result(
        &backend,
        "set_game_folder",
        json!({ "path": cfg, "confirm": true }),
    );
    assert!(!failed, "{reply}");
    assert!(reply["executable"].as_str().unwrap().ends_with("game.exe"));
    assert!(!backend.runner().busy());
    assert!(backend.dirs.settings().setup_done);
}

#[test]
fn remembered_agent_permission_defaults_off() {
    let (_temp, backend) = setup();
    assert!(!backend.dirs.settings().agent_control);
    assert!(!auth::endpoint_file(&backend.dirs).exists());
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).is_ok());
}
