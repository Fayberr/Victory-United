# Where the changes come from

Since 2026-11-26 the tree of this repository is [TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)
at commit `fab5286c` (GPL-3.0-or-later). Every commit of that history is kept with its original
author, and so is the history of this repository before the switch: the merge commit that joins
them keeps both. Vice-Tay's own list of the trees it is built from is in its
[credits](docs/credits.md) and in the [README](README.md#credits).

## Why the tree was switched

The tree this repository had before (`6757b6eff`, tagged `archive/main-2026-11-26`) was built by
merging pull requests and branches from [fxceless/victory-fork](https://github.com/fxceless/victory-fork),
[Marvisak/VIctory](https://github.com/Marvisak/VIctory), [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX),
Belobug, vitutu1/victory-perf and others (the list that was here is kept in the archive tag's
`PROVENANCE.md`). On the test machine (RTX 3080 Ti) that tree ran at 8.5 frames a second in the first room, while
Vice-Tay's own head ran at about 11 frames a second there, so the tree was switched and the other work is folded in again one piece at a time, checking the
picture after each.

## What has been taken since

Nothing yet. Each change taken from another project is listed here with its source and author when
it goes in.
