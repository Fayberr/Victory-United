# KLBF Lab patch notes

Every KLBF Lab version, newest first. Measured on one machine: Windows 11, AMD Radeon RX 9070 XT, Ryzen 7 9800X3D, 32 GB RAM. Full run log: [docs/klbf/results.md](docs/klbf/results.md).

Product and controller names (Xbox, DualSense…) are only used to say what a build works with. They belong to their owners; KLBF Lab is not affiliated with or endorsed by them.

## Lab 62 — Test build `beta-2` — 26 Nov 2026
- **Sharper textures, far fewer freezes:** textures capped at 1024 px by default (`VICTORY_TEXTURE_MAX_DIM=1024`, Evan-Lowry's cap); `-LowMemory` now sets 512.
- **Loading screens and menus kept sharp:** screen-sized and short-mip images are left at full size under the cap (`VICTORY_TEXTURE_MAX_DIM_KEEP_UI`, on by default).
- **AMD pipeline-library lock available** (Ammar Zubair, against amdxc64.dll crashes): on by default on AMD in the code, off in `play.ps1` (`VICTORY_LIBRARY_LOCK=0`, the serialized compile was too slow in play).
- **Texture LRU** (samishating, from AKA0GHOST/victory-fork): `VICTORY_TEXTURE_LRU=1`, off by default.
- **Result:** measuring (real play session in progress).
- **Known issues:** road paint markings and some small props (parking rocks) flicker, bushes look coarse; plus Lab 61's (shadows near trees, black mirrors, invisible car glass, Series S resolution upscaled).

## Lab 61 — Test build `beta-2` — 26 Nov 2026
- **Base:** Neptune 2026-11-25m (Traffic & zm0kiemedia) with WillyboPeep's changes, face fixes (proneonn, tippythetaidum), fxceless/victory-fork#1216 (GamingRipperDoc) and #1213 (IxMxAMAR, MalfoyDraco99), zm0kiemedia's clamps and avatarmanz's renderer port 1 and 2.
- **Ours:** the crash-surviving pipeline pre-build and skip list, KLBF Lab names, `play.ps1` with our measured settings.
- **New:** `VICTORY_DYNAMIC_SLOTS=786432` set by default (about 37 draws a frame were dropped for lack of descriptor slots: flicker fixed); `-LowMemory` switch sets `VICTORY_TEXTURE_MAX_DIM=512` (Evan-Lowry's texture cap: fewer freezes when memory is full, softer textures).
- **Result on RX 9070 XT, Series S + FSR 4, real play with a controller:** 20–23 fps standing still looking at the sky, 15–17 in traffic, up to 14 on foot in the open world (varies with the area), 9–12 driving fast with short dips while new areas load. Occasional short freezes in any situation (on foot or driving); they do not crash the game and it stays playable. `VICTORY_PASSES_AHEAD` stays at its default (=0 made vegetation and shadows flash).
- **Known issues:** occasional freezes when memory fills (`-LowMemory`), some shadows flicker near trees, mirrors stay black (need ray tracing), car windows (glass) are not drawn, Series S resolution upscaled.

## Lab 39 (test) — 25 Nov 2026
- **Water physics fixed:** the ocean height map the GPU computes is copied back into the game's memory (from Geckomacabre/vx-layer), and XOR swizzle modes are addressed (from MiamiGame/XTP, Mesa addrlib tables). Boats float and move.
- **Menus and HUD in Series S mode** (Lab 38): the interface plane is composed over the game image.
- **AMD FSR frame generation**, optional: `VICTORY_FRAMEGEN=1` (~8.5 real fps → ~17 shown).
- **Pre-build** resumes on a fresh cache ([#6](https://github.com/PastiiJay/klbf-lab-beta/issues/6)).
- Base: latest fxceless/victory-fork + our AMD fixes (Lab 35/37).

## AMD test branch `klbf-amd` — 24 Nov 2026
- **Base:** fxceless's earlier fork `578d459` + VIctory #1599.
- **AMD fixes:** memory leak (shader `39f0e9aeb156a61c` stuck in the AMD compiler, skipped by default); 32-lane pixel shaders run per half wave on 64-lane hardware (purple blocks); NaN/Inf filter on colour outputs; larger, configurable texture upload queue (late/missing textures).
- **Result on RX 9070 XT:** room fully lit, ~4.4 fps stable; 6–9 fps with experimental settings (device removed after ~3 min). Ambient light still purple; triangle edges, dark faces remain.

## Lab 23 (current) — 24 Nov 2026
- **Base:** fxceless's earlier fork at `578d459`, unchanged. All credit to its author.
- **What it does on our card:** the first gameplay scene is drawn (characters, buildings, the yard); ~2 fps after warm-up; no device removal.
- **Known problem:** committed memory keeps growing; our run stopped at 514 s when Windows ran out of virtual memory.

## Lab 21 / 22 — 24 Nov 2026 (test builds, not published)
- Shader coverage fixes on the Lab 15 base: #1250, #1230, #1273, #1301 (+ two renamed instructions), then #1130, #298, #1074, #385, #1412, #953, #539, #704 + #193.
- **Result (Lab 21):** 25 min with no device removal, median 283 ms/frame (~3.5 fps) in the first gameplay scene, mission objective shown, picture still dark.

## Lab 15 — 24 Nov 2026 (tag `klbf-lab-15`)
- **Base:** the [AnyXboxX](https://github.com/LooperSalty/AnyXboxX) integration by LooperSalty, plus #431 (AntoninMichelq) and #67 (alessiodeia).
- **Result:** 25 min in the world with no device removal; ~1 fps; rough world shapes visible.

## Lab 03 — 24 Nov 2026 (tag `klbf-lab-03`)
- VIctory + #452, #431, #67, #584 (limit raised), #493.
- **Result:** 1,006 s with no device removal; pad input and rumble work; black world.
