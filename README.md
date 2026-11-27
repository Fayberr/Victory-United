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

Early. It boots, plays the intro and the menus and reaches the first playable room. It is not
playable yet: in that room it ran at 12 to 15 frames a second in four runs of 7 minutes, and the aim is 20.

Measured on one machine: Windows 11 Pro (build 26300), NVIDIA GeForce RTX 3080 Ti with 12 GB
(driver 32.0.16.2124), Ryzen 7 5700X3D, 64 GB RAM. Commit `65a8fce7` plus the machine profile in
`scripts/play.ps1`, Series S profile, no frame generation, no
real save (a fresh copy of the throwaway test data), measured in the first playable room, standing
at the window. These switches were set (a switch this tree
does not know has no effect): internal render scale 0.5 with an own upscaler, relay mode 2, late
wait 3, 1.5 GB card buffers with their budgets reserved (64 KB minimum, 1 GB windows), textures
limited to 85 percent of the card's budget, and stubs for exposure, ambient light and the shadow
mask.

- **Speed:** minutes 3 to 6 of 7 minute runs, from the frame counter in the log, each run on a
  cache copied fresh from the same warm one: with textures limited to 85 percent of the budget
  14.5, 14.8, 14.9 and 14.6 fps in one run and 11.9, 11.9, 12.2 and 12.1 in a repeat; with 75 percent
  14.6, 15.1, 15.1 and 14.8; with 90 percent 13.0, 13.3, 13.2 and 13.3. Minutes 1 and 2 are slower
  (about 2.5 fps) while the game settles. Without the limit (96 percent, the default) three of five
  runs of the same build ran at 4 to 5 fps instead: the card then held 12.4 GB of its 12 GB. These
  are single runs of one build on one machine, not a range across machines.
- **Picture:** from minute 3 on every saved frame was a lit frame (89 to 113 per minute, none dark or
  blank); the frames at minute 4 and at the end of the 75 percent run show the room lit through the
  window with the watchtower outside and the character in front of it. Some detail is missing: the
  character's face and hair show horizontal stripes, the game's ray-traced light is
  not built by default, and [Vice-Tay's list of known problems](https://github.com/TaynanGT/Vice-Tay#known-problems)
  was not checked here.
- **Stability:** these runs lasted 7 minutes and had no panic or device-lost line in the log. A longer
  session has not been shown on this tree. On the earlier tree
  below, a session played by hand for 34.7 minutes ended normally. Vice-Tay reports memory growing in long
  sessions and the frame rate falling with it.
- **Earlier tree, played by hand:** on commit `3588725d` (before this tree moved to Vice-Tay's code) a person played
  for 34.7 minutes, launch to close, about 33.6 minutes of it in the room after the intro, and moved the
  character. That is the player's observation, not something the log measures. From the log: median 8.5 fps
  over 361 five-second reports in the room. The session ended when the window was closed normally, with no crash,
  panic or device-removed line. This is a different tree from the one measured above.

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
