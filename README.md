# Victory United

A tree for running GTA VI (the Xbox Series build) on Windows, built from the best work of the
VIctory forks.

[VIctory](https://github.com/Marvisak/VIctory) is a compatibility layer written in Rust that runs
an Xbox Series game natively on Windows. A lot of people work on it at the same time, in the
upstream repository and in forks like [victory-fork](https://github.com/fxceless/victory-fork),
and many good fixes sit in open pull requests that never get merged. Victory United starts from
the furthest-along fork and merges the best of those pull requests into one tree, so there is one
place to build from. [PROVENANCE.md](PROVENANCE.md) lists what is in it and where it came from.

It contains no game files. You need your own copy of the game. It is not affiliated with or
endorsed by the game's developer or publisher, see [LEGAL.md](LEGAL.md).

## State

Early. It boots, plays the intro and the menus and reaches the first playable room. It is not
playable yet. Shadows are missing some light, because the game's ray tracing does not run yet.

Measured on one machine: Windows 11 Pro (build 26300), NVIDIA GeForce RTX 3080 Ti with 12 GB
(driver 32.0.16.2124), Ryzen 7 5700X3D, 64 GB RAM. Commit `db89ba3b`, Series S profile, no frame
generation, no real save (a fresh copy of the throwaway test data), measured in the first
playable room. Settings that differ from a plain start: internal render scale 0.5 with the
project's own upscaler, relay mode 2, late wait 3, 2 GB card buffers, diagnostics logging on
(frame log and profile), and a few shader stubs (exposure, ambient, shadow mask). The full list
of settings and how each run is done are in `docs/measurements.md`.

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
- **Input:** controller buttons work in the start screens and menus (A and X take the title from
  there to the first playable room). Moving the character is not confirmed: a run that held the
  move keys in the room for about two minutes did not move it (the character only idled at the
  window), so the figures above are for standing in the room, not for a play session.

Earlier, on commit `33f3c35` with the diagnostics build, the same room measured 4.9 to 5.2 frames
per second over two 6 minute runs.

Paired run on the same machine, same settings and same starting data: `cf5d6000` against
`ac63840d` (14 performance changes taken from other projects, listed in `PROVENANCE.md`), 6
minutes each. Once the room had settled, the first measured 250 ms a frame (4.0 fps) and the
second 199 ms a frame (5.0 fps), at about 4,450 draws a frame in both. The part that shrank most
was the heap buffers pass (106 ms to 72 ms). One run per build, no crash in either; details in
`docs/measurements.md`.

After that, the whole performance branch of VIctory-7900XTX was merged in (`3588725d`, listed in
`PROVENANCE.md`). One 15 minute run of that build in the same room, same machine and settings:
the last 20 settled five-second reports (4,300 to 4,700 draws a frame) had a median of 103 ms a
frame (9.7 fps), at about 4,330 draws; the median over the whole run was 9.3 fps. No crash. It is
one run, not a paired one: the 5.0 fps figure above came from a 6 minute run, so the two are
not an exact like-for-like comparison. Details in `docs/measurements.md`.

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

This stands on the work of Marvisak and the VIctory contributors, fxceless and the victory-fork
contributors, the victory-perf author, and the authors of the pull requests in
[PROVENANCE.md](PROVENANCE.md). Thank you.
