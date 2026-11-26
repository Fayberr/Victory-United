# Where the changes come from

Since 2026-11-26 the tree of this repository is [TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)
at commit `fab5286c` (GPL-3.0-or-later). Every commit of that history is kept with its original
author, and so is the history of this repository before the switch: the merge commit that joins
them keeps both. Vice-Tay's own list of the trees it is built from is in its
[credits](docs/credits.md) and in the [README](README.md#credits).

## Why the tree was switched

The tree this repository had before (`6757b6eff`, tagged `archive/main-2026-11-26`) was built by
merging pull requests and branches from [fxceless/victory-fork](https://github.com/fxceless/victory-fork),
[Marvisak/VIctory](https://github.com/Marvisak/VIctory), [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX),
Belobug, vitutu1/victory-perf and others (the list that was here is kept in the archive tag's
`PROVENANCE.md`). On the test machine (RTX 3080 Ti) that tree ran at 8.5 frames a second in the first room, while
Vice-Tay's own head ran at about 11 frames a second there, so the tree was switched and the other work is folded in again one piece at a time, checking the
picture after each.

## What has been taken since

### NVIDIA and Avatar fold, 2026-11-26

This fold starts at `777c75b67`. It records two separate merges, in this order:

- [trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA),
  `nvidia/main` at `6a31e431e`, merge `41b2e8856`: superboynick's mouse device
  (`12b1d16d3`), Indy2l's texture read-back worker (`2d1dc3826`, already present
  in this tree), descriptor view-table reporting (`b8fcd99b9`, author recorded
  upstream as Local Test), and the NVIDIA launcher BAR cap changes by Indy2l,
  Jeremy Goldberg and trustno1-coder. All eleven upstream commits are ancestors.
- [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX),
  `avatar/main` at `014d52ded`, merge `bfce5cb43`: all seven new commits
  `e9428969f`, `5d6bd8418`, `cd8dfc06a`, `1e7286109`, `84917c34f`,
  `d13e9e0c6`, `014d52ded`. beloooo's submission-prelude texture uploads and
  optional PC FidelityFX port (originally DexNom `ab34a8b2`) are included,
  along with the upstream RT port and its diagnostics/ignored GPU benchmark.
  The RT commits' author is recorded upstream as local.

Both sources retain their original history and attribution. No upstream commit
was excluded from ancestry. This is a compilation candidate, with no game,
picture, frame-rate or driver acceptance result yet.

#### Choices made while merging

- Keep the local spread-dispatch batching exclusion, wave64-to-wave32 lowering,
  and the image emitter unchanged (a16 atomics, counter stores, ROVs,
  `nan_guard`, `sampler_phi_choice`).
- Keep PS5 input, scripted keys, dynamic table release, ranged title-write
  reporting, per-list movie-plane upload buffers, exact decoration-bake reads,
  the NVIDIA pipeline-worker stack size and shader deny list.
- Keep this tree's newer diagnostics-only HUD and fence-owned upload buffers.
  Do not replace them with Avatar's older HUD or enable it in plain builds.
- Restrict RX 9070 XT texture caps, descriptor replacement/mip-clamp policy,
  indirect-batch pooling and compiler recovery to AMD adapters. NVIDIA keeps
  its previous behavior for these paths. An explicitly requested texture cap
  is supported on other adapters too.
- Keep the existing texture-eviction thresholds and Cargo build profiles.
  Avatar's changed thresholds and thin-LTO settings are not taken. Its ambient
  filter duplicates the filter already in this tree, so keep the existing
  shader unchanged. README and lockfile retain this tree's versions.

#### Switches and defaults

- `VICTORY_GAMEPAD_MOUSE`: **on by default**, `0` disables the focused-window
  mouse device. Texture read-back worker `VICTORY_TEXBACK_ASYNC` remains
  **on by default**; `0` lands writes synchronously.
- `scripts/play-nvidia.ps1 -WindowsBarMB`: unset caps mapped VRAM windows at
  3072 MiB on PCs with less than 24 GiB physical RAM, otherwise leaves the
  runtime cap off. An explicit `0` disables the cap; `$More` can override it.
- `VICTORY_PORT_RT`: off, `1` enables RT part 1 (placed-copy ordering and
  fill forwarding). `VICTORY_RT_STAMPS`: off, `1` enables dispatch timestamps
  in diagnostics builds.
- `VICTORY_FSR_4K`: off, accepts `quality`, `balanced` or `performance`.
  `scripts/play.ps1 -Fsr4K` selects the same presets. Requires Series X and
  an external `amd_fidelityfx_upscaler_dx12.dll` in `VICTORY_FIDELITYFX_DIR`
  or `[paths].fidelityfx_dir`. No provider DLL is included.
- `VICTORY_TEXTURE_MAX`: AMD defaults to 1024 pixels, then 512 at 92 percent
  VRAM use and 256 at 97 percent; other adapters default to uncapped. `0`
  disables the cap, a positive value explicitly sets it on any adapter.
- `VICTORY_COMPILER_GUARD`: **on by default on AMD only**, `0` disables it.
  `VICTORY_WARM_BAD`: **on by default inside that guarded path**, `0` ignores
  the saved bad-pipeline list. NVIDIA retains the existing initializer and
  does not load that list or install the recovery handler.

#### Verification

NVIDIA alone passed the Windows workspace/all-targets diagnostics check.
Final combined-tree diagnostics and plain Windows checks are required before
this candidate is handed back. Runtime acceptance belongs to the manager.
