//! Source for the generated agent reference.
use super::tools::{self, Confirm};
use std::fmt::Write;

const INTRO: &str = r#"# Victory United Launcher agent interface

In **Settings**, turn on **Let AI agents control the launcher**. Agent control is off by default.
The card shows the local port, token file and recent calls. Click **Turn off** to close the port.

## Connect a client

Claude Code, using the local stdio transport:

```powershell
claude mcp add --transport stdio victory-launcher -- "C:\Tools\Victory United\victory-launcher.exe" --mcp-stdio
```

For clients that take an MCP server configuration:

```json
{
  "mcpServers": {
    "victory-launcher": {
      "command": "C:\\Tools\\Victory United\\victory-launcher.exe",
      "args": ["--mcp-stdio"]
    }
  }
}
```

Update the executable path to match your installation. The stdio process forwards to the
running endpoint. If the launcher is closed, it starts a headless instance in the same process.
If a launcher window is open with agent control off, tool calls are refused.

For Streamable HTTP, read `url` and `token` from `%APPDATA%\Victory-United\agent.json`.
Connect on this PC with `Authorization: Bearer <token>` on each request. Keep that file private.
The port and token change each time the endpoint starts. Use JSON-RPC 2.0 on `POST /mcp` with
`Accept: application/json, text/event-stream`. The endpoint returns JSON. It supports the MCP
initialization handshake through protocol revision 2025-11-25. See the
[Streamable HTTP transport specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports).

Run `victory-launcher.exe --headless` to open the endpoint without a window. It prints the
port and token file path. Exit with Ctrl+C or the confirmed `shutdown` tool.
Headless startup skips the graphics probe. `hardware`, `launch_preview` and `play` can request it.

Start with `status` and `tools/list`. Call a tool through `tools/call` with its name and argument
object. A response with `confirmation_required: true` describes the proposed operation.
Confirm only after the person approves. For long storage moves and engine downloads,
`job_status` reports progress, and `cancel_job` requests cancellation.

Profiler settings take effect when the engine starts. Monitor history comes from samples the
engine has published and the launcher has read. Older engines may expose no samples.

Recent calls appear in Settings and in `logs\agent.log`. Entries show the tool name, UTC time
and outcome. Close the endpoint from Settings if you no longer need it.

## Tool reference

Generated from the launcher tool table. Regenerate with
`victory-launcher --write-agent-docs docs/launcher-agent.md`.

"#;

pub fn render() -> String {
    let mut doc = INTRO.to_string();
    for (name, tools) in tools::groups() {
        writeln!(doc, "### {name}\n").unwrap();
        for tool in tools {
            writeln!(doc, "#### `{}`\n\n{}\n", tool.name, tool.description).unwrap();
            match tool.confirm {
                Confirm::Never => {}
                Confirm::Always(_) => doc.push_str("Requires `confirm: true`.\n\n"),
                Confirm::When(_) => doc.push_str("Some changes require `confirm: true`.\n\n"),
            }
            for param in &tool.all_params() {
                let requirement = if param.required {
                    "required"
                } else {
                    "optional"
                };
                writeln!(doc, "- `{}` ({requirement}). {}", param.name, param.help).unwrap();
            }
            if !tool.all_params().is_empty() {
                doc.push('\n');
            }
            writeln!(doc, "Example arguments\n\n```json\n{}\n```\n", tool.example).unwrap();
        }
    }
    doc.trim_end().to_string() + "\n"
}
