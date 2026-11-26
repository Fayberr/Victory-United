<<<<<<< HEAD
# Victory United
||||||| merged common ancestors
# VIctory
=======
# VIctory fork for AMD Radeon (work in progress)

This is a fork of [VIctory](https://github.com/Marvisak/VIctory) by Marvisak
and its contributors. It follows upstream `main` up to `8e79c80` (2026-11-24),
carries a number of upstream pull requests, and adds fixes found while running
the game's world on one AMD card.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## State of this fork (2026-11-25)

**Not playable yet: 5 to 7 frames per second.** Tested on one machine only:
Windows 11, AMD Radeon RX 9070 XT 16 GB, Ryzen 7 9800X3D, 64 GB RAM, Series X
profile. Nothing here has been run on an NVIDIA or Intel card.

| What | State on that machine |
|------|-----------------------|
| Boot, menus | work, 30 fps (as upstream) |
| The world | renders lit, with textures, people, vehicles, vegetation, water and sky |
| Saves | load and store |
| Frame rate in the world | 5 to 8 fps (7.2 to 7.7 standing still in a scene of 8,800 draws; less while driving). Shown in the picture's top left corner (`VICTORY_FPS=0` hides it) |
| Light in shadow | a stand-in that follows the game's sky by day and by night, less of it in creases and under objects: the game's ray-traced indirect light does not run |
| Reflections | the game's, held down: without rays every glossy surface reflected the open sky, indoors too, and floors looked whitewashed |
| Shadows of the sun and of lamps | the game's own, and they work; too much of the constant light drowns them |
| Distant trees | drawn |
| Vehicles in motion | whole (they lost and stretched parts until the copies of title memory saw writes through every view of a page) |
| In-game phone | **broken**: with the phone out the world is unlit (black) and the phone is an empty outline |
| Memory | about 22 GB of RAM and more video memory than a 16 GB card has |

A driver timeout (Windows resets the display driver, every monitor goes black
for a moment) happened once in the session this was written in. Expect it.

### What this fork changes over upstream

Picture:
- The world's indirect diffuse light is replaced by a constant while the ray
  passes do not run. What the game's resolve pass left instead was bright
  magenta on leaves, cloth and paper, and blocks of colours indoors
  (`VICTORY_AMBIENT=<level>`, 0.6 by default; `off` keeps the game's output).
  Its colour can be set (`VICTORY_AMBIENT_TINT=r,g,b`), and less of it
  reaches creases, corners and the underside of things, worked out from the
  scene's depth alone (`VICTORY_AMBIENT_SHADE=strength,radius`; grainy, it is
  not blurred yet). In diagnostics builds all of it can be tuned in a running
  game from a file (`VICTORY_AMBIENT_FILE`). It follows the sky dome the
  game draws for its reflections (`VICTORY_AMBIENT_SKY=off` keeps the
  constant, `gain,least,power` sets it): its level is `gain * mean^power +
  least` with the sky's hue, so night, dawn and noon need no hand-set level.
- Translated shaders get clean-up passes before they are written (dead
  code, constants, one instruction for one value, branch facts, one read
  for one place), taken from the VIctory fork at commit ba8697aa and
  adapted (`VICTORY_SHADER_OPT=0` turns them off). All 25,623 shaders this
  fork emits for the game pass Microsoft's validator with them. The first
  launch after such a change rebuilds every pipeline (minutes of black
  world) and **the water is missing until the game is started again**.
- The game's reflections are held to a part of that constant light after
  its last reflection pass (`VICTORY_AMBIENT_REFLECT`, 0.5 by default, 0
  leaves them alone). Without rays they are the sky's, unoccluded.
- Blending is off on integer render targets in host pipelines: the console
  ignores it there, D3D12 refuses the pipeline. One world pipeline the driver
  refused at every launch is made now (pull request #45, pgm1207 and Arvi23).
- In pixel shaders every read of another pixel of the same 2x2 quad goes
  through quad reads, helper pixels included. Wave reads return 0 from helper
  pixels on AMD, which showed as lines along every triangle on car paint,
  glass and skin. Upstream does this after discards only.
- A helper pixel's `v_readfirstlane` reads its own value (upstream `0601955`).
- Twice the dynamic descriptor slots, so the tables of heavy frames fit.
- Depth and stencil twins for depth textures written by compute, screen
  passes with pipeline workers of their own, textures told apart by the mips
  their memory really holds, and other fixes listed in the history.

Speed:
- The clamps of a run of indirect draws are recorded in a list before the
  list that draws, and run one after another (`VICTORY_HOIST_CLAMPS`, 7 by
  default, 0 for each before its draw): 225 to about 80 ms of GPU time per
  frame in a scene of 9,000 draws.
- The snapshots and the copy-back of the tiles shaders write are taken on
  several threads (pull request #18, PikPakPik): copying back went from
  about 43 to about 33 ms a frame here. The frame rate did not move.
- The title's submissions return before the host GPU is done, with what
  they owe finished at the next wait (`VICTORY_ASYNC_SUBMIT=0` turns it
  off): 200-238 to 150-186 ms a frame in the same place. It gained nothing
  until the next two changes were in.
- Title memory the game shows one 64 KiB view at a time is used in place:
  its pages follow one another in their section, so the stretch is mapped
  once more at an address of ours and the host heap opened there
  (`VICTORY_STRETCH=0` turns it off). Copying back went from 36 to 12 ms a
  frame. Ranges made of several stretches are still copied.
- The table of descriptor views is a read-write lock. A PIX timing capture
  showed the game's five recording threads taking turns on it as a mutex,
  6,600 context switches a second each.
- Thin LTO and one codegen unit for the release profile (pull request #96,
  Key-rs), and eight changes by romainhedouin that take repeated work out
  of the draw path (#24, #25, #27, #28, #29, #30, #33, #34). None of the
  eight moved the frame time measurably here; nothing broke.
- What a frame still waits for, from the same capture: the thread that
  submits, which prepares each batch while the host GPU is idle (the plan
  of directly indexed descriptors walks every slot, `bindless_plan`), and
  copies of title memory, about 100 MB a frame.
- Shader cache entries of another build can be reused
  (`VICTORY_SHADER_BUILD_OF=<executable>`, diagnostics).

Memory the title shows at several addresses:
- Every view of a guarded physical page is guarded with it, so a write of
  the title's through another address is seen (the idea of victory-fork
  #115 and victory-amd, written anew here); blocks the kernel is about to
  write stay writable for two seconds. Before this, parts of moving
  vehicles were left behind; the distant trees' quads that crossed the
  screen have not come back since either (cause not proven).

AMD:
- Shaders that crash or never leave the AMD driver's compiler cost only
  their pipeline, not the run.
- Buffer element counts come from the host's own table, not a size query the
  driver's compiler crashes on.

Lifetimes: a list keeps the copies of title memory whose addresses it gives
as root views, and objects the title let go of are kept eight seconds at
least (32 presented frames were a third of a second in the menus).

Diagnostics: every translated shader is logged with its DXIL hash, the one
[Radeon GPU Detective](https://gpuopen.com/radeon-gpu-detective/) names a
shader in flight by, so its crash reports can be matched to the game's
programs. Diagnostics builds: `VICTORY_SHADER_PATCH` changes one word of a
shader's machine code before translation, `VICTORY_SHADER_SWAP` replaces
shader containers by files, `VICTORY_ALWAYS_COPY=1` refreshes every copy of
title memory each frame, `VICTORY_GPU_TIMING=1` logs GPU time per pipeline.
Each is described in a comment where it is read.

### How it is run here

A diagnostics build (`cargo build --profile diag -p victory --features
diagnostics`), with:

```
VICTORY_PRESENT_TARGET=1x1:1
VICTORY_HAIR=1
victory run <game_scarlett_master.exe> --execute --console series-x
```

The first launch after a build translates every shader and builds about
2,500 pipelines; the loading screen can stay frozen, with sound, for minutes.

### Known problems

- With the in-game phone out the world is unlit (see the table).
- The per-draw descriptor slots are few for the heaviest places: when they
  run out, some draws of a frame are dropped.
- Memory: the process commits about 33 GB after a quarter of an hour's
  drive (the title's own 7 GB, 8 GB of textures on the card, copies of its
  buffers, and the system's backing of video memory).
- No ray-traced light; what stands in for it is the same everywhere in a
  frame. With the sky following turned off the constant of 0.6 is too much at
  night and too little at noon.
- Streamed textures are capped at 1024 pixels a side (512, then 256, as
  video memory fills): at 2048 this 16 GB card goes 1 to 2 GB over its
  budget (`VICTORY_TEXTURE_MAX=2048` to try).
- Some compute programs do not translate (ray-tracing structure builders,
  wind volumes, a few others) and their passes are dropped. With
  `VICTORY_RAYS=1` the builders translate and run, and the device is
  removed a minute and a half later: do not expect light from it. Two
  causes found with AMD's Radeon GPU Detective are fixed (lists and
  descriptors named copies of title memory that were already destroyed);
  the run now ends on a read at an address no resource ever had.
- Some draws of a frame are dropped in the world for want of a host
  pipeline: 266 programs of the pack do not translate (195 take console
  addresses in scalar registers: the ray-tracing builders and a family of
  geometry shaders; 71 mesh shaders pass the host's 32 KiB of outputs), and
  the AMD driver's compiler crashes on one compute program. Four mesh
  pipelines counted as refused before were only on the list of pipelines
  the driver's compiler does not end, put there by a rule that fired while
  the world loads; delete `pipelines/crashed.txt` in the cache folder if
  an older build wrote it. The program that activates the screen probes'
  rays is left out on purpose while rays do not run.
- Windows 10 is reported not to get past start-up without a newer D3D12
  runtime (issue #9, pull request #44, not merged or tried here yet); this
  fork has only been run on Windows 11.
- Tests that fail on this machine, before and after this update:
  `list::tests::lists_are_cut_at_waits_that_follow_work` in `victory-gpu`
  and five `xmem` tests in `victory-hle` (issues #2 and #8).
- `crates/victory-shader/src/emit/lower.rs` is larger than the size limit of
  `cargo xtask policy`; it is source code, not an asset.
- Many changes are specific to this title and this GPU, and some are marked
  `shortcut:` in the code where they approximate what the console does.

### Credits

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors is the base of everything here. Licensed GPL-3.0-or-later; so
  is this fork. Upstream's history and the authors of the pull requests
  carried here are kept in the commit history.
- Pull requests merged into this fork, read line by line and run in the
  world before merging: #45 (pgm1207, sent by Arvi23), #18 (PikPakPik),
  #96 (Key-rs), and #24, #25, #27, #28, #29, #30, #33, #34 (romainhedouin).
  About ninety others are still to be read, the larger speed changes
  first; thank you for them, for the test reports, and for the advice to
  take a PIX timing capture, which found more in an hour than a day of
  our own counters.
- [fxceless/victory-fork](https://github.com/fxceless/victory-fork) and
  [myokex/victory-amd](https://github.com/myokex/victory-amd) were read for
  comparison; no code of theirs is in this fork.
- [XCL](https://github.com/ivall/xcl) by ivall: its README was read; no code
  of it is here.
- This fork's changes were developed with
  [Claude Code](https://claude.com/claude-code) and Gemini (Antigravity).

---

## From upstream's README

What follows is taken from upstream VIctory's own README: what the project
is, how to build it, its commands and its debugging switches. They apply to
this fork too. Upstream's status section is left out, because it describes
upstream's state and upstream's test machine (an RX 6700 XT), not this fork
or the machine above: read it
[in upstream's repository](https://github.com/Marvisak/VIctory#readme).

# VIctory
>>>>>>> belobug/main

A tree for running GTA VI (the Xbox Series build) on Windows, built from the best work of the
VIctory forks. The program, its settings (`VICTORY_*`) and its crates keep the VIctory name, so
that work can still be shared between the forks.

[VIctory](https://github.com/Marvisak/VIctory) is a compatibility layer written in Rust that runs
an Xbox Series game natively on Windows. A lot of people work on it at the same time, in the
upstream repository and in forks like [victory-fork](https://github.com/fxceless/victory-fork),
and many good fixes sit in open pull requests that never get merged. Victory United starts from
the furthest-along fork and merges the best of those pull requests into one tree, so there is one
place to build from. [PROVENANCE.md](PROVENANCE.md) lists what is in it and where it came from.

<<<<<<< HEAD
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
||||||| merged common ancestors
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
=======
## Getting started
>>>>>>> belobug/main

You need Windows 11, a recent GPU driver, the Rust toolchain from `rust-toolchain.toml` and your
own copy of the game.

```
cargo build --release -p victory
```

Copy `victory.example.toml` to `victory.toml` and point it at your game folder. The build needs
the signed x64 Agility runtime (Microsoft.Direct3D.D3D12 1.620.2), and `stage-agility.ps1` fetches
and checks it. `launch-curated.cmd` is an example of how to start the game.

<<<<<<< HEAD
## Contributing
||||||| merged common ancestors
Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:
=======
Input: XInput pads. A DualSense or DualSense Edge (USB or Bluetooth) is read
over HID as well and plays as the first pad when no XInput pad answers. With
no pad connected the keyboard is a pad while the title's window has focus:
>>>>>>> belobug/main

<<<<<<< HEAD
Pull requests are welcome, including AI-assisted ones. See [CONTRIBUTING.md](CONTRIBUTING.md). If
you know of an open pull request in VIctory or another fork that should be in here, open an issue.
||||||| merged common ancestors
| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |
=======
| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, O | A, B |
| Backspace, X, Y | B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |
>>>>>>> belobug/main

## Credits

This stands on the work of Marvisak and the VIctory contributors, fxceless and the victory-fork
contributors, the victory-perf author, and the authors of the pull requests in
[PROVENANCE.md](PROVENANCE.md). Thank you.
