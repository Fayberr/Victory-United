# Where the changes come from

The base is [fxceless/victory-fork](https://github.com/fxceless/victory-fork) at `f85101b`. On top
of it we merged pull requests from other projects. Commits keep their original authors.

**fxceless/victory-fork:** #510, #604, #615, #644, #657, #715, #734, #735, #749, #753, #754, #756,
#757, #758, #759, #763, #764, #765, #768, #783, #784, #785, #787, #788, #789, #792, #795, #799,
#802, #803, #804, #805, #806, #807, #808, #809, #812, #824, #827, #836, #837, #838, #839, #841,
#845, #846, #853, #854, #855, #856, #857, #858, #860, #862, #864, #868
(wave 4:) #213, #255, #298, #377, #384, #409, #457, #589
(wave 5:) #272, #403, #408, #410
(wave 6:) #294, #355, #277, #405, #592

**fxceless/victory-fork main:** synced up to `9c184b3` (14 commits, including the frame-rate
third session). The five newer commits (up to `6d20eed`: the card compare with a group of 64 threads
from fork #1014, a list of changed card copies, notes about unmapped blocks kept until told) are not
taken yet: they collide with the card-compare and view-guard code we have reworked, and the new paths
are off by default there.

**Marvisak/VIctory:** #240, #443, #478, #1033, #1537, #1623, #1880, #1894, #1955, #1956, #2007,
#2012, #2037, #2102
(pull requests still open there, taken 2026-11-26:) #2900, #2901, #2902, #2906

**Marvisak/VIctory main:** looked through up to `63bcca4` (history recorded in one merge that keeps our files as they are). About 130 single commits taken over (shader
lowering fixes, format and loader fixes, GPU stability fixes, dialog and input fixes), each one with the
upstream hash on the last line of its message and its original author. The large rework series (page
tables, title memory, texture staging) is not taken: our tree already has its own versions of those.

**Belobug/VIctory-Belobug-AMD:** two list-state commits by romainhedouin (a pipeline a list binds
again is not named and queried again; an unaligned root constant buffer is copied into list scratch
once per list).

**vitutu1/victory-perf:** imported as one batch (batching, copy-back and barrier work, shader and
compute pipeline precompile, lighting and exposure fallbacks). The original repository is gone.

Plus two small fixes of our own: scalar unspill tracking (`3d13942`) and VOP3 no-op literals
(`0dbf043`).

## Performance intake, 2026-11-26

- [victory-fork #1044](https://github.com/fxceless/victory-fork/pull/1044) sizes the DirectStorage pool to physical cores, up to the logical CPU capacity available to the process. VU reads the Windows record size at its documented offset and falls back to the available logical CPUs on query failure. `VICTORY_DSTORAGE_WORKERS` overrides the default with a value from 1 to 64.
- [victory-fork #1094](https://github.com/fxceless/victory-fork/pull/1094) scans sparse copy-back masks eight bytes at a time. The patch originated as [klbf-lab #26](https://github.com/PastiiJay/klbf-lab/pull/26).
- [victory-fork #1172](https://github.com/fxceless/victory-fork/pull/1172) signals the caller once after all jobs in its copy batch complete.
- [victory-fork #1185](https://github.com/fxceless/victory-fork/pull/1185) reports the full requested commit range to the GPU memory watch when Windows rounds a fixed allocation address down.
- [victory-fork #1192](https://github.com/fxceless/victory-fork/pull/1192) prepares descriptor-table records and groups dynamic views once per shader stage. Small sets of table slots and offsets stay inline during draws.
- [Flamingo #4](https://github.com/realblastex/flamingo/pull/4) contributes the in-place merge of written ranges during GPU submission.
- [Marvisak/VIctory #2795](https://github.com/Marvisak/VIctory/pull/2795) reuses short XMA2 transform stages and uses stack scratch space for common audio block sizes.
