# Victory United

A tree for running GTA VI (the Xbox Series build) on Windows, built from the best work of the
VIctory forks. The program, its settings (`VICTORY_*`) and its crates keep the VIctory name, so
that work can still be shared between the forks.

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

## Credits

<<<<<<< HEAD
This stands on the work of Marvisak and the VIctory contributors, fxceless and the victory-fork
contributors, the victory-perf author, and the authors of the pull requests in
[PROVENANCE.md](PROVENANCE.md). Thank you.
||||||| e2a28ec7
This is a tree of other people's work with more on top. The history is kept, and
commits taken from elsewhere name the commit and its author.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: the loader, the runtime services, the Direct3D 12 back end and
  the shader recompiler.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: the fork
  this one started from, and the people whose pull requests it carries.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: ray tracing fixes,
  cube gradients, sparse textures.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**:
  the ambient light idea, pipeline workers, and work by romainhedouin.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**.
- **[myokex/victory-amd](https://github.com/myokex/victory-amd)**: the guard over
  every view of a page (with Spexivus, joroyt and tightfall), and Joshua Odom's
  reads into title memory.
- **Geckomacabre** and **cxy0072019-sketch**: the texture read-back.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the same
  title behaves under another layer; no code of it is here.
- This fork's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not
covered is named in [docs/asset-policy.md](docs/asset-policy.md).

ZETTROX
=======
This is a tree of other people's work with more on top. The history is kept, and
commits taken from elsewhere name the commit and its author.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: the loader, the runtime services, the Direct3D 12 back end and
  the shader recompiler.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: the fork
  this one started from, and the people whose pull requests it carries.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: ray tracing fixes,
  cube gradients, sparse textures.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**:
  the ambient light idea, pipeline workers, and work by romainhedouin.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**.
- **[myokex/victory-amd](https://github.com/myokex/victory-amd)**: the guard over
  every view of a page (with Spexivus, joroyt and tightfall), and Joshua Odom's
  reads into title memory.
- **Geckomacabre** and **cxy0072019-sketch**: the texture read-back.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the same
  title behaves under another layer; no code of it is here.
- This fork's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not
covered is named in [docs/asset-policy.md](docs/asset-policy.md).
>>>>>>> avatar/main
