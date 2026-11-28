# Victory United Launcher agent interface

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

### Status and hardware

#### `status`

Launcher version, engine build, game folder, whether the game runs, active profile and where the files are. Start here.

Example arguments

```json
{}
```

#### `hardware`

Graphics card, driver, memory, processor, Direct3D capabilities and warnings. The first call looks at the graphics card.

Example arguments

```json
{}
```

#### `shutdown`

Ends a launcher that runs without a window (--headless or --mcp-stdio). A running game keeps running, but nothing backs up the saves after it.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

### Settings, presets, profiles and flags

#### `get_settings`

Read launcher settings and the active profile.

Example arguments

```json
{}
```

#### `set_settings`

Change launcher settings. Supported keys: appearance, behavior, update_schedule, engine_override, backups, storage.verify_cache_by_hash and dirs.

Some changes require `confirm: true`.

- `launcher` (required). Settings to change. Use null to clear engine_override, backups.root or an entry in dirs.
- `confirm` (optional). Needed for changes that decide which programs or files are loaded.

Example arguments

```json
{"launcher":{"appearance":{"theme":"dark"}}}
```

#### `set_game_folder`

Choose the game folder using MicrosoftGame.Config.

Some changes require `confirm: true`.

- `path` (required). Existing game folder or its MicrosoftGame.Config.
- `confirm` (optional). Needed for changes that decide which programs or files are loaded.

Example arguments

```json
{"path":"D:/Games/Game/Content"}
```

#### `list_presets`

List launch presets and the automatic choice.

Example arguments

```json
{}
```

#### `apply_preset`

Set the active profile's preset.

- `preset` (required). Preset id from list_presets, or auto.

Example arguments

```json
{"preset":"auto"}
```

#### `list_profiles`

List profiles with their settings and mark the active profile.

Example arguments

```json
{}
```

#### `edit_profile`

Edit a profile's console, safe mode, toggles or upscaler settings.

- `name` (optional). Profile to edit. Defaults to the active one.
- `options` (required). Options to change: console, safe, toggles, upscaler, upscaler_mode, dlss (an object with enabled, quality as quality, balanced or performance, and frame_generation). DLSS only takes effect on an NVIDIA RTX card with the Streamline folder chosen.

Example arguments

```json
{"options":{"console":"series-x","safe":true}}
```

#### `select_profile`

Switch to a saved profile.

- `name` (required). Profile name.

Example arguments

```json
{"name":"Default"}
```

#### `create_profile`

Create a new profile and select it. Optionally copy an existing profile.

- `name` (required). Profile name.
- `copy_from` (optional). Name of the profile to copy.

Example arguments

```json
{"name":"Testing"}
```

#### `rename_profile`

Rename a profile to an unused name.

Some changes require `confirm: true`.

- `name` (required). Profile name.
- `new_name` (required). New profile name.
- `confirm` (optional). Needed for changes that decide which programs or files are loaded.

Example arguments

```json
{"name":"Testing","new_name":"Baseline"}
```

#### `delete_profile`

Delete a saved profile.

Requires `confirm: true`.

- `name` (required). Profile name.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{"name":"Testing"}
```

#### `import_profile`

Read a TOML profile into a new named profile.

- `name` (required). Profile name.
- `path` (required). Profile TOML file.

Example arguments

```json
{"name":"Imported","path":"D:/Exports/profile.toml"}
```

#### `export_profile`

Save a copy of a profile to a new TOML file.

- `name` (required). Profile name.
- `path` (required). New file to write.

Example arguments

```json
{"name":"Default","path":"D:/Exports/profile.toml"}
```

#### `list_flags`

List flag descriptions, risks and types, plus extra flags with their enabled state.

- `search` (optional). Match by name, group or description.
- `offset` (optional). Skip this many matching flags.
- `limit` (optional). Return up to this many flags. Defaults to 50.

Example arguments

```json
{"search":"threads"}
```

#### `set_flag`

Set an extra flag or an override in the active profile.

- `name` (required). Name of the launch flag.
- `value` (optional). Value to set. Omit for an extra flag's default.
- `scope` (optional). Layer to edit. Defaults to extra.
- `enabled` (optional). Enable or disable the flag. Defaults to true.

Example arguments

```json
{"name":"VICTORY_THIN_THREADS","value":"8"}
```

#### `clear_flag`

Remove an extra flag or return a profile override to its preset value.

- `name` (required). Name of the launch flag.
- `scope` (optional). Layer to edit. Defaults to extra.

Example arguments

```json
{"name":"VICTORY_THIN_THREADS"}
```

### Playing, logs and bug reports

#### `launch_preview`

Show the command, paths and environment the launcher would use.

- `without_extra_flags` (optional). Leave the extra flags out for this launch.

Example arguments

```json
{}
```

#### `play`

Start the configured game, including shader warm-up when needed.

- `without_extra_flags` (optional). Leave the extra flags out for this launch.
- `start_anyway` (optional). Acknowledge missing mesh shader support.

Example arguments

```json
{}
```

#### `stop`

Stop the game or shader warm-up and its child processes.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `warmup_status`

Show shader warm-up progress and the current run state.

Example arguments

```json
{}
```

#### `skip_warmup`

Stop translating shaders and continue starting the game.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `log_tail`

Read the tail of a game or warm-up log. Optionally filter the returned lines.

- `name` (optional). Log file name. Defaults to the current or most recent game log.
- `lines` (optional). Maximum lines to read before filtering. Defaults to 100.
- `search` (optional). Keep lines containing this text, ignoring case.

Example arguments

```json
{"lines":50,"search":"error"}
```

#### `make_bug_report`

Create an anonymized bug report ZIP in the local reports folder.

- `items` (optional). Include only these item names: log-tail.txt, flags.txt, versions.txt, hardware.txt, settings.txt.

Example arguments

```json
{}
```

### Monitor and profiler

#### `monitor_snapshot`

Read frame rate, timing, memory and counters from the running engine.

Example arguments

```json
{}
```

#### `monitor_history`

Read available frame samples from the last N seconds.

- `seconds` (optional). History window in seconds. Defaults to 30.

Example arguments

```json
{"seconds":15}
```

#### `export_monitor_csv`

Export the available frame samples to a local CSV.

- `seconds` (optional). History window in seconds. Defaults to 30.

Example arguments

```json
{"seconds":30}
```

#### `get_panel`

Read whether the in-game performance panel will be enabled for the next launch.

Example arguments

```json
{}
```

#### `set_panel`

Enable or disable the in-game performance panel on the next launch.

- `enabled` (required). Enable the in-game performance panel.

Example arguments

```json
{"enabled":true}
```

#### `profiler_status`

Read the sampling schedule for the next launch.

Example arguments

```json
{}
```

#### `start_profiler`

Enable scheduled profile sampling on the next launch.

Some changes require `confirm: true`.

- `windows` (optional). Sampling windows in seconds after the engine starts.
- `sample_ms` (optional). Milliseconds between samples.
- `threads` (optional). Maximum threads to sample.
- `depth` (optional). Maximum stack depth.
- `filter` (optional). Match thread names containing this text.
- `symbolize` (optional). Resolve module and function names.
- `folder` (optional). Existing output folder. Omit to keep the current folder.
- `confirm` (optional). Needed for changes that decide which programs or files are loaded.

Example arguments

```json
{"windows":[{"from":20,"to":40}]}
```

#### `stop_profiler`

Disable sampling for future launches. A running engine keeps its existing schedule.

Example arguments

```json
{}
```

#### `profiler_overview`

Read the most recent capture or a selected file in the configured profiler folder.

- `name` (optional). Capture file name. Defaults to the newest capture.
- `group` (optional). Group the rows by function, module or thread.
- `top` (optional). Maximum rows.
- `thread` (optional). Only include this thread id.
- `search` (optional). Match row names.

Example arguments

```json
{"group":"function","top":15}
```

### Saves and storage

#### `list_backups`

List backups with their labels, sizes and creation times.

Example arguments

```json
{}
```

#### `create_backup`

Take a manual backup of the current saves.

- `label` (optional). Optional label to keep with the backup.

Example arguments

```json
{"label":"Before moving saves"}
```

#### `verify_backup`

Check the files and hashes inside a backup.

- `id` (required). Backup id from list_backups.

Example arguments

```json
{"id":"backup-id"}
```

#### `restore_backup`

Replace current saves with a backup. Makes a recovery backup first.

Requires `confirm: true`.

- `id` (required). Backup id from list_backups.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{"id":"backup-id"}
```

#### `rename_backup`

Change the label of a save backup.

- `id` (required). Backup id from list_backups.
- `label` (required). New label.

Example arguments

```json
{"id":"backup-id","label":"Chapter one"}
```

#### `delete_backup`

Delete a save backup.

Requires `confirm: true`.

- `id` (required). Backup id from list_backups.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{"id":"backup-id"}
```

#### `import_backup`

Verify and import a backup ZIP.

- `path` (required). Local file or folder path.

Example arguments

```json
{"path":"D:/Backups/saves.zip"}
```

#### `export_backup`

Copy a verified backup to a ZIP file.

Some changes require `confirm: true`.

- `id` (required). Backup id from list_backups.
- `path` (required). Local file or folder path.
- `confirm` (optional). Needed for changes that decide which programs or files are loaded.

Example arguments

```json
{"id":"backup-id","path":"D:/Backups/saves.zip"}
```

#### `storage_sizes`

Measure game data, shader cache and backup folders and report any pending move.

Example arguments

```json
{}
```

#### `move_storage`

Start a verified background move of the cache or game data to an empty folder.

Requires `confirm: true`.

- `location` (required). Shader cache or saved game data.
- `path` (required). Local file or folder path.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{"location":"cache","path":"D:/VictoryCache"}
```

#### `resume_storage_move`

Resume an interrupted storage move.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `finish_storage_move`

Close a completed move or cancel an incomplete move.

Requires `confirm: true`.

- `delete_old` (optional). Remove the old copy after a completed move.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{"delete_old":false}
```

#### `clear_cache`

Clear the shader and pipeline cache. The next run may need shader warm-up.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `clear_game_data`

Clear saves and game data after making a recovery backup.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `job_status`

Read progress or the final result of the latest background launcher job.

Example arguments

```json
{}
```

#### `cancel_job`

Request cancellation of the running storage move or engine download.

Example arguments

```json
{}
```

### Engine builds

#### `list_engine_builds`

List installed engine builds, including the active and previous builds.

Example arguments

```json
{}
```

#### `check_engine_update`

Check the release manifest for the configured engine channel and build kind.

- `channel` (optional). Release channel.
- `kind` (optional). Engine build kind.

Example arguments

```json
{"channel":"stable","kind":"release"}
```

#### `install_engine_update`

Download and verify the engine found by check_engine_update, then make it active.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `rollback_engine`

Select an installed engine build or return to the previous build.

Requires `confirm: true`.

- `build` (optional). Installed build name. Defaults to the previous build.
- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```

#### `cleanup_engine_builds`

Remove superseded engine builds while keeping the active and rollback builds.

Requires `confirm: true`.

- `confirm` (optional). Set to true to really do it. Without it the answer only says what would happen.

Example arguments

```json
{}
```
