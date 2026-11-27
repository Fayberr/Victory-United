# Victory United

A tree for running GTA VI (the Xbox Series build) on Windows, built from the best work of the
VIctory forks.

[VIctory](https://github.com/Marvisak/VIctory) is a compatibility layer written in Rust that runs
an Xbox Series game natively on Windows. A lot of people work on it at the same time, in the
upstream repository and in forks, and many good fixes sit in branches and pull requests that never
get merged. Victory United starts from the fork that measured best on our test machine and folds the
best of the others into it, so there is one place to build from. Since 2026-11-26 that starting
point is [Vice-Tay](https://github.com/TaynanGT/Vice-Tay) at commit `fab5286c`;
[PROVENANCE.md](PROVENANCE.md) lists what is in the tree and where it came from.

It contains no game files. You need your own copy of the game. It is not affiliated with or
endorsed by the game's developer or publisher, see [LEGAL.md](LEGAL.md).

## State

It boots, plays the intro and the menus, loads an open-world save and renders the city. In open-world
sessions played by hand it runs at 8 to 12 frames a second (9.5 fps on average), with 20 fps as the aim.

Test machine: Windows 11 Pro (build 26300), NVIDIA GeForce RTX 3080 Ti with 12 GB
(driver 32.0.16.2124), Ryzen 7 5700X3D, 64 GB RAM. Commit `65a8fce7` plus the machine profile in
`scripts/play.ps1`, Series S profile, no frame generation, half render scale with an own upscaler,
relay mode 2, late wait 3, 1.5 GB card buffers with their budgets reserved (64 KB minimum, 1 GB windows),
stubs for exposure, ambient light and the shadow mask, playing through an open-world street scene at night.

- **Speed:** 8 to 12 frames a second (9.5 fps on average) across minutes 3 to 6 of three 7 minute open-world
  sessions played by hand on a warm shader cache: 9.1, 7.7, 10.7 and 12.1 fps (card texture share 85 percent),
  9.2, 8.3, 11.9 and 10.6 fps (96 percent, the default) and 9.5, 7.9, 7.7 and 9.4 fps (85 percent again).
  Minute 2 ran at 10.6 to 11.4 fps and minute 1 at about 10 fps on a warm cache.
- **Picture:** from minute 2 on, 1,135 of 1,140 frames were lit (54 to 90 saved per minute), showing the
  street at night with the character, a billboard, shop fronts, a parked tow truck and a lit shop window
  with no white or black blocks. Ray-traced lighting is off by default (see
  [Vice-Tay's known problems](https://github.com/TaynanGT/Vice-Tay#known-problems) for upstream shader notes).
- **Stability:** a 40 minute open-world session played by hand on this tree ran smoothly at around 9.5 fps
  with no crashes, and all 7 minute open-world sessions played by hand completed cleanly with no crashes,
  panics or device-lost errors.
- **Earlier tree, played by hand:** on commit `3588725d` (before moving to Vice-Tay's code), a 34.7 minute
  session played by hand (33.6 minutes in the room after the intro, median 8.5 fps over 361 five-second reports)
  ended normally when the window was closed, with no crash, panic or device-removed errors.

## Build

You need Windows 11, a recent GPU driver, the Rust toolchain from `rust-toolchain.toml` and your
own copy of the game.

```
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --profile diag -p victory --features diagnostics
```

Translate the shaders and warm the pipelines once before the first run (each can be stopped and
resumed), then play:

```
target\diag\victory.exe shaders <the shader archives>
target\diag\victory.exe warm --used
scripts\play.ps1 -Exe <path of your copy's executable>
```

`Play-VIctory.cmd -Exe <path>` does the same from a double-click. The options of `scripts\play.ps1`
(window or full screen, Series S or X profile, settings for your card) are described in
[docs/this-fork.md](docs/this-fork.md) and in the script itself.

## Contributing

Pull requests are welcome, including AI-assisted ones. See [CONTRIBUTING.md](CONTRIBUTING.md). If
you know of a branch or pull request in VIctory or another fork that should be in here, open an
issue.

## Credits

This stands on the work of Marvisak and the VIctory contributors, the Vice-Tay authors and the
people whose work Vice-Tay carries (everyone with a commit is listed in
[docs/credits.md](docs/credits.md)), and the authors of the changes in
[PROVENANCE.md](PROVENANCE.md). Thank you.

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file it does not cover is named in
[docs/asset-policy.md](docs/asset-policy.md).
