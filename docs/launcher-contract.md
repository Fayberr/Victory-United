# Launcher contract

How the launcher, the engine and the release package fit together. Two people (or workers) build the
launcher in parallel, so these points are fixed up front. Change one only after telling the other side.

## Folders

The launcher is portable. Unpack the release anywhere and run `victory-launcher.exe`.

```
<launcher folder>\
  victory-launcher.exe
  manifest.json                installed release manifest (written by the updater)
  engine\
    current.txt                name of the active build folder, e.g. "a1b2c3d4-release"
    <build folder>\victory.exe plus everything the engine needs next to it (D3D12 folder, DLLs)
%APPDATA%\Victory-United\
  settings.toml                launcher settings (game folder, active profile, language, engine override)
  profiles\<name>.toml         saved launch profiles (preset + overrides + extra flags)
  logs\run-YYYYMMDD-HHMMSS.log engine stdout and stderr, last 20 kept
  cache\                       shader and pipeline caches (the engine's `cache_dir`), one folder per game install
  reports\                     bug report ZIPs
```

Helper functions for all of this are in `crates/victory-launcher/src/paths.rs`.
Game files are never copied, bundled or downloaded by the launcher (docs/asset-policy.md).

## Launching the engine

The launcher starts the engine as its own process, the same way `scripts/play.ps1` does:

    engine\<build>\victory.exe run <game executable> --console <series-x|series-s> --execute

with the final `VICTORY_*` environment (preset, then profile overrides, then extra flags, later wins) and
stdout and stderr redirected to a new file in `logs\`. Warm-up (shader translation) runs
`victory.exe shaders <game folder or executable>` first, as play.ps1 does. The machine profiles and
default settings in `scripts/play.ps1` are the source for the presets.

The core half fills `app::Shared` (`game_pid`, `launch_env`, `engine_exe`, `log_file`) every frame; the
ops half only reads it.

## Live statistics block (engine to launcher)

The engine process publishes one named shared-memory block, `Local\VictoryUnited.Stats.<engine pid>`
(`paths::stats_block_name`). Fixed layout, magic and version in the header, little endian, defined in
`crates/victory-stats` (single source of truth for both sides, no copy of the layout anywhere else).

- The engine writes at most once per presented frame with plain atomic stores: no locks, no system
  calls, no allocation, no formatting on the frame path. Cost target: under 1 microsecond per frame.
- The launcher reads it once per second (monitor tab) and derives fps itself from the frame
  counter and timestamps. It must work in the normal build and the diagnostics build.
- A block that does not exist (older engine) means "no data", never an error.

## Release package and manifest

A release on GitHub holds one ZIP (launcher, engine build, notices) and a `manifest.json` asset. The
manifest has a `schema` number, the launcher version, and per channel the engine build id (commit),
translator version, kind, download URL, SHA-256, size and notes. Channels: `stable` (latest release)
and `dev` (newest main build, a pre-release). The updater checks SHA-256 before it installs, installs
into a new `engine\<build>` folder, flips `current.txt` last, and keeps the previous build for rollback.

Not shipped in the repo or the ZIP unless a licence check says otherwise: NVIDIA DLSS/Streamline
files (the launcher points the user to them), and anything from the game.

## Idle CPU rules (hard requirement)

The emulator is CPU bound, so the launcher must cost almost nothing while the game runs.

- No `request_repaint()` loops and no animation. Draw only on input or on an event.
- The only timer allowed is `request_repaint_after(1 second)`, and only while the Monitor tab is
  visible and the game runs. Hidden or minimised: no repaint at all.
- Background threads block (channel `recv`, `Child::wait`, a blocking HTTP call). No sleep-poll loops.
  When a thread has a result it sends it and calls `ctx.request_repaint()` once.
- Targets, measured on the WinPC with a release build: idle Play tab under 0.5 percent of one core,
  game running with the Monitor tab open under 1 percent of one core, under 100 MB resident.
- Use the glow (OpenGL) renderer. Do not add a second GPU API or a web view.

## Look

Neutral dark gray, no color accent. Clean, simple, modern. Not a copy of any other launcher.
Colors outside the gray ramp are only for warnings and errors, muted and used sparingly.
All user-visible text goes through `i18n::tr()`; English only for now. No em-dashes anywhere.

## Ownership (to avoid merge conflicts)

- core half: `src/main.rs`, `src/app.rs` shell, `src/theme.rs`, `src/i18n.rs`, `src/core/**`,
  `locales/en/core.json`, presets and flag registry data files, `docs/launcher.md`.
- ops half: `src/ops/**`, `locales/en/ops.json`, `crates/victory-stats`, `crates/victory-update`,
  the engine-side publisher in `crates/victory-gpu`, release packaging under `scripts/release/`,
  `docs/release.md`.
- Shared and rarely edited: this file, `src/paths.rs`, the `Shared` struct in `src/app.rs`. Add to them
  with small commits and say so in the commit message.
