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

Early. It boots and shows the menus, and the open world runs at a few frames per second, so it is
not playable yet. Shadows are missing some light, because the game's ray tracing does not run yet.

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
