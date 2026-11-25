# Where the changes come from

The base is [fxceless/victory-fork](https://github.com/fxceless/victory-fork) at `f85101b`. On top
of it we merged pull requests from other projects. Commits keep their original authors.

**fxceless/victory-fork:** #510, #604, #615, #644, #657, #715, #734, #735, #749, #753, #754, #756,
#757, #758, #759, #763, #764, #765, #768, #783, #784, #785, #787, #788, #789, #792, #795, #799,
#802, #803, #804, #805, #806, #807, #808, #809, #812, #824, #827, #836, #837, #838, #839, #841,
#845, #846, #853, #854, #855, #856, #857, #858, #860, #862, #864, #868
(wave 4:) #213, #255, #298, #377, #384, #409, #457, #589

**fxceless/victory-fork main:** synced up to `9c184b3` (14 commits, including the frame-rate
third session).

**Marvisak/VIctory:** #240, #443, #478, #1033, #1537, #1623, #1880, #1894, #1955, #1956, #2007,
#2012, #2037, #2102

**Belobug/VIctory-Belobug-AMD:** two list-state commits by romainhedouin (a pipeline a list binds
again is not named and queried again; an unaligned root constant buffer is copied into list scratch
once per list).

**vitutu1/victory-perf:** imported as one batch (batching, copy-back and barrier work, shader and
compute pipeline precompile, lighting and exposure fallbacks). The original repository is gone.

Plus two small fixes of our own: scalar unspill tracking (`3d13942`) and VOP3 no-op literals
(`0dbf043`).
