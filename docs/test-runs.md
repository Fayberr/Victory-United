# Test runs

Measured runs, newest last. One PC: RTX 5070 12 GB (32-lane waves, driver's
own), Ryzen 7 8700F (16 threads), 32 GB, Windows 11; Series X profile. One
title, played by hand, so scenes differ between runs: compare the shape, not
the last digit. Numbers come from the layer's own frame line (about one every
five seconds), world frames only (1,000 draws and more).

## 2026-11-25

| Run | Build | Minutes | Frames/s median | best | Frame ms 90 % | 95 % | worst | Stalls of 1 s and more | Errors |
|---|---|---|---|---|---|---|---|---|---|
| 6 | main `ae10a74` | 22.9 | 5.4 | 8.1 | 1,345 | 7,174 | 21,559 | 9 | 0 |
| 8 | soldsawn 9e06958, cold pipeline cache | 18.8 | 4.9 (8.0 once pipelines were made) | 12.5 | 683 | 1,111 | 6,530 | 11 | 0 |
| 9 | the same, `SKIP_WC_COMPARE` off, warm cache | 7.5 | 5.8 | 7.7 | 389 | 528 | 1,168 | 1 | 0 |
| 10 | 8's settings, warm cache, shared copies kept (below) | 10.8 | 6.7 | 10.6 | 496 | 1,054 | 2,733 | 5 | 0 |
| 11 | first unified build (`unify/all-forks` ecade07) | 20.1 | 6.8 | 12.9 | 478 | 1,042 | 4,648 | 9 | 0 |
| 12 | + water copied back, marked-slot heap pass | 25.3 | 6.9 | 11.2 | 283 | 548 | 13,362 | 7 | 0 |
| 14 | + view slots made side by side, dispatches in parts, NaN guard (a recorder ran beside it) | 8.2 | 7.3 | 10.5 | 201 | 366 | 840 | 0 | 0 |
| 15 | `--work gpu`, windows of 1 MiB, timing on | 9.3 | 7.5 | 12.8 | 252 | 551 | 3,464 | 2 | 0 |
| 17 | ray-tracing structure builder on, memory declared by address only | 3.5 | 3.2 | 4.3 | 1,188 | 1,883 | 1,883 | 2 | 0 |

Run 11's first five minutes shared the processor with builds and tests; its
numbers are low for that part. By three-minute part, frames/s median: 4.8,
5.8, 8.0, 5.5, 5.8, 7.5, 7.8.

First world frame after the start (with the wait for a button in it):
425 s (6), 106 s (8), 75 s (9), 166 s (10), 97 s (11). Pipelines made while
running: about 2,190 (8, cold), 0 (9), 0 at the start and 371 later (11).

### Where a frame goes

| Per frame, median | Run 8 | Run 9 | Run 10 | Run 11 |
|---|---|---|---|---|
| Fetched back from the card | 1.4 MB | 85 MB | 8.9 MB | 8.9 MB |
| Sent up to the card | 18 MB | 85 MB | 24 MB | 24 MB |
| Submitting thread waits for the card | 12 ms | 62 ms | 21 ms | 21 ms |
| Preparing (of it copies) | 68 (51) ms | 67 (47) ms | 75 (64) ms | 73 (58) ms |
| The pass over the view heap | 41 ms | 36 ms | 35 ms | 39 ms |

Neither side is full. A four-second look in run 11: 4.6 of 16 threads busy,
the busiest thread at 56 %; the card at 38 % and 84 W of 250 W at its full
clock. What limits the frame is one step waiting for the next.

### Findings

- **Heads and necks missing (run 8).** Present in runs 9, 10 and 11. Two
  candidates, not yet told apart: shader writes that stay on the card and do
  not reach other copies of the same memory (10 and 11 have a change for it),
  and one-time compute passes dropped while pipelines are made (run 8 alone
  had a cold cache). The control run, warm cache and the change off, is open.
- **Once a frame the submitting thread waits 21 ms (43 at the ninetieth of a
  hundred)** for a title buffer copy that touches memory flights may write.
  A switch that runs such copies on the card is written, not measured.
- **Card memory is full** within two minutes; 7,000 to 12,000 textures wait
  while the largest level of every texture is left out already. 330 GB of
  texture uploads in run 11's twenty minutes.
- **Boats sink.** The title reads a height map its compute passes write; it
  stayed on the card. Ported after run 11 from PastiiJay's klbf fork
  (ebdbcc3, ee7081b); being tested.
- Ray tracing structure builder: off, its stages are dropped (about 220 a
  frame at first, 450 later).
- Open faults seen: tile-shaped yellow halos round light patches while the
  camera moves, dark tiles on water, shade that is close to black, walls
  that come in late after a change of scene, about 35 s of black between two
  scenes.
- Keyboard and mouse carried run 11 for twenty minutes: no pad is needed.

### Tests

Unified tree: 234 of 234 GPU tests, 34 of 34 format tests, 122 and 10 of the
rest pass (5 and 6 are left out by design).

### Later the same day (runs 12 to 17)

- **Boats float** (run 12): the height map comes back to the title.
- **Run 13** was started with every saved pipeline invalid (a change to all
  pixel programs): 2,259 were made in the title at about two a second, with a
  black picture. The pre-build does the same in two to four minutes with the
  title closed; it has to run after any change of the translator.
- **A usual frame of 123 ms (3,700 draws, run 15):** submitting thread 108 ms
  at work (preparing 67, waiting for the card 23), the title's own passes 79
  ms of card time in 191 lists, our extra passes 0.8 ms, the card idle 25 ms.
  No pass is large: the dearest takes 2.6 ms.
- **No gain:** `VICTORY_REFRESH_ONCE` with `VICTORY_VIEW_KNOWN` (25 readings
  each way: 128 ms off, 138 on), `--work gpu` with windows of 1 MiB.
- **Ray tracing:** with the thread counts of `DispatchX`, per-list watermarks
  of numbered copies and bounded builder writes the structure builder runs
  whole (53 programs) and the title went on for as long as it was played (a
  few minutes, no fault). Run 16: 2 to 2.5 s a frame, because 8.1 GB of
  windows were declared written by programs that cannot store by address. Run
  17 without that: 200 to 240 ms in a cutscene, of it 43 ms on the card.
- **Characters black for some seconds after a cut:** their textures had not
  been uploaded yet. Open.

### Later the same day (runs 18 to 21)

One build, `d63ad3f` (`FX3` and `ShakiraRED` merged). Speeds are the median
over frame lines with 2,500 draws and more. The scenes were played by hand and
differ, so small differences mean nothing.

| Run | Added to the base | World readings | fps | 90 % under (ms) | Result |
|---|---|---|---|---|---|
| 18 | ray tracing, `TABLE_BUFFERS`, `TEXTURE_HEAPS`, `STAGING_BLOCKS`, `SECTION_MB=2`, `TEXTURE_ROOM_PERCENT=96`, `DISPLAY_MODE=2560x1440`, `FRAME_LOG`, `CARD_METER` | 0 | | | **hung at the first ray-traced frames**; the title ended itself after about 30 s |
| 19 | nothing | 29 | 9.9 | 216 | closed normally; more than half of the readings are one steady scene |
| 20 | `DISPLAY_MODE=2560x1440` | 21 | 7.8 | 156 | closed normally |
| 21 | `TABLE_BUFFERS=1` | 30 | 7.3 | 265 | closed normally; no gain, not kept |

- **Run 18's hang has no known cause.** The title's report names a wait between
  queues that was never signalled; our log has no error and no lost device.
  Runs 19 to 21 (ray tracing off) did not hang, so the merge alone is not
  enough to cause it. The switches are being turned on one per run. **Ray
  tracing together with the merged build is untested apart from run 18.**
- `DISPLAY_MODE` changes the mode the title is told; the window's buffer stayed
  3840x2160 in run 20 as in 19.
- Open, seen in pictures: the sea is wholly black in some scenes with every
  setting so far; eyes are blotched white in some shots and right in others;
  mouths are reported not to move; characters are coarse in some cutscenes;
  clouds differ in detail between runs 19 and 20 (one picture each, not proven).
- The title holds about 7.6 GB of an 11.2 GB card budget while about 4,600
  textures still wait (run 19's end).
