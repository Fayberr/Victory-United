# Selected s908 and s909 fold

Base: the requested `integ/c17`, resolved to `d27c75472` on 2026-11-27.
This ref contains the Vice-Tay fold but does not contain dgcf `6334c35c8`
or the combined s912 worker fix `821ac0391`. The existing compute wait limits,
frame budget, FRAME_WAITED and panic-surviving worker are unchanged. No dgcf
rescue code was present to preserve. The manager must reconcile that base
before treating this as the combined Vice-Tay plus dgcf tree.

## Picks

| Source | Local pick | Result |
| --- | --- | --- |
| `feb8d6ea4` | `8281e72dd` | In-place range merging and indirect-argument capture |
| `852fde9e0` | `c5b09b8ce` | Mesh stride uses all thread dimensions and output counts |
| `2ce1369f8` | `be96ec873` | Local Windows timestamps on stderr |
| `9532fb892` | `f4a11804f` | Per-present slow-frame warnings and errors |
| `e4a5ea7ac` | `53a1de6ad` | Texture queue budget aliases |
| `211dbb28b` | `5e3cd2573` | Larger remap queue and one overflow rescan |
| `fad015ac6` | Skipped | Base already passes a null pointer for an empty pipeline-library blob |

All other s908/s909 work was excluded, including the Proton DXC fallback and
64-lane denormal/half-wave changes. The timestamp commit's inherited AI
co-author trailer was removed while preserving source provenance.

## Integration details

- Preserve the existing range compaction that covers every changed range.
  Remaps now retain up to 1,048,576 ranges and request a full placed-slot
  rescan on overflow. Page writes retain their 32,768-range limit.
- Remove stray incoming closing braces from the slow-frame and remap patches,
  and the duplicate diagnostics attribute from the argument-capture patch.
- Bump TRANSLATOR_VERSION once, from 7 to 8, for changed mesh shader output.
- Keep `&& !is_spread` in the indirect batch condition. Leave `lower.rs`,
  all image shader modules and `pipeline/make.rs` unchanged from the base.
- No uncertain conflict resolutions. The base mismatch above remains open.

## Switches and defaults

- New `VICTORY_TEXTURE_QUEUE_MB` is an alias used only when
  `VICTORY_TEXTURE_STAGE_MB` is absent. With both unset, budgets remain
  adaptive: 96/192/256/384 MiB by VRAM budget, normally 192 MiB for a 12 GiB
  adapter, capped at 128 MiB for limited host RAM. Existing pressure limits
  and pass-specific budgets still apply.
- New `VICTORY_TEXTURE_QUEUE_MS` is used only when
  `VICTORY_TEXTURE_STAGE_MS` is absent. Default: 250 ms. The pick's title
  says 1024 MB/1000 ms, but its code does not set those defaults.
- Existing `VICTORY_TEXTURE_CATCHUP` remains ON by default and triples the
  normal byte/time budgets when at least 1024 slots are due.
- Argument capture adds no switch. It is OFF by default and uses existing
  diagnostics capture settings, including `VICTORY_CAPTURE` and
  `VICTORY_CAPTURE_FRAMES` or world-selected frames.
- Slow-frame logging is ON by default, without a new switch: warning above
  100 ms, error above 500 ms. The first present has no interval to log.
- Local timestamps are ON by default for Windows stderr. File logs keep
  their existing UTC timestamps. No new timestamp switch.

## Verification

Windows MSVC workspace/all-targets checks use `/tmp/vu-cargo.lock` and private
`CARGO_TARGET_DIR=/tmp/vu-target-s913`, backed by `/dev/shm/vu-target-s913`
because the disk was full. Sources are touched before each check.

No game execution, Windows runtime validation, picture comparison or FPS
measurement was performed. Nothing was pushed.

Both checks passed with exit status 0 and no compile errors:

- Diagnostics (`--features victory/diagnostics,victory-gpu/diagnostics`):
  1m 45s, log `/tmp/vu-s913-diag.log`.
- Without diagnostics: 31.65s, log `/tmp/vu-s913-no-diag.log`.

Both used `cargo xwin check --workspace --all-targets --keep-going
--message-format short --target x86_64-pc-windows-msvc`. Warnings remain.
The upstream tests were compiled by the all-targets checks, not executed.
