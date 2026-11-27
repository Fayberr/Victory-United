# Launcher ops worker: status

Branch `launcher/ops`. Core's theme and capture mode were merged once (`d2433b3d4`).

## Done

- `crates/victory-stats`: versioned shared-memory block (8704 bytes, magic `VUSTATS1`), writer, reader, seqlock for the
  frame section, per-field atomics for slow values. 6 tests (layout, round trip, ring wrap, missing block, torn read
  under a concurrent writer, frame cost). Measured on Linux in-process: about 7 ns per `Writer::frame`.
  Not measured inside the engine on the WinPC.
- Engine publisher: `crates/victory-gpu/src/launcher_stats.rs`, called from `Meter::frame` in `host.rs` (frame path:
  a few atomic stores plus one QPC read; the once-a-second branch writes VRAM, RAM, draws, dispatches).
  `VICTORY_STATS=0` turns it off; only the first meter owns the block. Per-frame CPU and GPU times are the change of
  `SUBMISSION_US` between two presents. VRAM comes from `Host::video_memory` (two statics added there). The "ms/frame"
  log lines were not touched. NOT verified: no engine was run, so the block has never been read from a real engine.
  Pipelines building and shader warm-up progress are written as 0 (the engine has no counter for them).
- `crates/victory-update`: manifest (schema 1, channels, engines per kind), GitHub release lookup, WinHTTP client
  (system TLS and proxy, no C toolchain; chosen because it needs no extra crate), download with size and SHA-256 check
  into `.part`, install into `engine\<commit8>-<kind>` through `.partial`, `current.txt` flipped last through a rename,
  `previous.txt`, rollback, cleanup (keep 3). 8 offline tests including corrupt, cancelled and interrupted installs and
  path traversal. NOT verified: the WinHTTP path has only been compiled, never run against GitHub.
- Release packaging: `scripts/release/package.ps1`, `test-package.ps1`, `docs/release.md`. It refuses DLSS and game files.
- Screens (`src/ops/`): Monitor (numbers, graph of 512 frames, CPU/GPU split, memory, work counters, CSV export of the
  last 60 s, overlay controls), overlay (second viewport: no decorations, transparent, topmost, click-through, four
  corners), Updates, Developer (builds, own victory.exe, profiler string, log tail with search, folder buttons, cache
  clear with confirmation and a guard that only a folder named `cache` is emptied, active environment).
  `OpsState::demo()` supplies a fake stats stream and fake releases. Screens were reviewed from `--ui-demo --capture`
  on the WinPC.
- Small shared edit in `app.rs`: `OpsState::new(ctx)` and `self.ops.tick(...)` once per frame.

## Measured (WinPC, release build, hidden)

- Idle Play tab, no game: 0.00 percent of one core over 60 s (the process time counter has about 15 ms resolution),
  working set 81 MB.
- `--ui-demo`: 0.00 percent over 30 s, 77 MB.
- Cost of one Monitor read (copy 512 frames, summarise, keep history), Linux release test: about 6 microseconds,
  once a second. Drawing the plot is extra and was not timed.
- NOT measured: "game running with the Monitor tab open" and "overlay on". That needs a live engine pid, and the overlay
  is a topmost window that would show on the gaming PC, which the brief forbids. The lead should measure these with a
  real run.

## Not verified / not done

- Overlay over a borderless or windowed game: the window flags are set (no decorations, transparent, topmost,
  mouse passthrough, no taskbar entry) but I could not test it over a game window. Exclusive fullscreen will hide it
  and the UI says so. The overlay sits on the monitor size reported by egui, assumed to be the primary monitor.
  Whether the overlay keeps updating while the main window is minimised is unverified (it is redrawn from the main
  window's once-a-second update).
- The Developer tab's own `victory.exe` path is stored in `ops.toml` (`own_engine`). Core's Settings/play code must read
  it (or the lead should move it into `settings.toml`); nothing uses it yet.
- "Add to my flags" appends `NAME=value` to `launch-flags.txt`. Check core's real file format at merge.
- The profiler is only a string builder plus a flag; launching belongs to core's Play flow.
- The launcher's own update is a "newer launcher exists" line with a link only.
- Overlay is `show_viewport_immediate`, so it only redraws when the main window updates.
- `package.ps1` result on the WinPC dry run: see the end of this file.

## Gates

Both Windows gates (`/tmp/vu/chk-launcher.sh`) were green at commit "ops: warnings". Later commits only touched
launcher UI and tests; re-run the gates after merging.
