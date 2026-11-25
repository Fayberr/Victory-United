# Victory United

One repository for running GTA VI (the Xbox Series build) on Windows, built from the best work of
all the VIctory forks instead of yet another fork.

**This repository contains no game files, code or assets.** You need your own copy of the game.
It is not affiliated with or endorsed by the game's developer or publisher.
See [LEGAL.md](LEGAL.md) for the disclaimer and what does not belong in this repo.

## What this is

[VIctory](https://github.com/Marvisak/VIctory) is a compatibility layer (written in Rust) that runs
an Xbox Series title natively on Windows. Many people now work on it in parallel, in the upstream
repository and in forks such as [victory-fork](https://github.com/fxceless/victory-fork). Good
fixes sit in hundreds of open pull requests that nobody has merged. Victory United is the one tree
that collects them:

1. Start from the furthest-along tree (today: victory-fork, which renders the prologue).
2. Take the best open pull requests from every project (upstream, forks, the XCL project's
   findings), one by one, each tested.
3. Keep a written record of what is in and why ([PROVENANCE.md](PROVENANCE.md)), and what was
   left out and why.
4. Keep every contributor's credit. Commits keep their original authors, and the credits are in
   [docs/FORK-README.md](docs/FORK-README.md).

Everything here is GPL-3.0-or-later, like VIctory.

## Current state (2026-11-25)

Not playable yet. Measured on one PC (Ryzen 7 5700X3D, RTX 3080 Ti 12 GB, Windows 11):
boot and menus run at 30 fps; the world runs at about 2 to 5 frames a second. The graphics card
is mostly idle, because the slow part is the layer that copies the Xbox's shared memory around
for the PC. Light in shadow is missing (the game's ray tracing does not run yet). More in
[docs/CURATED_CHANGES.md](docs/CURATED_CHANGES.md).

## How to build and run

You need Windows 11, a recent GPU driver, the Rust toolchain from `rust-toolchain.toml`, and your
own legally obtained game files.

```
cargo build --release -p victory
cargo xtask policy        # checks that no game data is committed
```

Copy `victory.example.toml` to `victory.toml` and point it at your game folder. SDK export 620
needs the signed x64 Agility runtime (Microsoft.Direct3D.D3D12 1.620.2): `stage-agility.ps1`
checks and stages it. `launch-curated.cmd` shows a working set of settings (profile,
render scale, cache folders). Read it before using it, some of its settings are speed tradeoffs
that are meant to be temporary.

Saves, caches and settings live in their own data folder per build. A save from another build
does not appear here until you copy its `saves` folder over.

## Contributing

Pull requests and AI-assisted code are welcome, as long as the code is tested on a real run, high
quality and safe. See [CONTRIBUTING.md](CONTRIBUTING.md) and [LEGAL.md](LEGAL.md).

## Curating (for contributors)

- `tools/curation/check-upstream.sh` lists what changed upstream and which new pull requests
  were opened. `tools/curation/find-pr.sh <word>` searches all projects before you build
  something yourself.
- Open an issue to propose a pull request for inclusion. Include a measurement (fps, ms a
  frame, or the crash it fixes) from a real run.

## Credits

All of it stands on the work of Marvisak and the VIctory contributors, fxceless and the
victory-fork contributors, the victory-perf author, and every author of the pull requests listed in
[PROVENANCE.md](PROVENANCE.md). Thank you.
