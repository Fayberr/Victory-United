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

The `vibr/main` fold records [edsuuu/VI-BR-PORT](https://github.com/edsuuu/VI-BR-PORT)
through `31b9a82fb260f9bad6acc41b47ec4e0571e8febb` (GPL-3.0-or-later). Its 25 new commits
keep their original authors in the merge history. The new code joins adjacent GPU compare ranges,
changes the small-card buffer defaults, makes skipping ray-only shadow passes opt-in, improves
pipeline failure logs, and lets the air-light bound work with constant ambient light. The play
profile also takes the buffer threshold and lighting settings.

Existing local shader, descriptor and memory-mapping implementations were retained where they
already contained these fixes or added later corrections. Upstream README claims, Brazil captions,
title icons and the duplicate key-file reader were omitted.

### NVIDIA and Avatar fold, 2026-11-26

This fold starts at `777c75b67`. It records two separate merges, in this order:

- [trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA),
  `nvidia/main` at `6a31e431e`, merge `41b2e8856`: superboynick's mouse device
  (`12b1d16d3`), Indy2l's texture read-back worker (`2d1dc3826`, already present
  in this tree), descriptor view-table reporting (`b8fcd99b9`, author recorded
  upstream as Local Test), and the NVIDIA launcher BAR cap changes by Indy2l,
  Jeremy Goldberg and trustno1-coder. All eleven upstream commits are ancestors.
- [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX),
  `avatar/main` at `014d52ded`, merge `bfce5cb43`: all seven new commits
  `e9428969f`, `5d6bd8418`, `cd8dfc06a`, `1e7286109`, `84917c34f`,
  `d13e9e0c6`, `014d52ded`. beloooo's submission-prelude texture uploads and
  optional PC FidelityFX port (originally DexNom `ab34a8b2`) are included,
  along with the upstream RT port and its diagnostics/ignored GPU benchmark.
  The RT commits' author is recorded upstream as local.

Both sources retain their original history and attribution. No upstream commit
was excluded from ancestry.

The choices made while merging these, and the switches they carried, are in the git history of this
repository before 2026-11-28 (the old line is joined into the current one by a merge commit that keeps
all of it).

# Streamline integration provenance

The Streamline integration in Victory United is derived from
[whiteh4cker-tr/VIctory_sm86](https://github.com/whiteh4cker-tr/VIctory_sm86)
under GPL-3.0-or-later. Copyright remains with the original authors. See [LICENSE](LICENSE)
and [credits](docs/credits.md).

| Area | Source |
| --- | --- |
| Streamline Super Resolution ABI, camera constants and evaluation | readmet3xt-lab in `8756f6ac5`, part of `9296e0329` |
| FidelityFX entry and Series X profile | readmet3xt-lab in `88e372cff` and `ab2ba97e9`, building on DexNom/VIctory; this branch uses Victory United's existing FidelityFX bridge and profile |
| Frame generation presentation and scene capture | readmet3xt-lab in `8756f6ac5`, `2abeba3b9` and `1d3040027`, merged in `11509be9d` |
| Ampere and Turing add-on loader and CUDA adapter selection | Alp Eren Demir (whiteh4cker-tr) in `b4686122a` |
| Setup reference | readmet3xt-lab in `b4da68cf4` |

The NVIDIA runtime libraries and the optional dlssg_for_sm86 add-on are
supplied by the user. NVIDIA retains rights to its libraries and kernels.
