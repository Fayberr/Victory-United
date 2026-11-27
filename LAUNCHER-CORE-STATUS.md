# Launcher core status

Branch `launcher/core`. Everything below is pushed. Windows gates (`/tmp/vu/chk-launcher.sh`, both runs) passed with no errors.

## Done
1. Theme, shell, `--ui-demo` and `--ui-demo --capture <dir>` (11 screens incl. first run and warm-up). Verified hidden on the WinPC. `capture ready` was written before the other steps.
2. `settings.toml`, game detection by `MicrosoftGame.Config` (folder, `Content` subfolder, exe pick, drag and drop, auto scan of XboxGames and similar roots one or two levels deep on fixed/removable drives), first-run screen.
3. Hardware check via DXGI and D3D12 on a worker thread, device dropped at once. Verified on the WinPC (RTX 3080 Ti, mesh shaders yes, driver 621.24, 8c/16t, 64 GB). Thresholds (16 GB RAM, 6 GB VRAM, old driver) are judgment calls, not measured.
4. `data/presets.toml` (ported from `scripts/play.ps1`: 7900xtx, 4090, 4070, 3080ti; auto pick like the script), options, upscalers and frame generation as data. Resolution order preset, options, edits, extra flags; tested.
5. Play flow: warm-up (`victory.exe shaders`), progress bar with ETA, Skip and Cancel, engine start with log in `logs\run-*.log` (last 20 kept), Stop kills the tree via `taskkill /T`, exit report, quick-exit recovery. Blocking threads only (stdout read, `Child::wait`). `apps/victory/src/translate.rs` now prints `VICTORY-PROGRESS shaders <bytes done> <bytes total>` (bytes of files scanned, size-only pre-pass).
6. Settings tab: preset, options, 4 groups of curated fields, changed markers, per-field Reset and Reset to preset, profiles (new, duplicate, rename, delete, export, import), engine row, final launch preview with Copy. DLSS and frame generation are data driven and show as unavailable with their reason.
7. Flags tab and `data/flags.toml`: 558 names, 51 documented by hand, coverage test fails when the code reads an unlisted flag. Add by name with autocomplete, edit, remove, enable/disable, `launch-flags.txt`, "Start without my flags", quick-exit recovery.
8. Bug report: previews with checkboxes, ZIP in `reports\`, anonymizer (profile path and user name), issue text to the clipboard, nothing uploaded.
9. About tab, `docs/launcher.md`.
10. 30 unit tests pass on Linux (`cargo test -p victory-launcher`).

## Measured (WinPC, release build, hidden)
- Idle, normal start (Play tab): 0.03 percent of one core over 60 s, working set 95 MB (close to the 100 MB limit).
- `--ui-demo` run (game "running" state, Play tab): 0.08 percent of one core, 78 MB.
- The Monitor tab and its once-a-second repaint belong to the ops worker and were not measured here.

## Not done or not verified
- The real Play flow was never run against the real engine and game (not allowed for this worker). Tested only with a fake engine script on Linux (warm-up progress, log, exit code). The `VICTORY-PROGRESS` lines compile under the Windows gates but were not run.
- The `progress_total` unit test in `apps/victory` compiles but cannot run on Linux; not run on Windows.
- "Running for" text and the warm-up ETA refresh only on events (no timer, by the idle rule), so they can look stale until input or the next progress line.
- File dialogs (`rfd`), drag and drop, clipboard and folder buttons are Windows code that was compiled, not clicked.
- Presets for 4070 Ti Super, 3060 and laptop named in the brief do not exist in `play.ps1`, so there are none. The 4070 preset is never picked automatically (same as the script).
- Generated flag types and defaults for the 507 undocumented flags are guesses from how the code reads them.
- `Cargo.toml` of the launcher gained `image`, `zip`, `rfd` and `windows` dependencies; `Cargo.lock` needs the lead's merge. `paths.rs` gained `launch_flags_file()`.

## For the lead
- After merging engine changes run `python3 crates/victory-launcher/tools/gen_flags.py` so new `VICTORY_*` names are in the registry (VICTORY_STATS, VICTORY_DLSS, VICTORY_DLSS_DIR, VICTORY_FRAMEGEN are already listed).
- Keep `scripts/play.ps1` and `presets.toml` in step.
