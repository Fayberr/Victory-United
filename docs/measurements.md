# Measurements

Build 672cf831 (`cargo build --profile diag -p victory --features diagnostics`), 2026-11-26,
on the test PC the README describes. Each session: `scripts\play.ps1` with the
RTX 4090 settings, full screen, past the start screen into the prologue, then
every minute a capture, the camera turned, a walk, and a route (run, turn,
look around, step aside) with another capture.

Frame columns are medians over the world lines (once a second, over 1,000
draws a frame). Memory and card columns are samples every 20 s over the whole
session, loading included.

| Profile | world lines | fps median | p10 | p90 | 1% low | ms/frame | preparing | copies ms / MB | textures ms / MB | GPU ms | copying back ms / MB | draws | dropped draws | captures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Series X | 123 | 14.3 | 13.6 | 14.7 | 3.5 | 69.7 | 24.3 | 10.8 / 90.0 | 0.4 / 21.5 | 35.0 | 20.0 / 39.0 | 4893.0 | 334.0 | 46 |
| Series S | 117 | 20.8 | 19.2 | 21.8 | 8.7 | 48.1 | 36.9 | 14.2 / 97.7 | 0.5 / 19.0 | 0.2 | 15.8 / 41.4 | 4591.0 | 257.0 | 42 |
| Series X, `-Safe` | 118 | 15.4 | 13.5 | 17.8 | 3.3 | 64.7 | 37.3 | 9.8 / 61.3 | 2.5 / 30.9 | 5.2 | 14.4 / 18.6 | 4233.5 | 94.0 | 47 |
| Series X, `-RayTracing` | 118 | 8.9 | 6.8 | 11.4 | 4.8 | 112.3 | 35.9 | 14.8 / 88.3 | 0.6 / 35.2 | 63.0 | 14.9 / 17.9 | 4004.0 | 1.0 | 44 |
| Series S, 70 minutes played through the prison | 775 | 12.9 | 8.3 | 20.1 | 5.0 | 77.2 | 45.5 | 16.7 / 97.3 | 0.7 / 32.6 | 0.0 | 16.1 / 36.9 | 5314.0 | 204.0 | 276 |

| Profile | samples | game memory median / max (GB) | working set max (GB) | card memory max (GB) | card load median (%) | power median (W) | temperature max (C) | clock median (MHz) | CPU seconds per minute |
|---|---|---|---|---|---|---|---|---|---|
| Series X | 39 | 29.3 / 30.6 | 30.5 | 13.6 | 90.0 | 152.9 | 57.0 | 2745.0 | 543 |
| Series S | 36 | 24.2 / 26.0 | 27.8 | 12.0 | 81.0 | 129.9 | 48.0 | 2760.0 | 732 |
| Series X, `-Safe` | 38 | 33.4 / 34.9 | 28.4 | 17.6 | 97.0 | 154.4 | 50.8 | 2745.0 | 397 |
| Series X, `-RayTracing` | 38 | 36.1 / 37.3 | 32.0 | 16.4 | 84.5 | 137.2 | 50.4 | 2760.0 | 423 |
| Series S, 70 minutes played through the prison | 210 | 39.4 / 52.3 | 31.2 | 18.4 | 49.5 | 110.2 | 50.1 | 2760.0 | 553 |

CPU seconds per minute: the game's CPU time over all its threads; 60 is one core busy all the time.
