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
