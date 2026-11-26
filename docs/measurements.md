# Measured runs

One run per entry. Numbers come from the project's own frame log, summarised by script
(median of the five-second frame reports, with intervals that still had shader pipelines being
built left out; in this run none did).

## 2026-11-26, commit db89ba3b, 35 minute run

- Machine: Windows 11 Pro build 26300, RTX 3080 Ti 12 GB (driver 32.0.16.2124), Ryzen 7 5700X3D, 64 GB.
- Binary: release build with diagnostics logging, SHA-256 `8F3BEBCFA046D10DB6842810452D3224F7D87C6AE0C2D674D5C82BA68224CA51`.
- Console profile: Series S. Scripted keys (scan code:key): `18:Enter,35:Enter,46:X,60:Enter`.
- Environment switches: `VICTORY_RELAY=2`, `VICTORY_LATE_WAIT=3`, `VICTORY_WIDE_BACK=1`,
  `VICTORY_RENDER_SCALE=0.5`, `VICTORY_OWN_UPSCALER=1`, `VICTORY_CARD_BUFFERS_MB=2048`,
  `VICTORY_CARD_BUFFERS_MIN_MB=1`, `VICTORY_LEVELS_ON_DEMAND=1`, `VICTORY_LIBRARY_TRIM=0`,
  `VICTORY_REBAR=0`, `VICTORY_COPY_BACK_ONLY_READBACK=0`, `VICTORY_CARD_ACCOUNTING=1`,
  `VICTORY_PROFILE=1`, `VICTORY_CENSUS=1`, `VICTORY_CAPTURE_WORLD=1`, stubs
  `VICTORY_EXPOSURE_STUB=1`, `VICTORY_AMBIENT_STUB=1`, `VICTORY_SHADOWMASK_STUB=1`.
- Data: a fresh copy of throwaway test data, not a real save.
- Result: 346 samples from 115 s to 1990 s after the first world frame. Median 4.8 fps (frame
  208 ms), 10th percentile 4.3, 90th 5.1, min 3.4, max 5.3. Median 4,369 draws a frame. No
  pipelines pending in any sample. Longest gap between samples 5.9 s.
- Ending: the script closed the game at its 35 minute deadline (exit code 0). No crash, panic or
  device-removed line in the log.
- Not measured: any other machine, any real save, any open world area, any comparison against
  an earlier commit under identical settings.

## 2026-11-26, paired run: `cf5d6000` against `ac63840d` (14 performance changes)

- Machine, console profile, scripted keys and environment switches: as in the entry above.
  Both runs used the same game data seed and the same starting shader and pipeline cache (each
  run got its own fresh copy of the cache), one after the other, 6 minutes each.
- Base: `cf5d6000` (same code as `db89ba3b`), release build with diagnostics logging, SHA-256
  `D33A22056D5C61EC238AB271FC66BB89423C036C759EE3DAF2418B7960E40A04`.
- Candidate: `ac63840d` (base plus 14 performance changes taken from other projects, listed in
  `PROVENANCE.md`), same build type, SHA-256
  `E212CF09B85F7E254CAF0CB46DB9829CE5B0DB4C55F4AE8D67DCEADFEAC3F18D`.
- Result, last 20 five-second reports of each run with 4,300 to 4,700 draws a frame (the room
  was settled in both): base median 250 ms a frame (4.0 fps) at 4,443 draws; candidate median
  199 ms a frame (5.0 fps) at 4,488 draws. Frame time 21 percent lower.
  Heap buffers pass 106 ms against 72 ms; preparing 83 against 80 ms (copies 48 against 45 ms);
  copying back 40 against 38 ms.
- Both runs: no crash, panic or device-removed line in the log. The script closed the game at
  the 6 minute deadline; it had to end the process after its 5 second close wait (exit code 0).
- The last real captured frame of each run was looked at: the room is drawn cleanly, with no
  bright blocks. The two frames show different moments of the room's camera animation, because
  the slower run had played less game time.
- Not measured: more than one run per build, any other machine, a long run of the candidate,
  and the picture compared pixel for pixel.

## 2026-11-26, avatar merge: `3588725d`, one 15 minute run

- Machine, console profile, scripted keys and environment switches: as in the entries above. Same
  game data seed and a fresh shader and pipeline cache. Release build with diagnostics logging.
- Build: `3588725d` (VU `main` with the whole performance branch of VIctory-7900XTX `e2a28ec7`
  merged in; conflicts resolved by hand, the tree compiles with and without the diagnostics
  features), SHA-256 `F72F14297C8683682EBF00ADB40124539628AECE7959FEF2632AE4B5E06C7B81`.
- Result: 112 five-second reports over the run. Median over all of them 9.3 fps (107 ms a
  frame). Last 20 reports with 4,300 to 4,700 draws a frame: median 103 ms a frame (9.7 fps) at
  4,328 draws; preparing 58 ms (copies 25 ms), copying back 25 ms, on the GPU 28 ms. The last
  three reports show 98.5 to 101.7 ms (9.8 to 10.1 fps). Heap buffers pass about 20 ms (median
  over the run), against 72 ms in the `ac63840d` run.
- Ending: the script closed the game at its 15 minute deadline and had to end the process after
  the close wait. No crash, panic or error line in the log, and no swap chain failure.
- Not measured: a paired run against another build in this session (the 5.0 fps of `ac63840d`
  and the 7.9 fps of VIctory-7900XTX `e2a28ec7` come from earlier runs of 6 minutes and longer),
  more than one run, any other machine, any real save, player control in the room.
