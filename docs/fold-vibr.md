# vibr/main fold

Source: edsuuu/VI-BR-PORT `vibr/main`, GPL-3.0-or-later, head
`31b9a82fb260f9bad6acc41b47ec4e0571e8febb`. Base: `777c75b67`.
Merge: `91ca0bb88`. All 25 previously unmerged commits are ancestors.

## Accepted behavior

- `2f59f1dfd`: adjacent ranges of one card copy share a compare dispatch.
- `389b97473`: cards with a local budget at most 12 GiB use a 1 MiB minimum
  and 1024 MiB copy budget. Unknown budgets retain 16/512 MiB.
- `ab9ae781f`: the play script sets `VICTORY_CARD_BUFFERS_MIN_KB=64`.
- `a04f6012e`: optional omission of ray-only shadow passes, subject to the
  existing all-rays-miss and disabled-BVH conditions.
- `557d2f038`: the air-light bound also works with constant ambient light.
- `e600d22cf`: refused compute pipelines log requested DXIL wave lanes.
- Play settings from `11169aab1`, `1f9d6f791`, `2aa1895d7`, `a31b9b128`
  and `31b9a82fb`, with the final upstream values rather than intermediate ones.

## Defaults

| Setting | Engine default | Play script |
| --- | --- | --- |
| `VICTORY_CARD_COMPARE_JOIN` | On, `0` disables | Inherits engine default |
| `VICTORY_SKIP_RT_SHADOW` | Off, `1` requests skipping | Unset |
| `VICTORY_RT_SHADOW` | Unset; `1` keeps shadow passes | Unset |
| `VICTORY_CARD_BUFFERS_MIN_KB` | Unset, uses adapter MiB minimum | `64` |
| `VICTORY_CARD_BUFFERS_MB` | `1024` on at most 12 GiB | Existing 4070 override remains `1536` |
| `VICTORY_CARD_AWAKE` | Off | `0`, existing 7900xtx override remains `1` |
| `VICTORY_AMBIENT` | Unset/off, unless an RT ambient band supplies an estimate | `constant:0.1` |
| `VICTORY_AMBIENT_AIR` | Unset/zero leaves title behavior | `0.8` |
| `VICTORY_PORT_CONSTANTS` | Off | **On (`1`)** |

Shadow skipping requires `VICTORY_SKIP_RT_SHADOW=1`, `VICTORY_RT_MISS=1`,
`VICTORY_BVH_BUILDER` other than `1`, and `VICTORY_RT_SHADOW` other than `1`.
The new runtime switch enabled by default is compare joining. The launcher
also enables the already-existing renderer port and changes ambient light.
These play defaults require picture checks by the manager.

## Exclusions and protected local implementations

- `383b56054` and `225c8c04b`: omit Brazil caption, title icon, and the
  title-directory plumbing used only for that icon. Keep VU window behavior.
- `3f1fc4036` and `ef3bac241`: keep the local README and measured claims.
- `9fff527c4`: retain the existing atomic, multi-key `VICTORY_KEYS_FILE`
  reader with explicit hold durations. Do not add a second reader that can
  consume and delete the same file. Bare-key input remains unsupported.
- `5af02b4dc`, `fa9411b4f`, `6349f8c0e`: renderer port and xmem work is
  already present. Keep the local mapping logic, including support for a
  backing section larger than a reservation and the adapted WARP test API.
  The conflicting upstream mapping diagnostics were not added.
- `189ee55a7`, `a07a5869c`, `221a2a098`, `22f3426d4`, `a06da66fd`,
  `5c17c8c83`: retain the existing shader, descriptor and texture fixes.
  Keep stricter mixed float/half handling, write tracking, current table-plan
  interfaces, heap-id release arguments and existing tests.
- Keep the local play script's removal of unconditional indirect batching,
  map-thread and skip-pipeline overrides. Existing conditional machine and
  non-safe profile settings remain.
- Preserve `&& !is_spread` in batch selection, wave64-to-wave32 lowering
  without the `!partial` restriction, and the image split's a16 atomics,
  counter stores, ROVs, `nan_guard` and `sampler_phi_choice`.

## Verification

Compile checks use the dedicated `/tmp/vu-target-fold-vibr` directory,
`flock /tmp/vu-cargo.lock`, `--workspace --all-targets --keep-going`, and
`--target x86_64-pc-windows-msvc`, both with
`--features victory/diagnostics,victory-gpu/diagnostics` and without them.
Both checks exited successfully with zero errors. The diagnostics check
took 1m 52s; the check without diagnostics took 30.03s, excluding lock waits.
Warnings remain; test targets were compiled, not executed. No game run or
speed claim is part of this fold.
