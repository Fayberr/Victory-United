<<<<<<< HEAD
# Victory United
||||||| a66ac212
# victory-gta6
=======
<p align="center"><img src="docs/klbf/logo.svg" width="120" alt="KLBF Lab"></p>
<h1 align="center">KLBF Lab 62 — test build (26 Nov 2026)</h1>
<p align="center">
  <img src="https://img.shields.io/badge/version-Lab%2062-1f46c8" alt="version: Lab 62">
  <img src="https://img.shields.io/badge/tested%20on-RX%209070%20XT-555" alt="tested on RX 9070 XT">
  <img src="https://img.shields.io/badge/stage-experimental-ffb000" alt="experimental">
  <a href="docs/klbf/MEASUREMENT.md"><img src="https://img.shields.io/badge/fps-shared%20measurement%20method-2ea44f" alt="shared measurement method"></a>
</p>
>>>>>>> klbf/beta-2

<<<<<<< HEAD
A tree for running GTA VI (the Xbox Series build) on Windows, built from the best work of the
VIctory forks. The program, its settings (`VICTORY_*`) and its crates keep the VIctory name, so
that work can still be shared between the forks.
||||||| a66ac212
A work-in-progress fork of [VIctory](https://github.com/Marvisak/VIctory), the
compatibility layer that runs the Xbox Series build of Grand Theft Auto VI
natively on Windows from files the user supplies. This fork follows upstream
`main` and adds fixes aimed at one goal: getting the game's world to render
correctly and playably on one particular PC.
=======
> **📏 A shared frame-rate standard.** Numbers between projects cannot be compared today (empty seas, menus, one lucky second). We propose one method for everyone: same starting save, same route, three tables (frame rate seen in the world, stability index, average performance index). **[Read the method](docs/klbf/MEASUREMENT.md)**, use it, and post your numbers.
>>>>>>> klbf/beta-2

<<<<<<< HEAD
[VIctory](https://github.com/Marvisak/VIctory) is a compatibility layer written in Rust that runs
an Xbox Series game natively on Windows. A lot of people work on it at the same time, in the
upstream repository and in forks like [victory-fork](https://github.com/fxceless/victory-fork),
and many good fixes sit in open pull requests that never get merged. Victory United starts from
the furthest-along fork and merges the best of those pull requests into one tree, so there is one
place to build from. [PROVENANCE.md](PROVENANCE.md) lists what is in it and where it came from.

**There are no game files, code or assets here.** You need your own copy of the game. This project
is not affiliated with or endorsed by the game's developer or publisher, see [LEGAL.md](LEGAL.md).

## State

Early. It boots, plays the intro and the menus and reaches the first playable room. It is not
playable yet. Shadows are missing some light, because the game's ray tracing does not run yet.

Measured on one machine: Windows 11 Pro (build 26300), NVIDIA GeForce RTX 3080 Ti with 12 GB
(driver 32.0.16.2124), Ryzen 7 5700X3D, 64 GB RAM. Commit `db89ba3b`, Series S profile, no frame
generation, no real save (a fresh copy of the throwaway test data), run in a hidden desktop
session by a script that presses the same keys every time to reach the first playable room.
Settings that differ from a plain start: internal render scale 0.5 with the project's own
upscaler, relay mode 2, late wait 3, 2 GB card buffers, diagnostics logging on (frame log and
profile), and a few shader stubs (exposure, ambient, shadow mask). Full list in
`docs/measurements.md`.

- **Speed:** 4.8 frames per second in the first playable room, standing at the window (median of
  346 five-second intervals over 31 minutes of play in the room; middle 80 percent between 4.3 and
  5.1, lowest 3.4, highest 5.3). The first 7 minutes were slower, about 4.2, while the game
  settled; after that about 4.8 to 4.9. This is one run of one build, so it is a single data
  point, not a range across machines.
- **Stability:** one run of 35 minutes without a crash or a GPU hang. It was stopped on purpose
  at its planned 35 minutes, so the true limit is not known yet. The log shows no crash or device
  removed message, and the game wrote frames until the last second.
- **Picture:** lit by daylight through the window, with the character and the window glass drawn.
  Two frames (from the middle and the end of the run) were looked at and showed no bright blocks.
  Shadows still miss some light.

Earlier, on commit `33f3c35` with the diagnostics build, the same room measured 4.9 to 5.2 frames
per second over two 6 minute runs.

Paired run on the same machine, same settings and same starting data: `cf5d6000` against
`ac63840d` (14 performance changes taken from other projects, listed in `PROVENANCE.md`), 6
minutes each. Once the room had settled, the first measured 250 ms a frame (4.0 fps) and the
second 199 ms a frame (5.0 fps), at about 4,450 draws a frame in both. The part that shrank most
was the heap buffers pass (106 ms to 72 ms). One run per build, no crash in either; details in
`docs/measurements.md`.

## Build

You need Windows 11, a recent GPU driver, the Rust toolchain from `rust-toolchain.toml` and your
own copy of the game.

```
cargo build --release -p victory
```

Copy `victory.example.toml` to `victory.toml` and point it at your game folder. The build needs
the signed x64 Agility runtime (Microsoft.Direct3D.D3D12 1.620.2), and `stage-agility.ps1` fetches
and checks it. `launch-curated.cmd` is an example of how to start the game.

## Contributing

Pull requests are welcome, including AI-assisted ones. See [CONTRIBUTING.md](CONTRIBUTING.md). If
you know of an open pull request in VIctory or another fork that should be in here, open an issue.
||||||| a66ac212
**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by Rockstar Games or
Take-Two Interactive.
=======
KLBF Lab is a test build of a compatibility layer that runs an Xbox Series title natively on Windows from files you supply. It gathers the best fixes we could measure, aimed at **AMD Radeon (RDNA, 64-lane) cards**. This build is based on Neptune (Traffic & zm0kiemedia) plus contributors' fixes credited below, and our pipeline pre-build that survives AMD driver crashes.

**No game files are included.** You need your own legally obtained copy. New to this? Follow the [beginner's guide](docs/GUIDE.md).

## Speed

**Lab 62 figures: measuring (real play session in progress).** The tables below are still Lab 61's.

No cherry-picked numbers: these come from ordinary play, not from a quiet scene chosen to look good (scenes with only open water run higher). Measured during a real play session (controller), RX 9070 XT 16 GB, Ryzen 7 9800X3D, 32 GB RAM, Series S profile + FSR 4, the settings of [`play.ps1`](play.ps1):

We propose a shared way to measure, so that every project can compare on equal footing: [docs/klbf/MEASUREMENT.md](docs/klbf/MEASUREMENT.md). The numbers below follow it.

**Frame rate seen in the world** (peaks by situation)

| Situation | fps |
|---|---|
| Standing still, looking at the sky | up to 27 |
| On foot in a quiet area (by a lake or a river) | up to 23 |
| Standing still in traffic, busy neighbourhood | up to 17 |
| On foot on a motorway slip road | up to 16 |
| On foot in a busy neighbourhood | up to 14 |
| Driving over a bridge above water | up to 17 |
| Driving a truck at moderate speed | up to 15 |
| Driving fast through streets (the world streams in) | up to 12 |

<!-- live-start -->
**Stability index**: how the frame rate is spread over a real play session still running, **37 minutes so far without a crash** (385 samples, on foot and driving; updated every 5 minutes of play):

| fps | Share of the time |
|---|---|
| 18 and more | 16 % |
| 15–18 | 28 % |
| 12–15 | 30 % |
| 9–12 | 15 % |
| 5–9 | 5 % |
| under 5 (freezes) | 6 % |

**Average performance index**, same session: average 14.0 fps, median 14.5, 5 % low 1.8, 1 % low 0.4 (the lows are the freezes).
<!-- live-end -->

Occasional short freezes in any situation (on foot or driving); they do not crash the game and it stays playable. We are working on them now.

Route: start at the male lead's house right after first arriving there (~3% progress), from the save made after waking up in the morning (same time of day every run), then wander his neighbourhood, some driving and some walking. One machine only: other cards and drivers may behave differently. Full log: [docs/klbf/results.md](docs/klbf/results.md).

Known issues:
- Occasional freezes when the card or PC memory fills (fewer since textures are capped at 1024 px; try `-LowMemory`).
- Road paint markings and some small props (parking rocks) flicker.
- Bushes look coarse.
- Some shadows flicker near trees; very distant trees sometimes flash black.
- Water surfaces are not drawn (the water is invisible or see-through), and lake water sometimes flickers; swimming still works.
- Mirrors stay black (they need ray tracing).
- Car windows (glass) are not drawn: you can break into a car but the glass is invisible.
- Resolution is Series S, upscaled.

## Build and run

Needs Git, [rustup](https://rustup.rs), Visual Studio Build Tools (C++ workload) and AMD's [FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 2.4 unzipped to `C:\FidelityFX-SDK`.

```powershell
git clone -b beta-2 https://github.com/PastiiJay/klbf-lab
cd klbf-lab
cargo build --release
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title -Prebuild   # once per build, about 10-20 min; resumes by itself after a driver crash
powershell -ExecutionPolicy Bypass -File .\play.ps1 -GameDir C:\Games\Title   # play
```

`-SeriesX` plays in Series X mode (no FSR needed, slower). Textures are capped at 1024 px by default; `-LowMemory` caps them at 512 px: fewer freezes when memory is full, softer textures. `-FfxDir` points at another FSR folder. Plug the controller in before starting.

## Settings

`play.ps1` sets these (the pre-build and the game must use the same ones: the shader cache depends on them):

- Our pre-build and skip list: `VICTORY_SKIP_PIPELINES` = [docs/klbf/skip-amd.txt](docs/klbf/skip-amd.txt); pipelines the driver crashed on are skipped from `prebuild-crashed.txt`.
- New in this build: `VICTORY_CLAMP_AFTER_WRITER=1` (zm0kiemedia), `VICTORY_PORT_CONSTANTS=1` and `VICTORY_PORT_DESCRIPTORS=1` (avatarmanz's renderer port), `VICTORY_DYNAMIC_SLOTS=786432` (more descriptor slots: no more draws dropped, fixes flicker), `VICTORY_INDIRECT_RAW=0` and `VICTORY_STAGE_POOL_MB=2048` (WillyboPeep).
- New in Lab 62: `VICTORY_TEXTURE_MAX_DIM=1024` by default (Evan-Lowry's cap; loading screens, menus and short-mip images are kept at full size, `VICTORY_TEXTURE_MAX_DIM_KEEP_UI=0` turns that off), `-LowMemory` sets 512. `VICTORY_LIBRARY_LOCK=0`: Ammar Zubair's AMD pipeline-library lock is on by default on AMD in the code, but the serialized compile was too slow in play, so the launcher turns it off (remove the line to try it). `VICTORY_TEXTURE_LRU=1` (samishating) is available, off by default.
- Series S: `--console series-s` and `VICTORY_FSR_DLL=<FidelityFX SDK>\Kits\FidelityFX\signedbin\amd_fidelityfx_upscaler_dx12.dll`.
- Neptune's set: `VICTORY_CARD_WINDOWS_MB=4096`, `BAKE_TEXTURES_MS=10000`, `EARLY_TEXTURES_MS=40`, `WAVE_OWN_LDS_GROUPS=64,128`, `WIDE_MERGE=1`, `RELAY_OWN_QUEUE=1`, `UNDER_WRITES_WORDS=1`, `UNDER_WRITES_RUNS=100000`, `UNDER_WRITES_MASKS=1`, `UNDER_WRITES_MASKS_LEAST=1`, `RELAY=2`, `WIDE_REFRESH=1`, `ASK_ONCE=1`, `TEXTURE_NO_WAIT=1`, `CARD_REHASH_RUNS=1`, `JOIN_COPIES=1`, `D3D12_BACKGROUND=0`, `LATE_WAIT_LISTS=1024`, `WINDOW_TAILS=1`, `TEXTURE_STAGE_MB=512`, `CARD_BUFFERS_MIN_KB=64`, `ASK_QUIET=8`, `COPY_CHANGED=1`, `QUIET_BACK=1`, `LIST_POOL_MOST=2048`, `LIST_POOL_MB=2048`, `WIDE_THREADS=12`, `TEXTURE_HASH_EVERY=32`, `LEVELS_ON_DEMAND=1`, `WINDOW_KB=1024`, `CARD_BUFFERS_MB=1536`, `CARD_WINDOW_UPLOAD=1`, `DROP_DXIL=1`, `DESC_DATA=1`, `VRAM_IDLE_MS=15000,2500,1000`, `PIPELINE_WAIT_MS=2500`, `OCEAN_READBACK=1`, `WATCH_4K=1`, `THIN=1`, `OVERLAY=0` (all with the `VICTORY_` prefix).

Please report results in an [issue](https://github.com/PastiiJay/klbf-lab/issues): GPU, driver, how long it ran, fps, what looks wrong. Text only, no game screenshots. Patch notes: [CHANGELOG.md](CHANGELOG.md).

Product names are only used to say what this works with; they belong to their owners. KLBF Lab is not affiliated with them. License: GPL-3.0 ([LICENSE](LICENSE)).
>>>>>>> klbf/beta-2

## Credits

<<<<<<< HEAD
This stands on the work of Marvisak and the VIctory contributors, fxceless and the victory-fork
contributors, the victory-perf author, and the authors of the pull requests in
[PROVENANCE.md](PROVENANCE.md). Thank you.
||||||| a66ac212
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here: the loader, the runtime
  services, the Direct3D 12 back end and the shader recompiler. Licensed
  GPL-3.0-or-later; so is this fork. Upstream's history is kept intact below
  the single commit that carries this fork's changes.
- **[XCL](https://github.com/ivall/xcl)** by ivall and its contributors
  (GPL-2.0-only) was used as a reference for how the same title behaves under
  another layer. Fixes inspired by it were written anew in VIctory's code.
- This fork's changes were developed with [Claude Code](https://claude.com/claude-code).

## Status of this fork (2026-11-25)

**Not playable yet: about 3 frames per second, and shadowed areas are black.**
Tested on one machine: Windows 11, Intel i7-14700KF, NVIDIA RTX 4070 12 GB,
32 GB RAM. The Series X profile is used throughout.

| Stage | State in this fork |
|-------|--------------------|
| Boot, menus, start screen | works, 30 fps (as upstream) |
| Opening cutscene | plays from start to finish, in order, lit: title cards, credits cards, night and dusk shots |
| Saves | the game autosaves a checkpoint and loads it again |
| First gameplay scene (the prologue) | renders: characters, textures, sky, sunlit exterior; HUD, pause menu and pad input work |
| Light in shadow | missing: the game's ray-traced bounce light and ray-traced lamp and sun shadows do not run yet, so anything the sun does not hit directly is black |
| Frame rate in the world | 2-3.5 fps (270-450 ms per frame), from 3-6 seconds per frame at the start |
| Stability | no driver hang or device loss in normal runs |

### What this fork changed (summary)

Rendering:
- Meshlet culling read its scene buffers as the wrong kind of view, so the world drew almost nothing; fixed.
- Per-draw descriptor tables could run out, after which draws used slots never made for them and the card hung; fixed.
- Light packed by the game's compute shaders was read back through empty copies (sky, probes, fog); fixed for every mip level and layer.
- The fog reconstruction, shadow-mask, hair, blend-shape skinning, fire and several material programs did not translate; fixed.
- Gathers at a named mip level (with and without an offset) read level 0; fixed.
- Implicit-level samples outside pixel shaders produced invalid DXIL in 730 compute programs; fixed.
- Rectangle lists, depth and stencil twins for depth textures written by compute, texture read-backs.
- Ray tracing, in progress and off by default: the game's own structure-builder programs translate, title memory can be read by raw address from shaders, and the hardware ray/box and ray/triangle test is emulated in DXIL (checked against a reference on the GPU).

Speed and memory:
- Waits for memory values, write-watch queries and page queries no longer walk page tables per call.
- Large title buffers get a copy in card memory, with shader writes found by hashing on the card.
- Submissions can be sent ahead of the wait for the card (`VICTORY_LATE_WAIT`), with a relay thread that lands results as the card finishes (`VICTORY_RELAY`, new and untested in the game).
- The bindless texture queue no longer uploads stale views; the view table no longer grows without bound; the pipeline library is mapped instead of read into memory.
- An intermittent device loss was traced to the pipeline library file being replaced while mapped; fixed.

Most of the newer speed changes sit behind environment switches that are off
by default (`VICTORY_LATE_WAIT`, `VICTORY_RELAY`, `VICTORY_WIDE_BACK`,
`VICTORY_CARD_BUFFERS_MIN_MB`, `VICTORY_INPLACE`, `VICTORY_BVH_BUILDER`, ...).
Each is described in a comment where it is read.

### Known problems

- Everything in shadow is black (see above).
- The interior of the first gameplay room is sometimes not drawn after the cutscene hands over to gameplay.
- Around 24 GB of memory is in use during play.
- Many changes here are specific to this title and this GPU, and some are marked `shortcut:` in the code where they approximate what the console does.

Everything below this line is upstream VIctory's own README, unchanged; its
status section describes upstream, not this fork.

---

# VIctory

Runs Grand Theft Auto VI (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
=======
Every commit keeps its author. Thank you:

- [@Marvisak](https://github.com/Marvisak) for [VIctory](https://github.com/Marvisak/VIctory), the base of everything here, and [@fxceless](https://github.com/fxceless) for [victory-fork](https://github.com/fxceless/victory-fork).
- Traffic and [@zm0kiemedia](https://github.com/zm0kiemedia) for Neptune, the base of this build, and the indirect-argument clamps.
- [@avatarmanz](https://github.com/avatarmanz) ([VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)): renderer port 1 and 2.
- [@Evan-Lowry](https://github.com/Evan-Lowry): the texture size cap.
- Ammar Zubair: the AMD pipeline-library lock (amdxc64.dll crash).
- samishating (from AKA0GHOST/victory-fork): least-recently-used texture copies (`VICTORY_TEXTURE_LRU`).
- [@WillyboPeep](https://github.com/WillyboPeep) ([neptune-ve](https://github.com/WillyboPeep/neptune-ve)): loading-screen pipelines, section views, profiler and settings.
- [@proneonn](https://github.com/proneonn) (Neon, [Vertex](https://github.com/proneonn/Vertex)) and [@tippythetaidum](https://github.com/tippythetaidum): face fixes.
- [@GamingRipperDoc](https://github.com/GamingRipperDoc) (fxceless/victory-fork#1216, analysis by [@IxMxAMAR](https://github.com/IxMxAMAR)): device-loss fix.
- [@IxMxAMAR](https://github.com/IxMxAMAR) and [@MalfoyDraco99](https://github.com/MalfoyDraco99) (fxceless/victory-fork#1213): AMD compiler crash fix.
- Carried in Neptune's base: [@myokex](https://github.com/myokex) ([victory-amd](https://github.com/myokex/victory-amd)), JoshuaO22, Spexivus, joroyt and tightfall.
- KLBF Lab ([@PastiiJay](https://github.com/PastiiJay)): the crash-surviving pipeline pre-build, the skip list, KLBF Lab names, tests and docs.
>>>>>>> klbf/beta-2
