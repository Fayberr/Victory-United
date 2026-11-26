# Play log: Series S, played by hand

Build 672cf831 (diag), the test PC in the README, `scripts\play.ps1 -Console series-s`
with the RTX 4090 settings, full screen, the whole session recorded to video on the
card's encoder. Played by hand through the scripted pad (`VICTORY_KEYS_FILE`), looking
at the screen before each move. The run ended after 70 minutes in the world.
The [measurement summary](measurements.md) covers the full session.

Loading: 88 s to the start screen, 126 s into the world.

## Minute by minute

Values come from recorded in-world frames with over 1,000 draws. Each row uses
the median for its UTC minute, with the minimum where shown.

| UTC | Where and what | fps | preparing ms | copying back ms | draws |
|---|---|---|---|---|---|
| 09:11 | Opening scene, then the dormitory | 17.0 | 49.2 | 17.0 | 2,986 |
| 09:12 | Dormitory, camera turned around | 20.7 | 37.5 | 16.4 | 4,561 |
| 09:13 | Walking to a cellmate, as the objective asks | 21.5 | 36.0 | 14.5 | 3,391 |
| 09:14 | The door opens, a guard comes in | 18.5 | 37.7 | 16.0 | 5,138 |
| 09:15 | Out of the dormitory, down the corridor behind the guard | 19.9 | 38.0 | 18.7 | 3,904 |
| 09:16 | Through a gate into a common room with other inmates | 24.5 | 31.2 | 13.6 | 2,288 |
| 09:17 | Waiting at a closed gate for the guard | 22.6 | 34.2 | 13.9 | 2,799 |
| 09:18 | Another corridor, into an office: a scene starts | 20.9 | 38.2 | 17.0 | 2,034 |
| 09:19 | A flashback starts, out in the city | 19.4 | 40.8 | 14.4 | 3,953 |
| 09:20 | Flashback, playable: aiming with the left trigger | 16.5 | 47.0 | 15.8 | 6,213 |
| 09:21 | Aiming with the right stick, threatening with X | 18.7 | 41.6 | 14.8 | 4,369 |
| 09:22 | Holding the aim while the scene plays | 19.3 | 39.8 | 14.3 | 4,364 |
| 09:23 | Running out to the street, jumping, sprinting | 13.4 | 54.6 | 11.4 | 9,267 |
| 09:24 | On a roof over the open city, wanted by the police | 12.3 | 48.5 | 16.9 | 4,216 |
| 09:25 | A scene in a car, then back in the office | 24.3 | 32.3 | 12.8 | 1,406 |

When the open city first came into view (09:23:49) one second ran at 5.0 fps
(200 ms a frame).

During those minutes: about 32-33 GB of private memory for the game, 15.3 GB of card
memory, 112-127 W and 45-46 C on the card.

## What worked

- Every pad input tried answers: both sticks, the left trigger held, X, A to jump or
  climb, the left stick click to run. X needs short taps; one long press was missed.
- The story moves from one checkpoint to the next without help: dormitory, corridor,
  gates, office, flashback, escape, the open city, and back.
- No crash, no hang.

## Picture defects seen

- A flat red patch behind a character standing in a doorway.
- In the flashback, clothes and people in the background turn a flat, blown-out white with
  coloured fringes, and the street and sky blow out. The flashback has a sepia grade of its
  own, but the flat white and the fringes look like the overexposure defect.
- Eyes lit white in a close-up.
- Skin and hair in flat bands of colour.
- Full-white frames between scenes are the game's own transitions: the next frame is right.

## Part 2: the boat (09:26 to 09:38 UTC)

| UTC | Where and what | fps | lowest second | preparing ms | copying back ms | draws |
|---|---|---|---|---|---|---|
| 09:26 | Office scene, then a cut to a scene on a boat | 12.7 | 3.5 | 41.4 | 13.4 | 2,740 |
| 09:27 | Driving the boat on open water, minimap on screen | 18.6 | 10.5 | 37.8 | 14.2 | 3,092 |
| 09:28 | Full throttle past other boats, a long bridge, a plane overhead | 18.2 | 14.6 | 38.9 | 15.1 | 3,736 |
| 09:29 | Off course round an island; back to a checkpoint with a new objective | 19.1 | 13.0 | 38.3 | 12.2 | 3,554 |
| 09:30 | Ran into mangroves: mission failed, the boat wrecked | 18.2 | 10.5 | 40.2 | 12.3 | 3,782 |
| 09:31 | Retried from the checkpoint, round the island | 17.1 | 10.1 | 40.5 | 12.8 | 4,346 |
| 09:32 | Mangroves again, failed; skipped the checkpoint | 18.4 | 13.6 | 39.5 | 12.1 | 3,766 |
| 09:33 | New objective with a GPS route on the minimap | 18.2 | 15.0 | 39.0 | 11.8 | 3,870 |
| 09:34 | Under an iron bridge, into the bushes | 18.9 | 16.9 | 38.0 | 11.6 | 4,348 |
| 09:35 | Failed, skipped the checkpoint | 17.2 | 3.6 | 41.6 | 13.2 | 4,694 |
| 09:36 | Short, careful legs: iron bridge, a canal | 16.5 | 9.7 | 42.0 | 13.4 | 5,007 |
| 09:37 | Past a ruined bridge onto a lake | 17.1 | 12.9 | 41.6 | 14.1 | 4,562 |
| 09:38 | Reached the objective, a scene starts | 16.4 | 14.8 | 42.1 | 14.3 | 4,658 |

Game memory rose to 35-36 GB on open water (from 33 GB in the prison); card memory
17.7-18.5 GB; 112-132 W and 43-46 C on the card.

What this part showed:

- Driving answers to the triggers and the left stick; the minimap, the GPS route and the
  distance to the objective update as it moves.
- The fail screen works: skipping the checkpoint was tried and works, and with no
  choice it retries from the checkpoint by itself.
- Two failures were the driver's: steering in one- and two-second steps at 15-19 fps is
  coarse. Three- to five-second legs with a look between them got there.

More picture defects:

- Light-blue flat rectangles and square blocks in the clouds, coming and going with the
  camera.
- Flat purple patches in tree tops.
- The reflection on the boat's dark hull in square blocks.
- One-second drops to 3.5 and 3.6 fps when a new area loads (the cut to the boat scene,
  skipping a checkpoint).

## Part 3: binoculars, a camp, a shoot-out (09:39 to 09:51 UTC)

| UTC | Where and what | fps | lowest second | preparing ms | copying back ms | draws |
|---|---|---|---|---|---|---|
| 09:39 | The second lead on the boat; the weapon wheel held open | 15.8 | 15.3 | 42.5 | 13.6 | 5,002 |
| 09:40 | Picking binoculars with the right stick, zooming with the d-pad | 14.1 | 7.8 | 42.9 | 14.1 | 6,024 |
| 09:41 | Watching a deal on a jetty through the binoculars | 14.1 | 9.7 | 41.7 | 13.7 | 6,068 |
| 09:42 | A scene on the boat, arriving at a camp | 13.5 | 11.1 | 43.7 | 15.2 | 6,384 |
| 09:43 | On foot, looking for someone in the camp | 13.2 | 12.6 | 40.6 | 13.1 | 7,138 |
| 09:44 | Through brush; the pause menu (30 fps there) | 15.1 | 10.8 | 43.1 | 13.4 | 4,591 |
| 09:45 | The 3D map from the pause menu, back to the game | 14.3 | 13.1 | 43.2 | 12.2 | 5,682 |
| 09:46 | Among trailers; a grab prompt near a person | 13.5 | 9.8 | 41.6 | 12.3 | 5,709 |
| 09:47 | A corridor of cages; hurt | 13.9 | 12.7 | 43.8 | 14.3 | 6,344 |
| 09:48 | A shoot-out: taking cover, drawing a weapon | 13.7 | 12.2 | 47.0 | 16.6 | 5,974 |
| 09:49 | Health critical, then dead | 12.6 | 10.1 | 55.3 | 17.4 | 7,081 |
| 09:50 | Back at the checkpoint with full health | 12.0 | 9.8 | 39.4 | 14.4 | 6,824 |
| 09:51 | Back to the camp | 12.3 | 11.6 | 47.5 | 14.8 | 6,412 |

**The game's memory keeps climbing:** 33 GB in the prison, 35-36 GB on the water, then
36.6 GB at 09:39 and 41.5 GB at 09:49, about 0.4-0.5 GB a minute in the camp. Card memory
stayed flat (17.8-18.1 GB). It runs on with the large page file, but whether this is
something held and never let go or a cache that grows with each new area is the next
thing to find out.

What worked: the weapon wheel, binoculars with zoom, the pause menu and the 3D map,
going back out of menus, taking cover, drawing a weapon, taking damage, dying and coming
back at the checkpoint, the prompt to grab a person.

More picture defects:

- Dark polygons covering corners of the 3D map.
- A plastic tank in flat, glowing purple; plants with a flat pink glow.
- A flat black shape with no detail at all (the black-blob defect).

## Part 4: a barn, a fight, a shoot-out (09:52 to 10:06 UTC)

| UTC | Where and what | fps | lowest second | preparing ms | copying back ms | draws |
|---|---|---|---|---|---|---|
| 09:52 | A destination set on the map | 12.2 | 9.8 | 43.9 | 14.4 | 6,276 |
| 09:53 | Checkpoint restarted from the pause menu | 12.8 | 12.1 | 41.9 | 13.4 | 5,372 |
| 09:54 | Jetty, trailers, the corridor of cages | 10.7 | 10.2 | 40.5 | 14.1 | 7,016 |
| 09:55 | Shoot-out: draw, aim, fire; one down | 10.7 | 8.5 | 38.5 | 15.6 | 7,025 |
| 09:56 | Out of ammo, reload; dead, checkpoint skipped | 11.2 | 9.2 | 64.0 | 20.2 | 6,667 |
| 09:57 | A fist fight in a barn, won | 11.5 | 7.6 | 68.4 | 20.8 | 6,662 |
| 09:58 | Picking up an item, healing with it | 10.1 | 7.3 | 79.7 | 28.6 | 6,700 |
| 09:59 | Dead again, back to the checkpoint | 10.5 | 4.4 | 62.3 | 20.6 | 6,762 |
| 10:00 | The item again, out of the barn with an ally | 10.6 | 9.1 | 66.9 | 23.4 | 7,300 |
| 10:01 | The item again, out of the barn with an ally | 10.0 | 3.7 | 70.5 | 21.3 | 7,333 |
| 10:02 | The item again, out of the barn with an ally | 10.1 | 9.6 | 64.3 | 23.3 | 7,210 |
| 10:03 | The item again, out of the barn with an ally | 10.5 | 8.7 | 57.1 | 20.6 | 7,353 |
| 10:04 | A shoot-out among trucks and quad bikes | 10.4 | 9.3 | 65.1 | 21.8 | 6,376 |
| 10:05 | A shoot-out among trucks and quad bikes | 9.4 | 7.8 | 74.1 | 20.2 | 7,727 |
| 10:06 | A shoot-out among trucks and quad bikes | 8.0 | 7.2 | 73.0 | 22.1 | 9,579 |

### The finding of this session: it slows down the longer it runs

- The game's private memory went 33 GB (prison), 36 GB (open water), 41.5 GB (09:49),
  46 GB (09:57) and **47.8 GB at 10:05**, past the **43.4 GB of RAM Windows sees**: from
  there part of the game lives in the page file.
- Over the same stretch the time spent **preparing** each frame doubled (38-43 ms to
  64-82 ms) and copying back rose (13-15 ms to 20-29 ms), while the card stayed where it
  was (18.0 GB, 100-115 W). The per-minute median fell to 8.0 fps by 10:06 UTC.
- The likely cause, still to be shown: something kept for each new resource or area is
  never let go (a leak, or a cache with no ceiling), and once past physical RAM frame
  preparation waits on the disk.
- Next: find where the memory goes (per-category memory log, a memory map), then run a
  long session again with a ceiling on that cache and see whether the frame rate holds.

What worked in this part: setting a destination on the map, restarting a checkpoint from
the pause menu, skipping one from the death screen, a fist fight, picking up an item and
healing with it, drawing, aiming, firing and reloading, an ally that follows, dropped
cash, enemies marked down on the minimap; several deaths and retries with no hang.

More picture defects: a skylight in flat, glowing blue; a window in flat purple.

## End of the session (10:07 to 10:22 UTC)

Frame samples continued until 10:22 UTC. The final minute contains one sample.

| UTC | fps | preparing ms | copying back ms | world lines |
|---|---|---|---|---|
| 10:07 | 7.5 | 84.1 | 26.1 | 11 |
| 10:08 | 9.3 | 78.9 | 25.1 | 11 |
| 10:09 | 10.9 | 64.7 | 23.4 | 11 |
| 10:10 | 12.8 | 59.9 | 18.0 | 10 |
| 10:11 | 8.9 | 85.6 | 27.4 | 11 |
| 10:12 | 9.2 | 81.2 | 24.4 | 11 |
| 10:13 | 8.9 | 82.7 | 24.3 | 12 |
| 10:14 | 8.2 | 89.1 | 27.0 | 11 |
| 10:15 | 6.7 | 108.1 | 26.0 | 10 |
| 10:16 | 9.1 | 83.4 | 21.4 | 10 |
| 10:17 | 8.7 | 82.4 | 24.7 | 11 |
| 10:18 | 9.6 | 74.8 | 23.2 | 12 |
| 10:19 | 9.5 | 76.7 | 25.9 | 11 |
| 10:20 | 10.6 | 69.7 | 20.4 | 11 |
| 10:21 | 8.4 | 85.2 | 21.3 | 12 |
| 10:22 | 7.9 | 91.0 | 21.1 | 1 |

## Still to check

- The log's "on the GPU" figure reads 0-2 ms on the Series S profile, against 33-36 ms on
  Series X. It is left out above until it is clear what it measures there.
