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

## Performance intake, round 2, 2026-11-26

Taken from the fastest projects' own code and from open pull requests, each with its original
author kept. All are GPL-3.0 sources. Each entry names the part of the frame it aims at
(measured on `db89ba3b`: heap buffers pass 45%, preparing 34%, copying back 15%). None of these
has a measured gain on VU yet; the matched run after this batch decides what stays.

- [Flamingo](https://github.com/realblastex/flamingo) `80001f1e`: the heap buffers pass looks again only at slots that changed since the last pass (full sweep every 256 passes; `VICTORY_HEAP_PASS_FULL=1` restores the old pass). It replaces the rolling 64-bucket cache this tree had.
- [victory-fork #665](https://github.com/fxceless/victory-fork/pull/665) (FJBP-code) `34499ae1`: in-flight card fetch pieces merged on several threads. Only active with `VICTORY_CARD_FETCH_INFLIGHT=1`.
- [victory-fork #1068](https://github.com/fxceless/victory-fork/pull/1068) (UwUCammie) `ee7a5666`: read-lock fast path for the shared buffer table and one lock for all heap-pass refreshes.
- [victory-fork #1032](https://github.com/fxceless/victory-fork/pull/1032) (UwUCammie) `d919676c`: guarded in-place write-back, only with `VICTORY_COPY_PLAIN=1`.
- [victory-fork #1237](https://github.com/fxceless/victory-fork/pull/1237) (Kyle Soderlund) `cdf74b56`: a thread remembers what it found for a buffer's host copy (`VICTORY_LOOKUP_CACHE=0` turns it off).
- [klbf-lab](https://github.com/PastiiJay/klbf-lab) (PastiiJay) `ca6e81cf`: new staging buffers are cut from 64 MB upload blocks (`VICTORY_STAGING_BLOCKS=0` turns it off).
- [VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) `3ead56e1`: refreshes copy only the 4 KiB parts of a page that were written (`VICTORY_PAGE_PARTS=0` turns it off).
- [AnyXboxX #304](https://github.com/LooperSalty/AnyXboxX/pull/304) (Mauro Alvarez) `c650c225`: texture uploads shared across duplicate heap views.
- [AnyXboxX](https://github.com/LooperSalty/AnyXboxX) (Radoudou) `021c8309`: card copy-back merges reuse their scratch bytes, and a short kernel read now fails instead of merging a partial piece.
- [victory-perf](https://github.com/vitutu1/victory-perf) (vitutu) `a7b2fb80`: card rehash runs joined a few pieces apart (`VICTORY_CARD_REHASH_GAP`; takes effect only with `VICTORY_CARD_REHASH_RUNS=1`).
- [victory-perf](https://github.com/vitutu1/victory-perf) (vitutu) `a37ac9a6`: a heap slot rewritten with the same descriptor key is not resolved again. The heap-pass change above replaces the code this touched, so nothing of it remains in the tree; it stays in the history for the author's credit.
- [VI-BR-PORT](https://github.com/edsuuu/VI-BR-PORT) (edsuuu) `ce6db3a7`: a heap slot that already shows its view is found without the heap-wide lock (port of victory-fork #867).
- [VItoriaBr](https://github.com/uiliannn/VItoriaBr) (uiliannn) `213fa545`: descriptor records are compared after a quick table hash.
- [Vertex](https://github.com/proneonn/Vertex) (Victor Corrêa) `11924ce6`: copies in the card's memory are queued when they change, not walked at every submission.

Two earlier picks that were missing from this list: [AnyXboxX #279](https://github.com/LooperSalty/AnyXboxX/pull/279) (Skyfrye, clock read only for summed submission steps, `6d7989f9`) and a VIctory-7900XTX change that sorts changed runs stably and joins piece runs while building them (`589e7e0d`, author `local`).

Looked at and not taken this round (too large for one safe step, conflicting with code VU already has, or off by default upstream): victory-fork #1138, Marvisak #2349, the Flamingo submission-thread work, klbf `88354fb1`, VItoriaBr `9ceb7e72`.

## Whole-branch merge: VIctory-7900XTX

[VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) `e2a28ec7` (GPL-3.0, avatarmanz and
contributors) was merged into this tree as a whole branch (`3588725d`, merged to `main` in `cce2ad11`),
not as single picks. Its commits and authors are kept. About 560 conflict hunks were resolved by hand
where both trees had changed the same lines; VU's own additions that the other branch did not have
(for example the shader cache `warm` call) were kept. One 15 minute run of the merged build is in
`docs/measurements.md`.
