# What this repository adds

Vice-Tay carries the work of the trees it comes from (see the README's
credits) and the changes below, made and measured on one PC: RTX 4090, Ryzen 9
9950X3D, 48 GB of RAM, a 4K screen. Each change names the switch that controls it
where there is one; the default is the behaviour described.

They come over to this tree in batches, so some are still on their way: the
commit log says which are in.

## Playing

- **Full screen.** The title's window covers the whole monitor with no frame
  (`VICTORY_FULLSCREEN=1`, set by `scripts\play.ps1`; `-Windowed` leaves it out).
  **F11** or **Alt+Enter** switches between full screen and a window fitted to the
  screen while the title runs; the picture is stretched to either. DXGI's own
  Alt+Enter, which would make the swap chain exclusive full screen, is turned off.
- **Settings for an RTX 4090 / Ryzen 9 9950X3D / 48 GB** (`scripts\play.ps1
  -Machine 4090`, picked for any NVIDIA card): the thread counts and switches in
  the README's table. `-Safe` keeps the wave-wide list there: without it four
  wave64 programs do not translate on a 32-lane card and their draws are dropped.

## Picture

- **Dynamic descriptor slots are given back when the card has run a flight**, not
  when the host list is reset some 64 flights later. On a card whose
  shader-visible view heap is capped at 1,000,000 (NVIDIA), the 2^18 slots for
  tables made afresh ran out in busy scenes: hundreds of draws a frame were
  dropped, and the picture blew out to white and filled with black blobs. The
  frame line counts the slots tables take (diagnostics builds).
- **Rectangle lists** expanded on the CPU find their right angles from the element
  the input layout names `POSITION`, not from the first two-component element,
  which may be a texture coordinate.
- **Wave64 programs on a 32-lane card** (two host waves per console wave):
  - a group barrier is placed only where every console wave of the group arrives
    (a scalar branch can take one console wave of a group and not the other; the
    first then waited on the host for threads that never came);
  - an operand ending at `v255` counts `v255`;
  - a lane write (`v_writelane`) inside a region a branch skips keeps the skip.
- The translator's version is 2: the shader cache translates again what version 1
  made.

## Loading and the runtime

- **DirectStorage**: a queue submits itself when it holds half its capacity, unless
  the title created it with automatic submission off (`DSTORAGE_QUEUE_OPTIONS`).
- **Shader cache**: a shader that translated is used even when its entry cannot
  be written. `VICTORY_WAVE_WIDE=0` turns the wave-wide form off for the listed
  shaders too, and `victory warm` makes a pipeline again when the wave-wide list
  that decides its shaders changes.
- **XAsync**: a call is over by the time its result shows, so a title that begins
  the next call on the block at once is not refused.
- **Late DLLs**: a title DLL whose imports fail to bind leaves nothing loaded
  behind for the next `LoadLibrary` of its name.
- **Closing**: only a `WM_CLOSE` to the title's own window ends the title; other
  windows it shows close by themselves.
- **Saves**: a version whose manifest names a blob that is gone is still the
  save when no older version is intact (that blob reads as not found).
- **Scripted keys** (`VICTORY_KEYS_FILE`): the file is taken by renaming it before
  it is read, so a batch written meanwhile is never deleted unread.

## Measuring

- Every host list that ran is kept for reuse.
- Frame dumps (`VICTORY_CAPTURE`) are encoded on a thread of their own, and the
  ones still queued are written before the process exits.
- Diagnostics builds: `VICTORY_WALLS_TRACE=1` logs the source of each read-back;
  a placed texture view that gets no host view says why once per kind.
