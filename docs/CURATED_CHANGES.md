# VIctory Curated

This local WinPC build combines compatibility and performance changes on
the `curated/perf-fixes` line (now `main` of Victory United). The source checkouts
for VIctory, victory-fork, and victory-perf retain their own launchers,
configurations, shader caches, and save directories.

## Changes by checkpoint

- `checkpoint-base` (`f85101b`) is the victory-fork base.
- `checkpoint-wave1a` (`6ff54b6`) integrates NVIDIA Wave32 and Wave64 lowering, linked
  mesh and pixel shaders, descriptor and CBV fixes, and extra texture formats.
- `checkpoint-wave1b` (`433cf42`) adds on-demand texture residency,
  DirectStorage fixes, resource alias and view restoration, and page-set
  optimizations.
- `checkpoint-wave2` (`f18a638`) adds victory-perf batching, copy-back and
  barrier work, shader and compute pipeline precompile, and the lighting and
  exposure fallback paths.
- `checkpoint-wave3` (`7341d8e`) adapts the upstream XMA, DSP and WASAPI patches
  (Marvisak/VIctory #1537, #443, #1880, #240 and #478), upload reuse, faster
  GPU bookkeeping and fallible pipeline-save allocation (#1894, #2102 and
  #2007), and instruction decoder and IR optimizations (#1623, #2012, #1033
  and #2037). It also adds cube-face and MSAA loads, mip-zero and partial
  image stores, and integer stores for descriptors shared with atomics
  (#1955 and #1956 with their prerequisites). Existing texture staging
  scratch reuse covers #1873.

A release follow-up (`3d13942`) tracks all scalar destination codes in the
unspill pass after prewarming exposed a crash. A second follow-up
(`0dbf043`) consumes VOP3 no-op literals even though those instructions
read no registers. Before the fix, the translator treated those literals
as instructions. Both cases have regression tests.

The image lowering lives in `crates/victory-shader/src/emit/image.rs` and
participates in the shader cache fingerprint. Integer stores preserve all
input bits. Float stores retain the existing NaN handling and lighting
fallback. The loopback network test explicitly binds sender and receiver
and verifies payload delivery, which avoids Windows auto-bind failures in
remote sessions.

## Use on WinPC

- `launch-curated.cmd` uses a Series S profile, half
  render scale, on-demand texture residency and the lighting fallbacks.
- `--series-x` uses Series X with the curated performance flags. `--raw`
  starts the binary with its defaults. Those modes are useful for comparing
  rendering at different settings.
- `--bind-only` checks executable and DLL imports without running the title.
  `--prewarm` fills the curated shader and compute pipeline caches.
- a local update script rebuilds the checked-out local
  branch using the installed Windows MSVC toolchain and native CPU target.

The curated cache is a separate cache folder. The curated
save and runtime directory is a separate data folder.
The source launcher is `launch-curated.cmd`. The shortcut batch file in the
parent folder delegates to it. It resolves the existing title files beside
the checkout.

## DirectX runtime

This build exports Agility SDK version 620. WinPC uses the Microsoft
`Microsoft.Direct3D.D3D12` package at `1.620.2`. The app-local
`target\release\D3D12\D3D12Core.dll` is the package's signed x64
runtime. The update script calls `stage-agility.ps1` to restore the x64 DLL and
license after a clean build. The helper pins the package and DLL checksums
and verifies Microsoft's Authenticode signature before staging. The
runtime is excluded from Git.
[Microsoft package](https://www.nuget.org/packages/Microsoft.Direct3D.D3D12/1.620.2)

The local `victory.toml` contains a `[paths]` table. Set `VICTORY_LOG`
in the environment when you need to adjust logging.

## Validation

- Windows MSVC release built with `-C target-cpu=native`. The config check
  and launcher bind-only check succeeded with zero missing imports for
  the executable and all title DLLs. The update script passed its smoke
  test and preserved the verified executable.
- Asset policy and Clippy passed for the workspace with and without all
  features. The final workspace suite passed 785 tests, with 11 existing
  ignored tests. Two WASAPI capture tests also passed with diagnostics on.
- Full RTX 3080 Ti prewarm processed 25,956 unique containers in 808 seconds.
  It translated 25,757 shaders and reported 199 translation failures.
  It compiled 2,453 compute PSOs and loaded 280 from the pipeline library,
  with zero compute PSO refusals. The resulting library is 531,014,456 bytes.
- The same two deterministic input samples match victory-perf at 1,400 of
  1,400 archive shaders and 12 of 15 executable containers.
- Executable SHA-256
  `786fc8452ff97e82b6b84aff3c28ead9af89d5faa8a905f2edab6b38db18718e`.

In-game FPS and gameplay remain unverified.
Offline prewarm covers shader translation and compute PSOs. Graphics PSOs
first encountered in the title may still compile during gameplay. Some
console shader operations and resource cases remain unsupported.
