# DLSS integration status

`integ/dlss` is based on `integ/c20x` at `13b520f90`. The branch ports
NVIDIA Streamline Super Resolution and 2x frame generation from VIctory_sm86.
Both are opt-in. Shader translation still uses `TRANSLATOR_VERSION=17`.

- `VICTORY_DLSS=1` uses Streamline Super Resolution. Select `quality`,
  `balanced` or `performance` with `VICTORY_DLSS_MODE`.
- `VICTORY_FRAMEGEN=dlss` selects NVIDIA frame generation. The existing
  `VICTORY_FRAMEGEN=1` value selects AMD frame generation.
- Supply NVIDIA Streamline SDK 2.14.16 in `VICTORY_STREAMLINE_DIR`. For
  the RTX 3080 Ti, supply the optional dlssg_for_sm86 release in
  `VICTORY_DLSSG_SM86_DIR` and enable Hardware-accelerated GPU scheduling
  before trying frame generation. See [setup](docs/dlss.md).

## Verification

The Windows MSVC workspace checks passed for all targets in both the plain
and diagnostics configurations. Seven Streamline ABI and camera tests also
passed on the Linux host, along with 233 shader tests (3 ignored). Hardware
runtime results, recorded visual review,
VRAM use, latency and performance on the RTX 3080 Ti are pending. The branch
includes no runtime DLLs. If Streamline fails after boot, the optional PC FSR
provider can take over. Full-resolution native fallback after a dispatch
failure depends on restarting with Streamline disabled if PC FSR is absent.

## WinPC acceptance

Use the same warmed save, camera route, display and 1440p output for each
recording. Clear separate upscaler or scale overrides from the launcher.
Keep the ordered submission settings constant across the three runs.

```powershell
$env:VICTORY_LOG = 'info'
$env:VICTORY_OUTPUT = '2560x1440'
$env:VICTORY_STREAMLINE_DIR = 'C:\sdk\streamline-sdk-v2.14.16\bin\x64'
$env:VICTORY_LATE_WAIT = '3'
$env:VICTORY_RELAY = '2'
$env:VICTORY_THIN = '1'
# Run with these switches unset for the baseline.
Remove-Item Env:VICTORY_DLSS, Env:VICTORY_FRAMEGEN -ErrorAction SilentlyContinue
# Run again with Super Resolution.
$env:VICTORY_DLSS = '1'
$env:VICTORY_DLSS_MODE = 'quality'
# Run a third time with Super Resolution and frame generation.
$env:VICTORY_FRAMEGEN = 'dlss'
$env:VICTORY_DLSSG_SM86_DIR = 'C:\sdk\dlssg_for_sm86'
```

Run `victory.exe run "<title-exe>" --execute --console series-x` at each step.
For Super Resolution, look for `Streamline: DLSS Super Resolution loaded` and
`Streamline DLSS: recorded the title's upscaling`, around 1706 by 960 into
2560 by 1440. For frame generation, look for
`DLSS Frame Generation: loaded dlssg_for_sm86 before Streamline`, the
active swap-chain proxy, first depth and motion tags, `on=true` and a
`shown` count above `rendered`. A menu or loading interval without a
new dispatch should turn interpolation off.

Record each run from start to exit and review the full recording. Keep the
logs and note timestamps for colour, lighting, textures, reflections, HUD
distortion, ghosting or stutter. Compare rendered FPS and frame times first,
then displayed frames and latency with frame generation. Include an exit,
a menu transition, and a resize in the frame-generation run.

## Branch state

The integration is on `origin/integ/dlss` for hardware acceptance and a
later pull request. At verification, the separate local `main` worktree was
clean and 2,025 commits behind `origin/main` (`6757b6eff` versus
`f0be576e3`). It can be fast-forwarded independently of the DLSS branch.
